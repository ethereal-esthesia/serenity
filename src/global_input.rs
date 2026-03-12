use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::sync::mpsc::Sender;

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

#[derive(Clone, Debug, Default)]
pub struct InputSnapshot {
    pub active: bool,
    pub keys_down: Vec<String>,
    pub mods: ModifierState,
    pub fn_mode: FnTrackingMode,
}

#[derive(Debug)]
struct SharedState {
    tap_active: bool,
    hid_active: bool,
    hid_monitor_started: bool,
    capture_enabled: bool,
    recreate_requested: bool,
    debug_enabled: bool,
    naive_mod_detect: bool,
    keys_down: BTreeSet<String>,
    inferred_modifier_keycodes: BTreeSet<u16>,
    rejected_modifier_keycodes: BTreeSet<u16>,
    last_modifier_flags: Option<u64>,
    hid_consumer_states: BTreeMap<u32, bool>,
    hid_consumer_active: bool,
    injection_ts_by_usage: BTreeMap<u32, Option<u64>>,
    mods: ModifierState,
    fn_mode: FnTrackingMode,
    fn_last_observed: Option<bool>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            tap_active: false,
            hid_active: false,
            hid_monitor_started: false,
            capture_enabled: false,
            recreate_requested: false,
            debug_enabled: false,
            naive_mod_detect: false,
            keys_down: BTreeSet::new(),
            inferred_modifier_keycodes: BTreeSet::new(),
            rejected_modifier_keycodes: BTreeSet::new(),
            last_modifier_flags: None,
            hid_consumer_states: BTreeMap::new(),
            hid_consumer_active: false,
            injection_ts_by_usage: BTreeMap::new(),
            mods: ModifierState::default(),
            fn_mode: FnTrackingMode::Unavailable,
            fn_last_observed: None,
        }
    }
}

