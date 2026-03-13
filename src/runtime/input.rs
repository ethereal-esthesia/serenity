use sdl3::event::Event;
use sdl3::keyboard::Mod;
use sdl3::keyboard::Keycode;

use crate::global_input::{
    GlobalInputCapture, InputEvent, InputEventKind, InputKeyState, ModifierState,
};
use crate::runtime::io_timestamp::IoTimestamp;

macro_rules! local_key_log {
    ($($arg:tt)*) => {
        println!(
            "[local_input ts={}] {}",
            IoTimestamp::current_time().raw(),
            format!($($arg)*)
        );
    };
}

#[derive(Debug, Default, Clone)]
pub struct WindowInputState {
    pub window_shown: bool,
    pub window_focused: bool,
    pub mouse_inside_window: bool,
    pub cursor_hidden: bool,
    pub thread_panel_scroll_lines: i32,
    pub keys_down: Vec<String>,
    pub keydown_events: Vec<String>,
    pub keyup_events: Vec<String>,
    pub frame_events: Vec<InputEvent>,
}

impl WindowInputState {
    pub fn with_cursor_hidden(cursor_hidden: bool) -> Self {
        Self {
            cursor_hidden,
            ..Self::default()
        }
    }
}

pub fn keycode_label(keycode: Keycode) -> String {
    format!("{:?}", keycode)
        .to_uppercase()
        .replace('_', " ")
        .replace(':', " ")
        .replace(',', " ")
}

pub fn is_modifier_key(keycode: Keycode) -> bool {
    matches!(
        keycode,
        Keycode::LShift
            | Keycode::RShift
            | Keycode::LCtrl
            | Keycode::RCtrl
            | Keycode::LAlt
            | Keycode::RAlt
            | Keycode::LGui
            | Keycode::RGui
    )
}

pub fn process_events_with_keydown(
    events: &mut sdl3::EventPump,
    state: &mut WindowInputState,
    debug: bool,
    mut on_keydown: impl FnMut(Keycode, bool) -> bool,
) -> bool {
    state.thread_panel_scroll_lines = 0;
    state.keydown_events.clear();
    state.keyup_events.clear();
    state.frame_events.clear();
    for event in events.poll_iter() {
        match event {
            Event::Quit { .. } => return true,
            Event::Window { win_event, .. } => match win_event {
                sdl3::event::WindowEvent::FocusGained => state.window_focused = true,
                sdl3::event::WindowEvent::FocusLost => state.window_focused = false,
                sdl3::event::WindowEvent::MouseEnter => state.mouse_inside_window = true,
                sdl3::event::WindowEvent::MouseLeave => state.mouse_inside_window = false,
                _ => {}
            },
            Event::KeyDown {
                keycode: Some(keycode),
                repeat,
                ..
            } => {
                if debug {
                    local_key_log!("key_down {:?} repeat={}", keycode, repeat);
                }
                if keycode == Keycode::Escape {
                    return true;
                }
                if !is_modifier_key(keycode) && !repeat {
                    let label = keycode_label(keycode);
                    if !state.keys_down.contains(&label) {
                        state.keys_down.push(label.clone());
                    }
                    state.keydown_events.push(label);
                    state.frame_events.push(InputEvent {
                        timestamp: None,
                        kind: InputEventKind::KeyDown,
                        alias: keycode_label(keycode),
                        keycode: keycode_to_u16(keycode),
                        state_keys: state
                            .keys_down
                            .iter()
                            .cloned()
                            .map(|alias| InputKeyState {
                                alias,
                                keycode: None,
                            })
                            .collect(),
                    });
                } else if !is_modifier_key(keycode) {
                    // Feed probe lock path even when OS is generating held-key repeat events.
                    state.keydown_events.push(keycode_label(keycode));
                }
                if on_keydown(keycode, repeat) {
                    return true;
                }
            }
            Event::KeyUp {
                keycode: Some(keycode),
                repeat: false,
                ..
            } => {
                if debug {
                    local_key_log!("key_up {:?}", keycode);
                }
                if !is_modifier_key(keycode) {
                    let label = keycode_label(keycode);
                    state.keys_down.retain(|k| k != &label);
                    state.keyup_events.push(label);
                    state.frame_events.push(InputEvent {
                        timestamp: None,
                        kind: InputEventKind::KeyUp,
                        alias: keycode_label(keycode),
                        keycode: keycode_to_u16(keycode),
                        state_keys: state
                            .keys_down
                            .iter()
                            .cloned()
                            .map(|alias| InputKeyState {
                                alias,
                                keycode: None,
                            })
                            .collect(),
                    });
                }
            }
            Event::MouseWheel { y, .. } => {
                state.thread_panel_scroll_lines += y.round() as i32;
            }
            _ => {}
        }
    }
    false
}

