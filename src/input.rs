use alacritty_terminal::term::TermMode;
use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// Translate a winit key event into the byte sequence expected by a terminal
/// application (classic xterm encoding).
///
/// Kitty keyboard protocol and `modifyOtherKeys` are intentionally not
/// advertised yet; applications fall back to this encoding.
pub fn encode_key(
    event: &KeyEvent,
    modifiers: ModifiersState,
    mode: TermMode,
    option_as_meta: bool,
) -> Option<Vec<u8>> {
    if event.state != ElementState::Pressed {
        return None;
    }

    // Command/Meta is reserved for application shortcuts.
    if modifiers.super_key() {
        return None;
    }

    if let Key::Named(named) = &event.logical_key {
        return encode_named(named, modifiers, mode.contains(TermMode::APP_CURSOR));
    }

    // AltGr is reported as Ctrl+Alt on most layouts; it must compose text
    // instead of being treated as a Meta shortcut.
    if modifiers.control_key() && modifiers.alt_key() {
        return event
            .text
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| text.as_bytes().to_vec());
    }

    if modifiers.control_key() {
        let Key::Character(text) = &event.logical_key else {
            return None;
        };
        let character = text.chars().next()?;
        return ctrl_byte(character).map(|byte| vec![byte]);
    }

    if modifiers.alt_key() && option_as_meta {
        let mut bytes = vec![0x1b];
        match event.key_without_modifiers() {
            Key::Character(text) => bytes.extend_from_slice(text.as_bytes()),
            _ => return None,
        }
        return Some(bytes);
    }

    if let Some(text) = &event.text
        && !text.is_empty()
    {
        return Some(text.as_bytes().to_vec());
    }

    if let Key::Character(text) = &event.logical_key {
        return Some(text.as_bytes().to_vec());
    }

    None
}

fn encode_named(named: &NamedKey, modifiers: ModifiersState, app_cursor: bool) -> Option<Vec<u8>> {
    let alt = modifiers.alt_key();
    let shift = modifiers.shift_key();
    let param = modifier_param(modifiers);

    match named {
        NamedKey::Enter => Some(enter(shift, alt)),
        NamedKey::Tab if shift => Some(vec![0x1b, b'[', b'Z']),
        NamedKey::Tab => Some(with_alt(vec![b'\t'], alt)),
        NamedKey::Backspace => {
            let byte = if modifiers.control_key() { 0x08 } else { 0x7f };
            Some(with_alt(vec![byte], alt))
        }
        NamedKey::Escape => Some(vec![0x1b]),
        NamedKey::Space => {
            if modifiers.control_key() {
                Some(vec![0x00])
            } else {
                Some(with_alt(vec![b' '], alt))
            }
        }

        NamedKey::ArrowUp => Some(arrow(b'A', param, app_cursor)),
        NamedKey::ArrowDown => Some(arrow(b'B', param, app_cursor)),
        NamedKey::ArrowRight => Some(arrow(b'C', param, app_cursor)),
        NamedKey::ArrowLeft => Some(arrow(b'D', param, app_cursor)),

        NamedKey::Home => Some(home_end(b'H', param, app_cursor)),
        NamedKey::End => Some(home_end(b'F', param, app_cursor)),

        NamedKey::Insert => Some(tilde(2, param)),
        NamedKey::Delete => Some(tilde(3, param)),
        NamedKey::PageUp => Some(tilde(5, param)),
        NamedKey::PageDown => Some(tilde(6, param)),

        NamedKey::F1 => Some(function_key(1, param)),
        NamedKey::F2 => Some(function_key(2, param)),
        NamedKey::F3 => Some(function_key(3, param)),
        NamedKey::F4 => Some(function_key(4, param)),
        NamedKey::F5 => Some(function_key(5, param)),
        NamedKey::F6 => Some(function_key(6, param)),
        NamedKey::F7 => Some(function_key(7, param)),
        NamedKey::F8 => Some(function_key(8, param)),
        NamedKey::F9 => Some(function_key(9, param)),
        NamedKey::F10 => Some(function_key(10, param)),
        NamedKey::F11 => Some(function_key(11, param)),
        NamedKey::F12 => Some(function_key(12, param)),

        _ => None,
    }
}