pub struct GlobalInputCapture {
    shared: Arc<Mutex<SharedState>>,
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
        if let Ok(mut guard) = shared.lock() {
            guard.debug_enabled = debug;
            guard.naive_mod_detect = true;
        }
        #[cfg(target_os = "macos")]
        let control_tx = macos::start_event_tap(shared.clone(), debug);
        #[cfg(not(target_os = "macos"))]
        {}
        Self {
            shared,
            #[cfg(target_os = "macos")]
            control_tx,
        }
    }

    pub fn snapshot(&self) -> InputSnapshot {
        if let Ok(mut guard) = self.shared.lock() {
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
                            println!(
                                "[global_input] injection_ts_assigned usage=0x{:X} ts={}",
                                usage, now
                            );
                        }
                    }
                }
            }
            return InputSnapshot {
                active: (guard.tap_active || guard.hid_active) && guard.capture_enabled,
                keys_down: guard.keys_down.iter().cloned().collect(),
                mods: guard.mods,
                fn_mode: guard.fn_mode,
            };
        }
        InputSnapshot::default()
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
            if !enabled {
                guard.keys_down.clear();
                guard.mods = ModifierState::default();
                guard.last_modifier_flags = None;
                guard.hid_consumer_states.clear();
                guard.hid_consumer_active = false;
                guard.injection_ts_by_usage.clear();
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
                println!("[global_input] injection_start source=focus_loss kc=null ts=null");
            }
            for label in guard.keys_down.iter() {
                if guard.debug_enabled {
                    println!("[global_input] synthetic_key_up label={} ts=null", label);
                }
            }
            guard.keys_down.clear();
            guard.mods = ModifierState::default();
            guard.last_modifier_flags = None;
            guard.hid_consumer_states.clear();
            guard.hid_consumer_active = false;
            guard.injection_ts_by_usage.clear();
        }
    }

    pub fn notify_focus_gained(&self) {
        if let Ok(guard) = self.shared.lock() {
            if guard.debug_enabled {
                println!("[global_input] injection_end source=focus_gain kc=null ts=null");
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
    use super::{FnTrackingMode, ModifierState, SharedState};
    use std::collections::BTreeSet;
    use std::ffi::c_void;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};

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
                println!(
                    "[global_input] focus_sync_mods flags=0x{:X} (aggregate)",
                    flags
                );
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
                    println!("[global_input] IOHIDManagerCreate failed; hid consumer monitor unavailable");
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
                    println!(
                        "[global_input] IOHIDManagerOpen failed (status={}); hid consumer monitor unavailable",
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
                println!("[global_input] hid consumer monitor active");
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
                    println!("[global_input] attempting tap attach");
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
                        println!("[global_input] CGEventTapCreate failed; fallback active");
                    }
                    set_active(&shared, false);
                    continue;
                }

                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    if debug {
                        println!("[global_input] CFMachPortCreateRunLoopSource failed; fallback active");
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
                    println!("[global_input] event tap active");
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
                            println!("[global_input] tap disabled/error detected; switching to fallback and recreating");
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
                    println!("[global_input] event tap released");
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
            if !guard.capture_enabled {
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
                println!(
                    "[global_input] hid_consumer_{} usage=0x{:X} value={}",
                    edge, usage, int_value
                );
            }
        }
    }

    fn begin_null_ts_injection(guard: &mut SharedState, usage: u32) {
        if guard.debug_enabled {
            println!(
                "[global_input] injection_start source=hid usage=0x{:X} ts=null",
                usage
            );
        }
        for label in guard.keys_down.iter() {
            if guard.debug_enabled {
                println!("[global_input] synthetic_key_up label={} ts=null", label);
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
            if !guard.capture_enabled {
                return event;
            }
            if should_passthrough_debug_combo(event_type, keycode, guard.mods) {
                return event;
            }
            match event_type {
                K_CG_EVENT_KEY_DOWN => {
                    if guard.debug_enabled {
                        println!("[global_input] key_down kc={}", keycode);
                    }
                    if guard.hid_consumer_active {
                        if guard.debug_enabled {
                            println!("[global_input] key_down_ignored_hid_active kc={}", keycode);
                        }
                    } else if !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes) {
                        let label = keycode_label(keycode);
                        if guard.keys_down.contains(&label) {
                            if guard.debug_enabled {
                                println!("[global_input] key_down_ignored_duplicate kc={}", keycode);
                            }
                        } else {
                            guard.keys_down.insert(label);
                        }
                    }
                }
                K_CG_EVENT_KEY_UP => {
                    if guard.debug_enabled {
                        println!("[global_input] key_up kc={}", keycode);
                    }
                    if guard.hid_consumer_active || !guard.injection_ts_by_usage.is_empty() {
                        if guard.debug_enabled {
                            println!("[global_input] key_up_ignored_injection_active kc={}", keycode);
                        }
                    } else if !is_modifier_keycode(keycode, &guard.inferred_modifier_keycodes) {
                        let label = keycode_label(keycode);
                        if guard.keys_down.contains(&label) {
                            guard.keys_down.remove(&label);
                        } else if guard.debug_enabled {
                            println!("[global_input] key_up_ignored_duplicate kc={}", keycode);
                        }
                    }
                }
                K_CG_EVENT_FLAGS_CHANGED => {
                    let flags = unsafe { CGEventGetFlags(event) };
                    if guard.debug_enabled {
                        println!(
                            "[global_input] flags_changed kc={} flags=0x{:X}",
                            keycode, flags
                        );
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
                            println!(
                                "[global_input] inferred_modifier kc={} changed_mask=0x{:X}",
                                keycode,
                                changed_mask
                            );
                        }
                    } else {
                        guard.rejected_modifier_keycodes.insert(keycode);
                        if guard.debug_enabled {
                            println!(
                                "[global_input] inferred_non_modifier kc={} (first seen without modifier-flag change)",
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
                        println!(
                            "[global_input] fn tracking mode {:?} -> {:?} (keycode={} fn_on={})",
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
                    if guard.debug_enabled {
                        log_all_mod_edges(prev_mods, guard.mods);
                    }
                }
                _ => {}
            }
            true
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
            println!("[global_input] {edge} kc={kc}");
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

    fn keycode_label(keycode: u16) -> String {
        if let Some(name) = keycode_name(keycode) {
            format!("{name}|KC{keycode}")
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
