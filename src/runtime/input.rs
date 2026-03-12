use sdl3::event::Event;
use sdl3::keyboard::Keycode;

#[derive(Debug, Default, Clone)]
pub struct WindowInputState {
    pub window_shown: bool,
    pub window_focused: bool,
    pub mouse_inside_window: bool,
    pub cursor_hidden: bool,
    pub keys_down: Vec<String>,
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
    mut on_keydown: impl FnMut(Keycode) -> bool,
) -> bool {
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
                repeat: false,
                ..
            } => {
                if keycode == Keycode::Escape {
                    return true;
                }
                if !is_modifier_key(keycode) {
                    let label = keycode_label(keycode);
                    if !state.keys_down.contains(&label) {
                        state.keys_down.push(label);
                    }
                }
                if on_keydown(keycode) {
                    return true;
                }
            }
            Event::KeyUp {
                keycode: Some(keycode),
                repeat: false,
                ..
            } => {
                if !is_modifier_key(keycode) {
                    let label = keycode_label(keycode);
                    state.keys_down.retain(|k| k != &label);
                }
            }
            _ => {}
        }
    }
    false
}

pub fn process_events(events: &mut sdl3::EventPump, state: &mut WindowInputState) -> bool {
    process_events_with_keydown(events, state, |_| false)
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