fn enter(shift: bool, alt: bool) -> Vec<u8> {
    // Shift+Enter is commonly used by TUIs (OpenCode, Claude Code, Codex) to
    // insert a newline. Legacy terminals cannot distinguish Shift+Return, so
    // send LF, which those apps accept as a newline.
    with_alt(vec![if shift { b'\n' } else { b'\r' }], alt)
}

fn with_alt(mut bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn modifier_param(modifiers: ModifiersState) -> u8 {
    let mut value = 1;
    if modifiers.shift_key() {
        value += 1;
    }
    if modifiers.alt_key() {
        value += 2;
    }
    if modifiers.control_key() {
        value += 4;
    }
    value
}

fn arrow(final_byte: u8, param: u8, app_cursor: bool) -> Vec<u8> {
    if param == 1 {
        if app_cursor {
            vec![0x1b, b'O', final_byte]
        } else {
            vec![0x1b, b'[', final_byte]
        }
    } else {
        format!("\x1b[1;{param}{}", final_byte as char).into_bytes()
    }
}

fn home_end(final_byte: u8, param: u8, app_cursor: bool) -> Vec<u8> {
    if param == 1 {
        if app_cursor {
            vec![0x1b, b'O', final_byte]
        } else {
            vec![0x1b, b'[', final_byte]
        }
    } else {
        format!("\x1b[1;{param}{}", final_byte as char).into_bytes()
    }
}

fn tilde(number: u8, param: u8) -> Vec<u8> {
    if param == 1 {
        format!("\x1b[{number}~").into_bytes()
    } else {
        format!("\x1b[{number};{param}~").into_bytes()
    }
}

fn function_key(number: u8, param: u8) -> Vec<u8> {
    match number {
        1..=4 => {
            let final_byte = b'P' + (number - 1);
            if param == 1 {
                vec![0x1b, b'O', final_byte]
            } else {
                format!("\x1b[1;{param}{}", final_byte as char).into_bytes()
            }
        }
        _ => {
            let code = match number {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                _ => return Vec::new(),
            };
            tilde(code, param)
        }
    }
}

fn ctrl_byte(character: char) -> Option<u8> {
    match character {
        'a'..='z' => Some(character as u8 - b'a' + 1),
        'A'..='Z' => Some(character as u8 - b'A' + 1),
        '@' | ' ' => Some(0),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}

/// Encode a mouse button event using the SGR protocol (mode 1006).
pub fn encode_mouse_sgr(
    button: MouseButton,
    pressed: bool,
    column: usize,
    row: usize,
    modifiers: ModifiersState,
) -> Vec<u8> {
    let mut code = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        MouseButton::Other(other) => 8 + (other & 0x0f),
    };
    code += mouse_modifier_bits(modifiers);
    let final_byte = if pressed { 'M' } else { 'm' };
    format!("\x1b[<{code};{};{}{final_byte}", column + 1, row + 1).into_bytes()
}

/// Encode a wheel event using the SGR protocol.
pub fn encode_wheel_sgr(up: bool, column: usize, row: usize, modifiers: ModifiersState) -> Vec<u8> {
    let code = 64 + u16::from(!up) + mouse_modifier_bits(modifiers);
    format!("\x1b[<{code};{};{}M", column + 1, row + 1).into_bytes()
}

fn mouse_modifier_bits(modifiers: ModifiersState) -> u16 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits |= 4;
    }
    if modifiers.alt_key() {
        bits |= 8;
    }
    if modifiers.control_key() {
        bits |= 16;
    }
    bits
}

#[cfg(test)]
#[path = "tests/input.rs"]
mod tests;
