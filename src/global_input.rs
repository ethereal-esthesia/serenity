use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, Debug, Default)]
pub struct ModifierState {
    pub lshift: bool,
    pub rshift: bool,
    pub lctrl: bool,
    pub rctrl: bool,
    pub lalt: bool,
    pub ralt: bool,
    pub lgui: bool,
    pub rgui: bool,
}

#[derive(Clone, Debug, Default)]
pub struct InputSnapshot {
    pub active: bool,
    pub keys_down: Vec<String>,
    pub mods: ModifierState,
}

#[derive(Debug, Default)]
struct SharedState {
    tap_active: bool,
    capture_enabled: bool,
    recreate_requested: bool,
    keys_down: BTreeSet<String>,
    mods: ModifierState,
}

pub struct GlobalInputCapture {
    shared: Arc<Mutex<SharedState>>,
    #[cfg(target_os = "macos")]
    control_tx: Sender<macos::ControlMessage>,
}

impl GlobalInputCapture {
    pub fn start() -> Self {
        Self::start_with_debug(false)
    }

    pub fn start_with_debug(debug: bool) -> Self {
        let shared = Arc::new(Mutex::new(SharedState::default()));
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
        if let Ok(guard) = self.shared.lock() {
            return InputSnapshot {
                active: guard.tap_active && guard.capture_enabled,
                keys_down: guard.keys_down.iter().cloned().collect(),
                mods: guard.mods,
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
            }
        }
    }

    pub fn request_attach(&self) {
        #[cfg(target_os = "macos")]
        {
            let _ = self.control_tx.send(macos::ControlMessage::AttachRequested);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{ModifierState, SharedState};
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

    type CGEventTapCallBack = Option<
        unsafe extern "C" fn(
            proxy: CGEventTapProxy,
            event_type: CGEventType,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef,
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
    const K_CG_EVENT_FLAG_MASK_CONTROL: CGEventFlags = 0x0004_0000;
    const K_CG_EVENT_FLAG_MASK_ALTERNATE: CGEventFlags = 0x0008_0000;
    const K_CG_EVENT_FLAG_MASK_COMMAND: CGEventFlags = 0x0010_0000;

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

    pub fn start_event_tap(shared: Arc<Mutex<SharedState>>, debug: bool) -> Sender<ControlMessage> {
        let (tx, rx) = channel::<ControlMessage>();
        std::thread::spawn(move || {
            run_tap_thread(shared, debug, rx);
        });
        tx
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

                if let Ok(mut guard) = shared.lock() {
                    guard.tap_active = true;
                    guard.recreate_requested = false;
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
            }
        }
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
                    if !is_modifier_keycode(keycode) {
                        guard.keys_down.insert(keycode_label(keycode));
                    }
                }
                K_CG_EVENT_KEY_UP => {
                    if !is_modifier_keycode(keycode) {
                        guard.keys_down.remove(&keycode_label(keycode));
                    }
                }
                K_CG_EVENT_FLAGS_CHANGED => {
                    let flags = unsafe { CGEventGetFlags(event) };
                    apply_modifier_change(keycode, flags, &mut guard.mods);
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

    fn is_modifier_keycode(keycode: u16) -> bool {
        matches!(keycode, 54 | 55 | 56 | 58 | 59 | 60 | 61 | 62)
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
        let shift_on = flags & K_CG_EVENT_FLAG_MASK_SHIFT != 0;
        let ctrl_on = flags & K_CG_EVENT_FLAG_MASK_CONTROL != 0;
        let alt_on = flags & K_CG_EVENT_FLAG_MASK_ALTERNATE != 0;
        let cmd_on = flags & K_CG_EVENT_FLAG_MASK_COMMAND != 0;
        match keycode {
            56 => mods.lshift = shift_on,
            60 => mods.rshift = shift_on,
            59 => mods.lctrl = ctrl_on,
            62 => mods.rctrl = ctrl_on,
            58 => mods.lalt = alt_on,
            61 => mods.ralt = alt_on,
            55 => mods.lgui = cmd_on,
            54 => mods.rgui = cmd_on,
            _ => {}
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