fn keycode_to_u16(keycode: Keycode) -> Option<u16> {
    let raw = keycode as i32;
    if (0..=u16::MAX as i32).contains(&raw) {
        Some(raw as u16)
    } else {
        None
    }
}

fn modifier_state_to_sdl_mod(mods: ModifierState) -> Mod {
    let mut out = Mod::NOMOD;
    if mods.caps_lock {
        out |= Mod::CAPSMOD;
    }
    if mods.lshift {
        out |= Mod::LSHIFTMOD;
    }
    if mods.rshift {
        out |= Mod::RSHIFTMOD;
    }
    if mods.lctrl {
        out |= Mod::LCTRLMOD;
    }
    if mods.rctrl {
        out |= Mod::RCTRLMOD;
    }
    if mods.lalt {
        out |= Mod::LALTMOD;
    }
    if mods.ralt {
        out |= Mod::RALTMOD;
    }
    if mods.lgui {
        out |= Mod::LGUIMOD;
    }
    if mods.rgui {
        out |= Mod::RGUIMOD;
    }
    out
}

#[derive(Debug, Clone)]
pub struct InputFrameView {
    pub hud_keys: Vec<String>,
    pub hud_optional_keys: Vec<String>,
    pub hud_mods: Mod,
    pub hud_fn: bool,
    pub thread_events: Vec<InputEvent>,
    pub should_quit: bool,
}

impl Default for InputFrameView {
    fn default() -> Self {
        Self {
            hud_keys: Vec::new(),
            hud_optional_keys: Vec::new(),
            hud_mods: Mod::NOMOD,
            hud_fn: false,
            thread_events: Vec::new(),
            should_quit: false,
        }
    }
}

pub fn resolve_input_frame_view(
    state: &WindowInputState,
    global_capture: Option<&GlobalInputCapture>,
    disable_global_input: bool,
    fallback_mods: Mod,
) -> InputFrameView {
    let local_view = || InputFrameView {
        hud_keys: state.keys_down.clone(),
        hud_optional_keys: Vec::new(),
        hud_mods: fallback_mods,
        hud_fn: false,
        thread_events: state.frame_events.clone(),
        should_quit: false,
    };
    if disable_global_input {
        return local_view();
    }
    let Some(capture) = global_capture else {
        return local_view();
    };
    capture.set_capture_enabled(should_enable_global_capture(state));
    let mut out = InputFrameView::default();
    let deadline = IoTimestamp::current_time();
    while let Some(event) = capture.next_event_before(deadline) {
        out.thread_events.push(event);
    }
    let snap = capture.snapshot();
    if snap.active {
        out.should_quit = snap.keys_down.iter().any(|k| k.alias == "ESCAPE");
        out.hud_keys = snap
            .keys_down
            .iter()
            .map(|k| {
                if let Some(kc) = k.keycode {
                    format!("{}|KC{}", k.alias, kc)
                } else {
                    k.alias.clone()
                }
            })
            .collect();
        out.hud_optional_keys = Vec::new();
        out.hud_mods = modifier_state_to_sdl_mod(snap.mods);
        out.hud_fn = snap.mods.fn_key;
        return out;
    }
    local_view()
}

pub fn process_events_with_debug(
    events: &mut sdl3::EventPump,
    state: &mut WindowInputState,
    debug: bool,
) -> bool {
    process_events_with_keydown(events, state, debug, |_, _| false)
}

pub fn process_events(events: &mut sdl3::EventPump, state: &mut WindowInputState) -> bool {
    process_events_with_keydown(events, state, false, |_, _| false)
}

pub fn sync_cursor_visibility(sdl: &sdl3::Sdl, state: &mut WindowInputState) {
    let should_hide = state.window_shown && state.window_focused && state.mouse_inside_window;
    if should_hide != state.cursor_hidden {
        sdl.mouse().show_cursor(!should_hide);
        state.cursor_hidden = should_hide;
    }
}

pub fn should_enable_global_capture(state: &WindowInputState) -> bool {
    state.window_shown && state.window_focused
}
