use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::runtime::io_timestamp::IoTimestamp;

#[derive(Clone, Copy, Debug, Default)]
pub struct ModifierState {
    pub caps_lock: bool,
    pub lshift: bool,
    pub rshift: bool,
    pub lctrl: bool,
    pub rctrl: bool,
    pub lalt: bool,
    pub ralt: bool,
    pub lgui: bool,
    pub rgui: bool,
    pub fn_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FnTrackingMode {
    #[default]
    Unavailable,
    Probing,
    Unreliable,
    Reliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEventKind {
    KeyDown,
    KeyUp,
    ModChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub timestamp: Option<IoTimestamp>,
    pub kind: InputEventKind,
    pub alias: String,
    pub keycode: Option<u16>,
    pub state_keys: Vec<InputKeyState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InputKeyState {
    pub alias: String,
    pub keycode: Option<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct InputSnapshot {
    pub active: bool,
    pub keys_down: Vec<InputKeyState>,
    pub mods: ModifierState,
    pub fn_mode: FnTrackingMode,
}

#[derive(Debug)]
struct SharedState {
    tap_active: bool,
    hid_active: bool,
    hid_monitor_started: bool,
    capture_enabled: bool,
    fail_open_mode: bool,
    consumer_last_heartbeat: Instant,
    recreate_requested: bool,
    debug_enabled: bool,
    naive_mod_detect: bool,
    keys_down: BTreeMap<u16, String>,
    inferred_modifier_keycodes: BTreeSet<u16>,
    rejected_modifier_keycodes: BTreeSet<u16>,
    last_modifier_flags: Option<u64>,
    hid_consumer_states: BTreeMap<u32, bool>,
    hid_consumer_active: bool,
    injection_ts_by_usage: BTreeMap<u32, Option<u64>>,
    pending_events: VecDeque<InputEvent>,
    probe_pending_keycode: Option<u16>,
    probe_started_at: Option<Instant>,
    probe_alias_by_keycode: BTreeMap<u16, String>,
    deferred_unresolved_keydowns: BTreeSet<u16>,
    local_keydown_alias_lookup: BTreeMap<String, Instant>,
    mods: ModifierState,
    fn_mode: FnTrackingMode,
    fn_last_observed: Option<bool>,
}

#[derive(Debug, Clone)]
struct LocalSdlKeydown {
    alias: String,
    repeat: bool,
    ts: IoTimestamp,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            tap_active: false,
            hid_active: false,
            hid_monitor_started: false,
            capture_enabled: false,
            fail_open_mode: false,
            consumer_last_heartbeat: Instant::now(),
            recreate_requested: false,
            debug_enabled: false,
            naive_mod_detect: false,
            keys_down: BTreeMap::new(),
            inferred_modifier_keycodes: BTreeSet::new(),
            rejected_modifier_keycodes: BTreeSet::new(),
            last_modifier_flags: None,
            hid_consumer_states: BTreeMap::new(),
            hid_consumer_active: false,
            injection_ts_by_usage: BTreeMap::new(),
            pending_events: VecDeque::new(),
            probe_pending_keycode: None,
            probe_started_at: None,
            probe_alias_by_keycode: BTreeMap::new(),
            deferred_unresolved_keydowns: BTreeSet::new(),
            local_keydown_alias_lookup: BTreeMap::new(),
            mods: ModifierState::default(),
            fn_mode: FnTrackingMode::Unavailable,
            fn_last_observed: None,
        }
    }
}

const MAX_PENDING_EVENTS: usize = 512;
const PROBE_TIMEOUT: Duration = Duration::from_millis(4);
const CONSUMER_WATCHDOG_POLL: Duration = Duration::from_millis(100);
const CONSUMER_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(4);

macro_rules! global_key_log {
    ($($arg:tt)*) => {
        println!(
            "[global_input ts={}] {}",
            IoTimestamp::current_time().raw(),
            format!($($arg)*)
        );
    };
}

fn queue_event(guard: &mut SharedState, event: InputEvent) {
    let mut event = event;
    event.state_keys = guard
        .keys_down
        .iter()
        .filter(|(kc, alias)| should_expose_key_state(guard, **kc, alias))
        .map(|(kc, alias)| InputKeyState {
            alias: alias.clone(),
            keycode: Some(*kc),
        })
        .collect();
    guard.pending_events.push_back(event);
    while guard.pending_events.len() > MAX_PENDING_EVENTS {
        let _ = guard.pending_events.pop_front();
    }
}

fn should_expose_key_state(guard: &SharedState, keycode: u16, alias: &str) -> bool {
    !alias.starts_with("KC") || guard.probe_alias_by_keycode.contains_key(&keycode)
}

fn prune_stale_local_aliases(guard: &mut SharedState) {
    guard
        .local_keydown_alias_lookup
        .retain(|_, seen_at| seen_at.elapsed() <= PROBE_TIMEOUT);
}

fn freshest_local_alias(guard: &mut SharedState) -> Option<String> {
    prune_stale_local_aliases(guard);
    guard
        .local_keydown_alias_lookup
        .iter()
        .max_by_key(|(_, seen_at)| *seen_at)
        .map(|(alias, _)| alias.clone())
}

fn expire_probe_if_timed_out(guard: &mut SharedState) {
    let Some(kc) = guard.probe_pending_keycode else {
        return;
    };
    let Some(started_at) = guard.probe_started_at else {
        return;
    };
    if started_at.elapsed() < PROBE_TIMEOUT {
        return;
    }
    guard.probe_pending_keycode = None;
    guard.probe_started_at = None;
    let _ = guard.deferred_unresolved_keydowns.remove(&kc);
    let _ = guard.keys_down.remove(&kc);
    prune_stale_local_aliases(guard);
    if guard.debug_enabled {
        global_key_log!("probe_timeout_drop kc={} timeout_ms={}", kc, PROBE_TIMEOUT.as_millis());
    }
}

fn apply_probe_alias_lock(guard: &mut SharedState, sdl_alias: &str) -> Option<(u16, String)> {
    let kc = guard.probe_pending_keycode.take()?;
    guard.probe_started_at = None;
    let alias = sdl_alias.to_string();
    guard.probe_alias_by_keycode.insert(kc, alias.clone());
    if let Some(current_alias) = guard.keys_down.get_mut(&kc) {
        *current_alias = alias.clone();
    }
    for ev in &mut guard.pending_events {
        if ev.keycode == Some(kc) {
            ev.alias = alias.clone();
        }
        for state_key in &mut ev.state_keys {
            if state_key.keycode == Some(kc) {
                state_key.alias = alias.clone();
            }
        }
    }
    if guard.deferred_unresolved_keydowns.remove(&kc) && guard.keys_down.contains_key(&kc) {
        queue_event(
            guard,
            InputEvent {
                timestamp: Some(IoTimestamp::current_time()),
                kind: InputEventKind::KeyDown,
                alias: alias.clone(),
                keycode: Some(kc),
                state_keys: Vec::new(),
            },
        );
    }
    Some((kc, alias))
}

fn clear_capture_runtime_state(guard: &mut SharedState) {
    guard.keys_down.clear();
    guard.mods = ModifierState::default();
    guard.last_modifier_flags = None;
    guard.hid_consumer_states.clear();
    guard.hid_consumer_active = false;
    guard.probe_pending_keycode = None;
    guard.probe_started_at = None;
    guard.deferred_unresolved_keydowns.clear();
    guard.local_keydown_alias_lookup.clear();
    guard.injection_ts_by_usage.clear();
}

fn capture_is_effectively_enabled(guard: &SharedState) -> bool {
    guard.capture_enabled && !guard.fail_open_mode
}

pub struct GlobalInputCapture {
    shared: Arc<Mutex<SharedState>>,
    local_key_tx: Sender<LocalSdlKeydown>,
    #[cfg(target_os = "macos")]
    control_tx: Sender<macos::ControlMessage>,
}

impl GlobalInputCapture {
    pub fn start() -> Self {
        Self::start_with_options(false, false)
    }

    pub fn start_with_debug(debug: bool) -> Self {
        Self::start_with_options(debug, false)
    }

    pub fn start_with_options(debug: bool, _naive_mod_detect: bool) -> Self {
        let shared = Arc::new(Mutex::new(SharedState::default()));
        let (local_key_tx, local_key_rx) = channel::<LocalSdlKeydown>();
        if let Ok(mut guard) = shared.lock() {
            guard.debug_enabled = debug;
            guard.naive_mod_detect = true;
        }
        start_local_probe_resolver_thread(shared.clone(), local_key_rx);
        start_consumer_watchdog_thread(shared.clone());
        #[cfg(target_os = "macos")]
        let control_tx = macos::start_event_tap(shared.clone(), debug);
        #[cfg(not(target_os = "macos"))]
        {}
        Self {
            shared,
            local_key_tx,
            #[cfg(target_os = "macos")]
            control_tx,
        }
    }

    pub fn snapshot(&self) -> InputSnapshot {
        if let Ok(mut guard) = self.shared.lock() {
            expire_probe_if_timed_out(&mut guard);
            let to_stamp: Vec<u32> = guard
                .injection_ts_by_usage
                .iter()
                .filter_map(|(usage, ts)| if ts.is_none() { Some(*usage) } else { None })
                .collect();
            if !to_stamp.is_empty() {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(d) => d.as_nanos() as u64,
                    Err(_) => 0,
                };
                for usage in to_stamp {
                    if let Some(ts_slot) = guard.injection_ts_by_usage.get_mut(&usage) {
                        *ts_slot = Some(now);
                        if guard.debug_enabled {
                            global_key_log!("injection_ts_assigned usage=0x{:X} ts={}", usage, now);
                        }
                    }
                }
            }
            if guard.probe_pending_keycode.is_some() {
                return InputSnapshot {
                    active: (guard.tap_active || guard.hid_active) && capture_is_effectively_enabled(&guard),
                    keys_down: Vec::new(),
                    mods: guard.mods,
                    fn_mode: guard.fn_mode,
                };
            }
            return InputSnapshot {
                active: (guard.tap_active || guard.hid_active) && capture_is_effectively_enabled(&guard),
                keys_down: guard
                    .keys_down
                    .iter()
                    .filter(|(kc, alias)| should_expose_key_state(&guard, **kc, alias))
                    .map(|(kc, alias)| InputKeyState {
                        alias: alias.clone(),
                        keycode: Some(*kc),
                    })
                    .collect(),
                mods: guard.mods,
                fn_mode: guard.fn_mode,
            };
        }
        InputSnapshot::default()
    }

    pub fn next_event_before(&self, deadline: IoTimestamp) -> Option<InputEvent> {
        if let Ok(mut guard) = self.shared.lock() {
            expire_probe_if_timed_out(&mut guard);
            if guard.probe_pending_keycode.is_some() {
                return None;
            }
            let ready = if let Some(front) = guard.pending_events.front() {
                front.timestamp.is_none_or(|ts| ts <= deadline)
            } else {
                false
            };
            if ready {
                return guard.pending_events.pop_front();
            }
        }
        None
    }

    pub fn try_lock_probe_alias(&self, sdl_alias: &str) -> Option<(u16, String)> {
        if let Ok(mut guard) = self.shared.lock() {
            if guard.debug_enabled {
                let pending = guard
                    .probe_pending_keycode
                    .map(|kc| kc.to_string())
                    .unwrap_or_else(|| "none".to_string());
                let age_us = guard
                    .probe_started_at
                    .map(|t| t.elapsed().as_micros().to_string())
                    .unwrap_or_else(|| "na".to_string());
                global_key_log!(
                    "probe_lock_attempt sdl_alias={} pending_kc={} age_us={}",
                    sdl_alias,
                    pending,
                    age_us
                );
            }
            expire_probe_if_timed_out(&mut guard);
            let locked = apply_probe_alias_lock(&mut guard, sdl_alias);
            if let Some((kc, alias)) = &locked
                && guard.debug_enabled
            {
                global_key_log!("probe_locked kc={} sdl_alias={}", kc, alias);
            }
            return locked;
        }
        None
    }

    pub fn on_local_sdl_keydown_for_probe(&self, sdl_alias: &str, repeat: bool) {
        let evt = LocalSdlKeydown {
            alias: sdl_alias.to_string(),
            repeat,
            ts: IoTimestamp::current_time(),
        };
        let _ = self.local_key_tx.send(evt);
    }

    pub fn note_consumer_heartbeat(&self) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.consumer_last_heartbeat = Instant::now();
            if guard.fail_open_mode {
                guard.fail_open_mode = false;
                if guard.debug_enabled {
                    global_key_log!("watchdog_recovered fail_open=false");
                }
            }
        }
    }
}

