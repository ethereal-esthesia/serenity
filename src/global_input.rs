use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

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
    active: bool,
    keys_down: BTreeSet<String>,
    mods: ModifierState,
}

pub struct GlobalInputCapture {
    shared: Arc<Mutex<SharedState>>,
}

impl GlobalInputCapture {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(SharedState::default()));
        #[cfg(target_os = "macos")]
        macos::start_event_tap(shared.clone());
        #[cfg(not(target_os = "macos"))]
        {}
        Self { shared }
    }

    pub fn snapshot(&self) -> InputSnapshot {
        if let Ok(guard) = self.shared.lock() {
            return InputSnapshot {
                active: guard.active,
                keys_down: guard.keys_down.iter().cloned().collect(),
                mods: guard.mods,
            };
        }
        InputSnapshot::default()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{ModifierState, SharedState};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

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

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: CFStringRef;

        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: Boolean);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: i32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
        fn AXIsProcessTrusted() -> Boolean;

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

    pub fn start_event_tap(shared: Arc<Mutex<SharedState>>) {
        std::thread::spawn(move || unsafe {
            let user_info = Box::into_raw(Box::new(shared.clone())) as *mut c_void;
            loop {
                if AXIsProcessTrusted() == 0 {
                    set_active(&shared, false);
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
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
                    set_active(&shared, false);
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }

                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    set_active(&shared, false);
                    CFRelease(tap as *const c_void);
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }

                if let Ok(mut guard) = shared.lock() {
                    guard.active = true;
                }
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, 1);

                loop {
                    if AXIsProcessTrusted() == 0 {
                        set_active(&shared, false);
                        CGEventTapEnable(tap, 0);
                        break;
                    }
                    let _ = CFRunLoopRunInMode(kCFRunLoopCommonModes, 0.25, 1);
                }

                CFRelease(source as *const c_void);
                CFRelease(tap as *const c_void);
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }

    fn set_active(shared: &Arc<Mutex<SharedState>>, active: bool) {
        if let Ok(mut guard) = shared.lock() {
            guard.active = active;
        }
    }

    unsafe extern "C" fn event_tap_callback(
        proxy: CGEventTapProxy,
        event_type: CGEventType,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            unsafe {
                CGEventTapEnable(proxy, 1);
            }
            return event;
        }
        let shared = unsafe { &*(user_info as *const Arc<Mutex<SharedState>>) };
        let keycode = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16 };

        if let Ok(mut guard) = shared.lock() {
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
        }
        match event_type {
            K_CG_EVENT_KEY_DOWN | K_CG_EVENT_KEY_UP | K_CG_EVENT_FLAGS_CHANGED => std::ptr::null_mut(),
            _ => event,
        }
    }

    fn is_modifier_keycode(keycode: u16) -> bool {
        matches!(keycode, 54 | 55 | 56 | 58 | 59 | 60 | 61 | 62)
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
        match keycode {
            0 => "A".to_string(),
            1 => "S".to_string(),
            2 => "D".to_string(),
            3 => "F".to_string(),
            4 => "H".to_string(),
            5 => "G".to_string(),
            6 => "Z".to_string(),
            7 => "X".to_string(),
            8 => "C".to_string(),
            9 => "V".to_string(),
            11 => "B".to_string(),
            12 => "Q".to_string(),
            13 => "W".to_string(),
            14 => "E".to_string(),
            15 => "R".to_string(),
            16 => "Y".to_string(),
            17 => "T".to_string(),
            18 => "1".to_string(),
            19 => "2".to_string(),
            20 => "3".to_string(),
            21 => "4".to_string(),
            22 => "6".to_string(),
            23 => "5".to_string(),
            24 => "=".to_string(),
            25 => "9".to_string(),
            26 => "7".to_string(),
            27 => "-".to_string(),
            28 => "8".to_string(),
            29 => "0".to_string(),
            30 => "]".to_string(),
            31 => "O".to_string(),
            32 => "U".to_string(),
            33 => "[".to_string(),
            34 => "I".to_string(),
            35 => "P".to_string(),
            36 => "RETURN".to_string(),
            37 => "L".to_string(),
            38 => "J".to_string(),
            39 => "'".to_string(),
            40 => "K".to_string(),
            41 => ";".to_string(),
            42 => "\\".to_string(),
            43 => ",".to_string(),
            44 => "/".to_string(),
            45 => "N".to_string(),
            46 => "M".to_string(),
            47 => ".".to_string(),
            48 => "TAB".to_string(),
            49 => "SPACE".to_string(),
            50 => "`".to_string(),
            51 => "BACKSPACE".to_string(),
            53 => "ESCAPE".to_string(),
            82 => "KP0".to_string(),
            83 => "KP1".to_string(),
            84 => "KP2".to_string(),
            85 => "KP3".to_string(),
            86 => "KP4".to_string(),
            87 => "KP5".to_string(),
            88 => "KP6".to_string(),
            89 => "KP7".to_string(),
            91 => "KP8".to_string(),
            92 => "KP9".to_string(),
            123 => "LEFT".to_string(),
            124 => "RIGHT".to_string(),
            125 => "DOWN".to_string(),
            126 => "UP".to_string(),
            _ => format!("KC{}", keycode),
        }
    }
}