fn start_local_probe_resolver_thread(shared: Arc<Mutex<SharedState>>, rx: Receiver<LocalSdlKeydown>) {
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if let Ok(mut guard) = shared.lock() {
                guard
                    .local_keydown_alias_lookup
                    .insert(event.alias.clone(), Instant::now());

                let pending_before = guard
                    .probe_pending_keycode
                    .map(|kc| kc.to_string())
                    .unwrap_or_else(|| "none".to_string());
                let age_before_us = guard
                    .probe_started_at
                    .map(|t| t.elapsed().as_micros().to_string())
                    .unwrap_or_else(|| "na".to_string());
                if guard.debug_enabled {
                    global_key_log!(
                        "local_probe_eval alias={} repeat={} pending_kc={} age_us={} sdl_ts={}",
                        event.alias,
                        event.repeat,
                        pending_before,
                        age_before_us,
                        event.ts.raw()
                    );
                }

                expire_probe_if_timed_out(&mut guard);
                if guard.probe_pending_keycode.is_none() {
                    if guard.debug_enabled {
                        global_key_log!(
                            "local_probe_skip alias={} reason=no_pending_probe",
                            event.alias
                        );
                    }
                    continue;
                }
                let locked = apply_probe_alias_lock(&mut guard, &event.alias);
                if let Some((kc, alias)) = locked {
                    if guard.debug_enabled {
                        global_key_log!("local_probe_locked kc={} sdl_alias={}", kc, alias);
                    }
                } else if guard.debug_enabled {
                    global_key_log!(
                        "local_probe_skip alias={} reason=lock_failed_with_pending",
                        event.alias
                    );
                }
            }
        }
    });
}

fn start_consumer_watchdog_thread(shared: Arc<Mutex<SharedState>>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(CONSUMER_WATCHDOG_POLL);
            if let Ok(mut guard) = shared.lock() {
                if !guard.capture_enabled {
                    continue;
                }
                let stale = guard.consumer_last_heartbeat.elapsed() >= CONSUMER_HEARTBEAT_TIMEOUT;
                if stale && !guard.fail_open_mode {
                    guard.fail_open_mode = true;
                    clear_capture_runtime_state(&mut guard);
                    if guard.debug_enabled {
                        global_key_log!(
                            "watchdog_trip fail_open=true stale_ms={} timeout_ms={}",
                            guard.consumer_last_heartbeat.elapsed().as_millis(),
                            CONSUMER_HEARTBEAT_TIMEOUT.as_millis()
                        );
                    }
                }
            }
        }
    });
}

impl GlobalInputCapture {
    pub fn note_local_keydown_alias(&self, sdl_alias: &str) {
        if let Ok(mut guard) = self.shared.lock() {
            guard
                .local_keydown_alias_lookup
                .insert(sdl_alias.to_string(), Instant::now());
            if guard.debug_enabled {
                global_key_log!("local_keydown_alias {}", sdl_alias);
            }
        }
    }

    pub fn is_tap_active(&self) -> bool {
        if let Ok(guard) = self.shared.lock() {
            guard.tap_active
        } else {
            false
        }
    }

    pub fn set_capture_enabled(&self, enabled: bool) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.capture_enabled = enabled;
            guard.consumer_last_heartbeat = Instant::now();
            if !enabled {
                guard.fail_open_mode = false;
                clear_capture_runtime_state(&mut guard);
            }
        }
    }

    pub fn request_attach(&self) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.control_tx.send(macos::ControlMessage::AttachRequested);
        }
    }

    pub fn notify_focus_lost(&self) {
        if let Ok(mut guard) = self.shared.lock() {
            if guard.debug_enabled {
                global_key_log!("injection_start source=focus_loss kc=null ts=null");
            }
            for (kc, alias) in guard.keys_down.iter() {
                if guard.debug_enabled {
                    global_key_log!("synthetic_key_up alias={} kc={} ts=null", alias, kc);
                }
            }
            guard.keys_down.clear();
            guard.mods = ModifierState::default();
            guard.last_modifier_flags = None;
            guard.hid_consumer_states.clear();
            guard.hid_consumer_active = false;
            guard.probe_pending_keycode = None;
            guard.probe_started_at = None;
            guard.deferred_unresolved_keydowns.clear();
            guard.local_keydown_alias_lookup.clear();
            guard.injection_ts_by_usage.clear();
        }
    }

    pub fn notify_focus_gained(&self) {
        if let Ok(guard) = self.shared.lock() {
            if guard.debug_enabled {
                global_key_log!("injection_end source=focus_gain kc=null ts=null");
            }
        }
        #[cfg(target_os = "macos")]
        {
            macos::sync_modifiers_from_system(&self.shared);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{
        FnTrackingMode, InputEvent, InputEventKind, IoTimestamp, ModifierState, SharedState,
        apply_probe_alias_lock, capture_is_effectively_enabled, expire_probe_if_timed_out,
        freshest_local_alias, queue_event, should_expose_key_state,
    };
    use std::collections::BTreeSet;
    use std::ffi::c_void;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    type Boolean = u8;
    type CFIndex = isize;
    type CFAllocatorRef = *const c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CFTimeInterval = f64;
    type CFMachPortRef = *mut c_void;
    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CGEventMask = u64;
    type CGEventType = u32;
    type CGEventFlags = u64;
    type CGEventSourceStateID = i32;
    type IOHIDManagerRef = *mut c_void;
    type IOHIDValueRef = *mut c_void;
    type IOHIDElementRef = *mut c_void;
    type IOOptionBits = u32;
    type IOReturn = i32;

    type CGEventTapCallBack = Option<
        unsafe extern "C" fn(
            proxy: CGEventTapProxy,
            event_type: CGEventType,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef,
    >;
    type IOHIDValueCallback = Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            result: IOReturn,
            sender: *mut c_void,
            value: IOHIDValueRef,
        ),
    >;

    const K_CG_HID_EVENT_TAP: u32 = 0;
    const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
    const K_CG_EVENT_KEY_DOWN: CGEventType = 10;
    const K_CG_EVENT_KEY_UP: CGEventType = 11;
    const K_CG_EVENT_FLAGS_CHANGED: CGEventType = 12;
    const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: CGEventType = 0xFFFF_FFFE;
    const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: CGEventType = 0xFFFF_FFFF;
    const K_CG_KEYBOARD_EVENT_KEYCODE: i32 = 9;
    const K_CG_EVENT_FLAG_MASK_SHIFT: CGEventFlags = 0x0002_0000;
    const K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT: CGEventFlags = 0x0001_0000;
    const K_CG_EVENT_FLAG_MASK_CONTROL: CGEventFlags = 0x0004_0000;
    const K_CG_EVENT_FLAG_MASK_ALTERNATE: CGEventFlags = 0x0008_0000;
    const K_CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;
    const K_CG_EVENT_FLAG_MASK_SECONDARY_FN: CGEventFlags = 0x0080_0000;
    const K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE: CGEventSourceStateID = 1;
    const K_HID_PAGE_CONSUMER: u32 = 0x0C;

    pub enum ControlMessage {
        AttachRequested,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: CFStringRef;
        static kCFRunLoopDefaultMode: CFStringRef;

        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: Boolean);
        fn CGEventTapIsEnabled(tap: CFMachPortRef) -> Boolean;
        fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
        fn CGEventSourceFlagsState(state_id: CGEventSourceStateID) -> CGEventFlags;

        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: CFIndex,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopRunInMode(
            mode: CFStringRef,
            seconds: CFTimeInterval,
            return_after_source_handled: Boolean,
        ) -> i32;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDManagerCreate(allocator: CFAllocatorRef, options: IOOptionBits) -> IOHIDManagerRef;
        fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
        fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: *const c_void);
        fn IOHIDManagerRegisterInputValueCallback(
            manager: IOHIDManagerRef,
            callback: IOHIDValueCallback,
            context: *mut c_void,
        );
        fn IOHIDManagerScheduleWithRunLoop(
            manager: IOHIDManagerRef,
            run_loop: CFRunLoopRef,
            run_loop_mode: CFStringRef,
        );

        fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
        fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> CFIndex;
        fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
        fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
    }

    pub fn start_event_tap(shared: Arc<Mutex<SharedState>>, debug: bool) -> Sender<ControlMessage> {
        let (tx, rx) = channel::<ControlMessage>();
        std::thread::spawn(move || {
            run_tap_thread(shared, debug, rx);
        });
        tx
    }

    pub fn sync_modifiers_from_system(shared: &Arc<Mutex<SharedState>>) {
        let flags = unsafe { CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM_STATE) };
        if let Ok(mut guard) = shared.lock() {
            let prev_mods = guard.mods;
            // Aggregate sync: side fidelity is unknown from this API, so normalize to left side.
            let shift_on = flags & K_CG_EVENT_FLAG_MASK_SHIFT != 0;
            let ctrl_on = flags & K_CG_EVENT_FLAG_MASK_CONTROL != 0;
            let alt_on = flags & K_CG_EVENT_FLAG_MASK_ALTERNATE != 0;
            let cmd_on = flags & K_CG_EVENT_FLAG_MASK_COMMAND != 0;
            let caps_on = flags & K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT != 0;
            let fn_on = flags & K_CG_EVENT_FLAG_MASK_SECONDARY_FN != 0;
            guard.mods.caps_lock = caps_on;
            guard.mods.lshift = shift_on;
            guard.mods.rshift = false;
            guard.mods.lctrl = ctrl_on;
            guard.mods.rctrl = false;
            guard.mods.lalt = alt_on;
            guard.mods.ralt = false;
            guard.mods.lgui = cmd_on;
            guard.mods.rgui = false;
            guard.mods.fn_key = fn_on;
            if guard.debug_enabled {
                global_key_log!("focus_sync_mods flags=0x{:X} (aggregate)", flags);
                log_all_mod_edges(prev_mods, guard.mods);
            }
        }
    }

    fn start_hid_consumer_monitor(shared: Arc<Mutex<SharedState>>, debug: bool) {
        std::thread::spawn(move || unsafe {
            let user_info = Box::into_raw(Box::new(shared.clone())) as *mut c_void;
            let manager = IOHIDManagerCreate(std::ptr::null(), 0);
            if manager.is_null() {
                if debug {
                    global_key_log!("IOHIDManagerCreate failed; hid consumer monitor unavailable");
                }
                let _ = Box::from_raw(user_info as *mut Arc<Mutex<SharedState>>);
                return;
            }
            IOHIDManagerSetDeviceMatching(manager, std::ptr::null());
            IOHIDManagerRegisterInputValueCallback(manager, Some(hid_input_value_callback), user_info);
            let run_loop = CFRunLoopGetCurrent();
            IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopCommonModes);
            let open_status = IOHIDManagerOpen(manager, 0);
            if open_status != 0 {
                if debug {
                    global_key_log!(
                        "IOHIDManagerOpen failed (status={}); hid consumer monitor unavailable",
                        open_status
                    );
                }
                CFRelease(manager as *const c_void);
                let _ = Box::from_raw(user_info as *mut Arc<Mutex<SharedState>>);
                return;
            }
            if let Ok(mut guard) = shared.lock() {
                guard.hid_active = true;
            }
            if debug {
                global_key_log!("hid consumer monitor active");
            }
            loop {
                let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 1);
            }
        });
    }

    fn run_tap_thread(shared: Arc<Mutex<SharedState>>, debug: bool, rx: Receiver<ControlMessage>) {
        unsafe {
            let user_info = Box::into_raw(Box::new(shared.clone())) as *mut c_void;
            loop {
                match rx.recv() {
                    Ok(ControlMessage::AttachRequested) => {}
                    Err(_) => break,
                }
                if let Ok(guard) = shared.lock() && guard.tap_active {
                    continue;
                }
                if debug {
                    global_key_log!("attempting tap attach");
                }

                let mask = (1u64 << K_CG_EVENT_KEY_DOWN)
                    | (1u64 << K_CG_EVENT_KEY_UP)
                    | (1u64 << K_CG_EVENT_FLAGS_CHANGED);
                let tap = CGEventTapCreate(
                    K_CG_HID_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_DEFAULT,
                    mask,
                    Some(event_tap_callback),
                    user_info,
                );
                if tap.is_null() {
                    if debug {
                        global_key_log!("CGEventTapCreate failed; fallback active");
                    }
                    set_active(&shared, false);
                    continue;
                }

                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    if debug {
                        global_key_log!("CFMachPortCreateRunLoopSource failed; fallback active");
                    }
                    set_active(&shared, false);
                    CFRelease(tap as *const c_void);
                    continue;
                }

                let mut start_hid_monitor = false;
                if let Ok(mut guard) = shared.lock() {
                    guard.tap_active = true;
                    guard.recreate_requested = false;
                    guard.fn_mode = FnTrackingMode::Probing;
                    guard.fn_last_observed = None;
                    if !guard.hid_monitor_started {
                        guard.hid_monitor_started = true;
                        start_hid_monitor = true;
                    }
                }
                if start_hid_monitor {
                    start_hid_consumer_monitor(shared.clone(), debug);
                }
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, 1);
                if debug {
                    global_key_log!("event tap active");
                }

                loop {
                    let _ = CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 1);
                    let recreate_requested = if let Ok(guard) = shared.lock() {
                        guard.recreate_requested
                    } else {
                        true
                    };
                    if recreate_requested || CGEventTapIsEnabled(tap) == 0 {
                        if debug {
                            global_key_log!(
                                "tap disabled/error detected; switching to fallback and recreating"
                            );
                        }
                        set_active(&shared, false);
                        CGEventTapEnable(tap, 0);
                        if let Ok(mut guard) = shared.lock() {
                            guard.recreate_requested = true;
                        }
                        break;
                    }
                }

                CFRelease(source as *const c_void);
                CFRelease(tap as *const c_void);
                if debug {
                    global_key_log!("event tap released");
                }
            }
            let _ = Box::from_raw(user_info as *mut Arc<Mutex<SharedState>>);
        }
    }

    fn set_active(shared: &Arc<Mutex<SharedState>>, active: bool) {
        if let Ok(mut guard) = shared.lock() {
            guard.tap_active = active;
            if !active {
                guard.keys_down.clear();
                guard.mods = ModifierState::default();
                guard.last_modifier_flags = None;
                guard.hid_consumer_states.clear();
                guard.hid_consumer_active = false;
                guard.injection_ts_by_usage.clear();
            }
        }
    }

    unsafe extern "C" fn hid_input_value_callback(
        context: *mut c_void,
        _result: IOReturn,
        _sender: *mut c_void,
        value: IOHIDValueRef,
    ) {
        if context.is_null() || value.is_null() {
            return;
        }
        let shared = unsafe { &*(context as *const Arc<Mutex<SharedState>>) };
        let element = unsafe { IOHIDValueGetElement(value) };
        if element.is_null() {
            return;
        }
        let usage_page = unsafe { IOHIDElementGetUsagePage(element) };
        if usage_page != K_HID_PAGE_CONSUMER {
            return;
        }
        let usage = unsafe { IOHIDElementGetUsage(element) };
        if usage == u32::MAX || usage > 0xFFFF {
            return;
        }
        let int_value = unsafe { IOHIDValueGetIntegerValue(value) };
        let down = int_value != 0;
        if let Ok(mut guard) = shared.lock() {
            if !capture_is_effectively_enabled(&guard) {
                return;
            }
            let prev = guard.hid_consumer_states.get(&usage).copied();
            if prev == Some(down) {
                return;
            }
            guard.hid_consumer_states.insert(usage, down);
            guard.hid_consumer_active = guard.hid_consumer_states.values().any(|v| *v);
            if down {
                begin_null_ts_injection(&mut guard, usage);
            } else {
                guard.injection_ts_by_usage.remove(&usage);
            }
            if guard.debug_enabled {
                let edge = if down { "down" } else { "up" };
                global_key_log!("hid_consumer_{} usage=0x{:X} value={}", edge, usage, int_value);
            }
        }
    }

    fn begin_null_ts_injection(guard: &mut SharedState, usage: u32) {
        if guard.debug_enabled {
            global_key_log!("injection_start source=hid usage=0x{:X} ts=null", usage);
        }
        for (kc, alias) in guard.keys_down.iter() {
            if guard.debug_enabled {
                global_key_log!("synthetic_key_up alias={} kc={} ts=null", alias, kc);
            }
        }
        guard.keys_down.clear();
        guard.injection_ts_by_usage.clear();
        guard.injection_ts_by_usage.insert(usage, None);
    }

    unsafe extern "C" fn event_tap_callback(
        _proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        let shared = unsafe { &*(user_info as *const Arc<Mutex<SharedState>>) };
        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            if let Ok(mut guard) = shared.lock() {
                guard.recreate_requested = true;
            }
            return event;
        }
        let keycode = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16 };

        let consume_keyboard_event = if let Ok(mut guard) = shared.lock() {
            expire_probe_if_timed_out(&mut guard);
            if !capture_is_effectively_enabled(&guard) {
                return event;
            }
            if should_passthrough_debug_combo(event_type, keycode, guard.mods) {
                return event;
            }
            if let Some(pending_kc) = guard.probe_pending_keycode
                && keycode != pending_kc
                && matches!(
                    event_type,
                    K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP | K_CG_EVENT_FLAGS_CHANGED
                )
            {
                let is_non_mod = !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes);
                let is_functional = is_function_keycode(keycode);
                if guard.debug_enabled {
                    global_key_log!(
                        "probe_blocking event_type={} kc={} pending_kc={}",
                        event_type,
                        keycode,
                        pending_kc
                    );
                }
                // Preserve OS passthrough policy while probe gate is active.
                return if is_non_mod && !is_functional {
                    event
                } else {
                    std::ptr::null_mut()
                };
            }
            let mut consume = true;
            match event_type {
                K_CG_EVENT_KEY_DOWN => {
                    if guard.debug_enabled {
                        global_key_log!("key_down kc={}", keycode);
                    }
                    let is_non_mod = !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes);
                    let is_functional = is_function_keycode(keycode);
                    if is_non_mod && !is_functional {
                        consume = false;
                        if guard.probe_alias_by_keycode.get(&keycode).is_none()
                            && guard.probe_pending_keycode.is_none()
                        {
                            guard.probe_pending_keycode = Some(keycode);
                            guard.probe_started_at = Some(Instant::now());
                            if guard.debug_enabled {
                                global_key_log!("probe_start kc={}", keycode);
                            }
                            if let Some(sdl_alias) = freshest_local_alias(&mut guard) {
                                let locked = apply_probe_alias_lock(&mut guard, &sdl_alias);
                                if let Some((lkc, alias)) = locked
                                    && guard.debug_enabled
                                {
                                    global_key_log!(
                                        "probe_locked_from_lookup kc={} sdl_alias={}",
                                        lkc,
                                        alias
                                    );
                                }
                            }
                        }
                    }
                    if guard.hid_consumer_active {
                        if guard.debug_enabled {
                            global_key_log!("key_down_ignored_hid_active kc={}", keycode);
                        }
                    } else if !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes) {
                        if guard.keys_down.contains_key(&keycode) {
                            if guard.debug_enabled {
                                global_key_log!("key_down_ignored_duplicate kc={}", keycode);
                            }
                        } else {
                            let alias = keycode_alias_for(&guard, keycode);
                            guard.keys_down.insert(keycode, alias.clone());
                            if should_expose_key_state(&guard, keycode, &alias) {
                                queue_event(
                                    &mut guard,
                                    InputEvent {
                                        timestamp: Some(IoTimestamp::current_time()),
                                        kind: InputEventKind::KeyDown,
                                        alias,
                                        keycode: Some(keycode),
                                        state_keys: Vec::new(),
                                    },
                                );
                            } else {
                                guard.deferred_unresolved_keydowns.insert(keycode);
                                if guard.debug_enabled {
                                    global_key_log!("key_down_deferred_until_probe kc={}", keycode);
                                }
                            }
                        }
                    }
                }
                K_CG_EVENT_KEY_UP => {
                    if guard.debug_enabled {
                        global_key_log!("key_up kc={}", keycode);
                    }
                    let is_non_mod = !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes);
                    let is_functional = is_function_keycode(keycode);
                    if is_non_mod && !is_functional {
                        consume = false;
                    }
                    if guard.hid_consumer_active || !guard.injection_ts_by_usage.is_empty() {
                        if guard.debug_enabled {
                            global_key_log!("key_up_ignored_injection_active kc={}", keycode);
                        }
                    } else if !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes) {
                        if guard.keys_down.remove(&keycode).is_some() {
                            let was_deferred = guard.deferred_unresolved_keydowns.remove(&keycode);
                            if was_deferred {
                                if guard.debug_enabled {
                                    global_key_log!("key_up_deferred_until_probe kc={}", keycode);
                                }
                            } else {
                                let alias = keycode_alias_for(&guard, keycode);
                                queue_event(
                                    &mut guard,
                                    InputEvent {
                                        timestamp: Some(IoTimestamp::current_time()),
                                        kind: InputEventKind::KeyUp,
                                        alias,
                                        keycode: Some(keycode),
                                        state_keys: Vec::new(),
                                    },
                                );
                            }
                        } else if guard.debug_enabled {
                            global_key_log!("key_up_ignored_duplicate kc={}", keycode);
                        }
                    }
                }
                K_CG_EVENT_FLAGS_CHANGED => {
                    let flags = unsafe { CGEventGetFlags(event) };
                    if guard.debug_enabled {
                        global_key_log!("flags_changed kc={} flags=0x{:X}", keycode, flags);
                    }
                    let prev_flags = guard.last_modifier_flags.unwrap_or(flags);
                    let changed_mask = (prev_flags ^ flags) & tracked_modifier_flag_mask();
                    if guard.naive_mod_detect
                        && !known_modifier_keycode(keycode)
                        && !guard.rejected_modifier_keycodes.contains(&keycode)
                        && !guard.inferred_modifier_keycodes.contains(&keycode)
                    {
                        if changed_mask != 0 {
                            let inserted = guard.inferred_modifier_keycodes.insert(keycode);
                        if inserted && guard.debug_enabled {
                            global_key_log!(
                                "inferred_modifier kc={} changed_mask=0x{:X}",
                                keycode,
                                changed_mask
                            );
                        }
                    } else {
                        guard.rejected_modifier_keycodes.insert(keycode);
                        if guard.debug_enabled {
                            global_key_log!(
                                "inferred_non_modifier kc={} (first seen without modifier-flag change)",
                                keycode
                            );
                        }
                    }
                    } else if guard.naive_mod_detect
                        && !known_modifier_keycode(keycode)
                        && changed_mask != 0
                    {
                        // Already classified as inferred/rejected; keep current classification.
                    }
                    guard.last_modifier_flags = Some(flags);
                    let prev_mods = guard.mods;
                    let fn_on = flags & K_CG_EVENT_FLAG_MASK_SECONDARY_FN != 0;
                    let prev_mode = guard.fn_mode;
                    if guard.fn_mode == FnTrackingMode::Probing {
                        let changed = guard.fn_last_observed.map(|prev| prev != fn_on).unwrap_or(false);
                        if keycode == 63 {
                            guard.fn_mode = FnTrackingMode::Reliable;
                        } else if changed {
                            guard.fn_mode = FnTrackingMode::Unreliable;
                        }
                    }
                    if guard.debug_enabled && guard.fn_mode != prev_mode {
                        global_key_log!(
                            "fn tracking mode {:?} -> {:?} (keycode={} fn_on={})",
                            prev_mode, guard.fn_mode, keycode, fn_on
                        );
                    }
                    guard.mods.fn_key = if guard.fn_mode == FnTrackingMode::Reliable {
                        fn_on
                    } else {
                        false
                    };
                    guard.fn_last_observed = Some(fn_on);
                    apply_modifier_change(keycode, flags, &mut guard.mods);
                    let alias = keycode_alias_for(&guard, keycode);
                    queue_event(
                        &mut guard,
                        InputEvent {
                            timestamp: Some(IoTimestamp::current_time()),
                            kind: InputEventKind::ModChanged,
                            alias,
                            keycode: Some(keycode),
                            state_keys: Vec::new(),
                        },
                    );
                    if guard.debug_enabled {
                        log_all_mod_edges(prev_mods, guard.mods);
                    }
                }
                _ => {}
            }
            consume
        } else {
            return event;
        };
        if consume_keyboard_event
            && matches!(
                event_type,
                K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP | K_CG_EVENT_FLAGS_CHANGED
            )
        {
            std::ptr::null_mut()
        } else {
            event
        }
    }

    fn known_modifier_keycode(keycode: u16) -> bool {
        matches!(keycode, 54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63)
    }

    fn is_function_keycode(keycode: u16) -> bool {
        matches!(
            keycode,
            36 | // RETURN
            48 | // TAB
            51 | // BACKSPACE
            53 | // ESCAPE
            82 | 83 | 84 | 85 | 86 | 87 | 88 | 89 | 91 | 92 | // keypad
            96 | 97 | 98 | 99 | 100 | 103 | 105 | 106 | 107 | 109 | 111 | 113 | 118 | 120 | 122 | // F-keys
            114 | 115 | 116 | 117 | 119 | 121 | // nav/edit cluster
            123 | 124 | 125 | 126 // arrows
        )
    }

    fn is_modifier_keycode(keycode: u16, inferred_modifier_keycodes: &BTreeSet<u16>) -> bool {
        known_modifier_keycode(keycode) || inferred_modifier_keycodes.contains(&keycode)
    }

    fn tracked_modifier_flag_mask() -> CGEventFlags {
        K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT
            | K_CG_EVENT_FLAG_MASK_SHIFT
            | K_CG_EVENT_FLAG_MASK_CONTROL
            | K_CG_EVENT_FLAG_MASK_ALTERNATE
            | K_CG_EVENT_FLAG_MASK_COMMAND
            | K_CG_EVENT_FLAG_MASK_SECONDARY_FN
    }

    fn log_mod_edge_raw(kc: u16, prev: bool, now: bool) {
        if prev != now {
            let edge = if now { "mod_down" } else { "mod_up" };
            global_key_log!("{edge} kc={kc}");
        }
    }

    fn log_all_mod_edges(prev: ModifierState, now: ModifierState) {
        log_mod_edge_raw(57, prev.caps_lock, now.caps_lock);
        log_mod_edge_raw(56, prev.lshift, now.lshift);
        log_mod_edge_raw(60, prev.rshift, now.rshift);
        log_mod_edge_raw(59, prev.lctrl, now.lctrl);
        log_mod_edge_raw(62, prev.rctrl, now.rctrl);
        log_mod_edge_raw(58, prev.lalt, now.lalt);
        log_mod_edge_raw(61, prev.ralt, now.ralt);
        log_mod_edge_raw(55, prev.lgui, now.lgui);
        log_mod_edge_raw(54, prev.rgui, now.rgui);
        log_mod_edge_raw(63, prev.fn_key, now.fn_key);
    }

    fn should_passthrough_debug_combo(
        event_type: CGEventType,
        keycode: u16,
        mods: ModifierState,
    ) -> bool {
        if !cfg!(debug_assertions) {
            return false;
        }
        let is_key_event = matches!(event_type, K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP);
        let is_escape = keycode == 53;
        let alt_down = mods.lalt || mods.ralt;
        let meta_down = mods.lgui || mods.rgui;
        is_key_event && is_escape && alt_down && meta_down
    }

    fn apply_modifier_change(keycode: u16, flags: CGEventFlags, mods: &mut ModifierState) {
        let caps_on = flags & K_CG_EVENT_FLAG_MASK_ALPHA_SHIFT != 0;
        let shift_on = flags & K_CG_EVENT_FLAG_MASK_SHIFT != 0;
        let ctrl_on = flags & K_CG_EVENT_FLAG_MASK_CONTROL != 0;
        let alt_on = flags & K_CG_EVENT_FLAG_MASK_ALTERNATE != 0;
        let cmd_on = flags & K_CG_EVENT_FLAG_MASK_COMMAND != 0;
        // Caps lock is latched and represented directly by the aggregate flag.
        mods.caps_lock = caps_on;
        match keycode {
            // flagsChanged arrives for each physical modifier key transition.
            // Toggle per-side state on that keycode, then reconcile with aggregate
            // flags to avoid stuck state when events are missed.
            56 => mods.lshift = !mods.lshift,
            60 => mods.rshift = !mods.rshift,
            59 => mods.lctrl = !mods.lctrl,
            62 => mods.rctrl = !mods.rctrl,
            58 => mods.lalt = !mods.lalt,
            61 => mods.ralt = !mods.ralt,
            55 => mods.lgui = !mods.lgui,
            54 => mods.rgui = !mods.rgui,
            _ => {}
        }
        if !shift_on {
            mods.lshift = false;
            mods.rshift = false;
        }
        if !ctrl_on {
            mods.lctrl = false;
            mods.rctrl = false;
        }
        if !alt_on {
            mods.lalt = false;
            mods.ralt = false;
        }
        if !cmd_on {
            mods.lgui = false;
            mods.rgui = false;
        }
    }

    fn keycode_alias_for(guard: &SharedState, keycode: u16) -> String {
        if let Some(alias) = guard.probe_alias_by_keycode.get(&keycode) {
            return alias.clone();
        }
        if (known_modifier_keycode(keycode) || is_function_keycode(keycode))
            && let Some(name) = keycode_name(keycode)
        {
            name.to_string()
        } else {
            format!("KC{}", keycode)
        }
    }

    fn keycode_name(keycode: u16) -> Option<&'static str> {
        match keycode {
            0 => Some("A"),
            1 => Some("S"),
            2 => Some("D"),
            3 => Some("F"),
            4 => Some("H"),
            5 => Some("G"),
            6 => Some("Z"),
            7 => Some("X"),
            8 => Some("C"),
            9 => Some("V"),
            11 => Some("B"),
            12 => Some("Q"),
            13 => Some("W"),
            14 => Some("E"),
            15 => Some("R"),
            16 => Some("Y"),
            17 => Some("T"),
            18 => Some("1"),
            19 => Some("2"),
            20 => Some("3"),
            21 => Some("4"),
            22 => Some("6"),
            23 => Some("5"),
            24 => Some("="),
            25 => Some("9"),
            26 => Some("7"),
            27 => Some("-"),
            28 => Some("8"),
            29 => Some("0"),
            30 => Some("]"),
            31 => Some("O"),
            32 => Some("U"),
            33 => Some("["),
            34 => Some("I"),
            35 => Some("P"),
            36 => Some("RETURN"),
            37 => Some("L"),
            38 => Some("J"),
            39 => Some("'"),
            40 => Some("K"),
            41 => Some(";"),
            42 => Some("\\"),
            43 => Some(","),
            44 => Some("/"),
            45 => Some("N"),
            46 => Some("M"),
            47 => Some("."),
            48 => Some("TAB"),
            49 => Some("SPACE"),
            50 => Some("`"),
            51 => Some("BACKSPACE"),
            53 => Some("ESCAPE"),
            54 => Some("RGUI"),
            55 => Some("LGUI"),
            56 => Some("LSHIFT"),
            57 => Some("CAPSLOCK"),
            58 => Some("LALT"),
            59 => Some("LCTRL"),
            60 => Some("RSHIFT"),
            61 => Some("RALT"),
            62 => Some("RCTRL"),
            63 => Some("FN"),
            82 => Some("KP0"),
            83 => Some("KP1"),
            84 => Some("KP2"),
            85 => Some("KP3"),
            86 => Some("KP4"),
            87 => Some("KP5"),
            88 => Some("KP6"),
            89 => Some("KP7"),
            91 => Some("KP8"),
            92 => Some("KP9"),
            96 => Some("F5"),
            97 => Some("F6"),
            98 => Some("F7"),
            99 => Some("F3"),
            100 => Some("F8"),
            101 => Some("F9"),
            103 => Some("F11"),
            105 => Some("F13"),
            106 => Some("F16"),
            107 => Some("F14"),
            109 => Some("F10"),
            111 => Some("F12"),
            113 => Some("F15"),
            114 => Some("HELP"),
            115 => Some("HOME"),
            116 => Some("PAGEUP"),
            117 => Some("DELETE"),
            118 => Some("F4"),
            119 => Some("END"),
            120 => Some("F2"),
            121 => Some("PAGEDOWN"),
            122 => Some("F1"),
            123 => Some("LEFT"),
            124 => Some("RIGHT"),
            125 => Some("DOWN"),
            126 => Some("UP"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::io_timestamp::IoTimestamp;
    use std::sync::{Arc, Mutex};

    #[cfg(test)]
    impl GlobalInputCapture {
        fn from_shared_for_tests(shared: Arc<Mutex<SharedState>>) -> Self {
            let (local_key_tx, _local_key_rx) = std::sync::mpsc::channel();
            #[cfg(target_os = "macos")]
            {
                let (tx, _rx) = std::sync::mpsc::channel();
                Self {
                    shared,
                    local_key_tx,
                    control_tx: tx,
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Self {
                    shared,
                    local_key_tx,
                }
            }
        }
    }

    fn sample_event(alias: &str, kc: u16) -> InputEvent {
        InputEvent {
            timestamp: Some(IoTimestamp::current_time()),
            kind: InputEventKind::KeyDown,
            alias: alias.to_string(),
            keycode: Some(kc),
            state_keys: Vec::new(),
        }
    }

    #[test]
    fn queue_event_hides_unresolved_kc_state() {
        let mut guard = SharedState::default();
        guard.keys_down.insert(31, "KC31".to_string());
        guard.keys_down.insert(59, "LCTRL".to_string());

        queue_event(&mut guard, sample_event("X", 7));
        let ev = guard.pending_events.pop_front().expect("event queued");
        assert!(
            ev.state_keys
                .iter()
                .any(|k| k.alias == "LCTRL" && k.keycode == Some(59))
        );
        assert!(
            !ev.state_keys.iter().any(|k| k.keycode == Some(31)),
            "unresolved KC should be hidden from returned state"
        );
    }

    #[test]
    fn expire_probe_timeout_drops_unresolved_state() {
        let mut guard = SharedState::default();
        guard.probe_pending_keycode = Some(110);
        guard.probe_started_at = Some(Instant::now() - PROBE_TIMEOUT - Duration::from_millis(1));
        guard.keys_down.insert(110, "KC110".to_string());
        guard.deferred_unresolved_keydowns.insert(110);

        expire_probe_if_timed_out(&mut guard);

        assert!(guard.probe_pending_keycode.is_none());
        assert!(guard.probe_started_at.is_none());
        assert!(!guard.keys_down.contains_key(&110));
        assert!(!guard.deferred_unresolved_keydowns.contains(&110));
    }

    #[test]
    fn apply_probe_alias_lock_backfills_and_emits_deferred_keydown() {
        let mut guard = SharedState::default();
        guard.probe_pending_keycode = Some(110);
        guard.probe_started_at = Some(Instant::now());
        guard.keys_down.insert(110, "KC110".to_string());
        guard.deferred_unresolved_keydowns.insert(110);
        queue_event(
            &mut guard,
            InputEvent {
                timestamp: Some(IoTimestamp::current_time()),
                kind: InputEventKind::ModChanged,
                alias: "KC110".to_string(),
                keycode: Some(110),
                state_keys: vec![InputKeyState {
                    alias: "KC110".to_string(),
                    keycode: Some(110),
                }],
            },
        );

        let locked = apply_probe_alias_lock(&mut guard, "SDL_HOME");
        assert_eq!(locked, Some((110, "SDL_HOME".to_string())));
        assert_eq!(
            guard.keys_down.get(&110).cloned(),
            Some("SDL_HOME".to_string())
        );
        assert_eq!(
            guard.probe_alias_by_keycode.get(&110).cloned(),
            Some("SDL_HOME".to_string())
        );
        assert!(guard.probe_pending_keycode.is_none());
        assert!(guard.probe_started_at.is_none());
        assert!(!guard.deferred_unresolved_keydowns.contains(&110));

        let events: Vec<InputEvent> = guard.pending_events.iter().cloned().collect();
        assert!(events.iter().any(|e| {
            e.keycode == Some(110)
                && e.alias == "SDL_HOME"
                && matches!(e.kind, InputEventKind::ModChanged)
        }));
        assert!(events.iter().any(|e| {
            e.keycode == Some(110)
                && e.alias == "SDL_HOME"
                && matches!(e.kind, InputEventKind::KeyDown)
        }));
    }

    #[test]
    fn snapshot_and_event_pull_are_blocked_while_probe_pending() {
        let shared = Arc::new(Mutex::new(SharedState::default()));
        {
            let mut guard = shared.lock().expect("lock shared");
            guard.capture_enabled = true;
            guard.tap_active = true;
            guard.probe_pending_keycode = Some(31);
            guard.probe_started_at = Some(Instant::now());
            guard.keys_down.insert(31, "KC31".to_string());
            queue_event(&mut guard, sample_event("KC31", 31));
        }
        let capture = GlobalInputCapture::from_shared_for_tests(shared);
        let snap = capture.snapshot();
        assert!(snap.active);
        assert!(snap.keys_down.is_empty(), "keys should be hidden during probe gate");
        assert!(
            capture.next_event_before(IoTimestamp::current_time()).is_none(),
            "events should be blocked during probe gate"
        );
    }

    #[test]
    fn text_key_event_is_withheld_until_probe_lock_then_returned() {
        let shared = Arc::new(Mutex::new(SharedState::default()));
        {
            let mut guard = shared.lock().expect("lock shared");
            guard.capture_enabled = true;
            guard.tap_active = true;
            guard.probe_pending_keycode = Some(31);
            guard.probe_started_at = Some(Instant::now());
            guard.keys_down.insert(31, "KC31".to_string());
            guard.deferred_unresolved_keydowns.insert(31);
        }
        let capture = GlobalInputCapture::from_shared_for_tests(shared);

        assert!(
            capture.next_event_before(IoTimestamp::current_time()).is_none(),
            "pre-lock text key events must not return"
        );
        assert_eq!(
            capture.try_lock_probe_alias("O"),
            Some((31, "O".to_string())),
            "probe lock should bind mac kc to SDL alias"
        );
        let event = capture
            .next_event_before(IoTimestamp::current_time())
            .expect("resolved keydown should now return");
        assert!(matches!(event.kind, InputEventKind::KeyDown));
        assert_eq!(event.alias, "O");
        assert_eq!(event.keycode, Some(31));
    }

}
