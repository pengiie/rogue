use std::collections::HashSet;
use winit::{event::ButtonId as WinitButtonId, keyboard::KeyCode as WinitKeyCode};

pub struct Keyboard {
    pressed_keys: HashSet<Key>,
    down_keys: HashSet<Key>,
    repeated_keys: HashSet<Key>,
    released_keys: HashSet<Key>,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            pressed_keys: HashSet::new(),
            down_keys: HashSet::new(),
            repeated_keys: HashSet::new(),
            released_keys: HashSet::new(),
        }
    }

    pub fn submit_input(&mut self, input: SubmitInput) {
        match input {
            SubmitInput::Pressed(key) => {
                if !self.down_keys.contains(&key) {
                    self.pressed_keys.insert(key);
                    self.down_keys.insert(key);
                } else {
                    self.repeated_keys.insert(key);
                }
            }
            SubmitInput::Released(key) => {
                self.released_keys.insert(key);
                self.down_keys.remove(&key);
            }
            SubmitInput::Repeated(key) => {
                self.repeated_keys.insert(key);
            }
        }
    }

    pub fn clear_inputs(&mut self) {
        self.pressed_keys.clear();
        self.repeated_keys.clear();
        self.released_keys.clear();
    }

    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.pressed_keys.contains(&key)
    }

    pub fn is_key_pressed_with_modifiers(&self, key: Key, modifiers: &[Modifier]) -> bool {
        self.is_key_pressed(key) && self.is_modifiers_down(modifiers)
    }

    pub fn is_key_down_with_modifiers(&self, key: Key, modifiers: &[Modifier]) -> bool {
        self.is_key_down(key) && self.is_modifiers_down(modifiers)
    }

    pub fn is_key_released_with_modifiers(&self, key: Key, modifiers: &[Modifier]) -> bool {
        self.is_key_released(key) && self.is_modifiers_down(modifiers)
    }

    pub fn is_key_down(&self, key: Key) -> bool {
        self.down_keys.contains(&key)
    }

    pub fn is_key_repeat(&self, key: Key) -> bool {
        self.repeated_keys.contains(&key)
    }

    pub fn is_key_released(&self, key: Key) -> bool {
        self.released_keys.contains(&key)
    }

    pub fn is_modifiers_down(&self, modifiers: &[Modifier]) -> bool {
        for modifier in modifiers {
            if !modifier
                .get_keys()
                .iter()
                .any(|k| self.is_key_down(k.clone()))
            {
                return false;
            }
        }
        return true;
    }
}

#[derive(Debug)]
pub enum SubmitInput {
    Pressed(Key),
    Repeated(Key),
    Released(Key),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,

    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,

    Escape,

    LControl,
    LShift,
    LAlt,
    LSystem,

    RControl,
    RShift,
    RAlt,
    RSystem,

    LBracket,
    RBracket,

    Semicolon,
    Comma,
    Period,
    Quote,
    Slash,
    Backslash,
    Tilde,
    Equal,
    Hyphen,

    Space,
    Enter,
    Backspace,
    Tab,

    PageUp,
    PageDown,
    End,
    Home,
    Insert,
    Delete,

    Left,
    Right,
    Up,
    Down,

    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

impl Key {
    pub fn from_winit_key_code(code: WinitKeyCode) -> Option<Key> {
        match code {
            WinitKeyCode::KeyA => Some(Key::A),
            WinitKeyCode::KeyB => Some(Key::B),
            WinitKeyCode::KeyC => Some(Key::C),
            WinitKeyCode::KeyD => Some(Key::D),
            WinitKeyCode::KeyE => Some(Key::E),
            WinitKeyCode::KeyF => Some(Key::F),
            WinitKeyCode::KeyG => Some(Key::G),
            WinitKeyCode::KeyH => Some(Key::H),
            WinitKeyCode::KeyI => Some(Key::I),
            WinitKeyCode::KeyJ => Some(Key::J),
            WinitKeyCode::KeyK => Some(Key::K),
            WinitKeyCode::KeyL => Some(Key::L),
            WinitKeyCode::KeyM => Some(Key::M),
            WinitKeyCode::KeyN => Some(Key::N),
            WinitKeyCode::KeyO => Some(Key::O),
            WinitKeyCode::KeyP => Some(Key::P),
            WinitKeyCode::KeyQ => Some(Key::Q),
            WinitKeyCode::KeyR => Some(Key::R),
            WinitKeyCode::KeyS => Some(Key::S),
            WinitKeyCode::KeyT => Some(Key::T),
            WinitKeyCode::KeyU => Some(Key::U),
            WinitKeyCode::KeyV => Some(Key::V),
            WinitKeyCode::KeyW => Some(Key::W),
            WinitKeyCode::KeyX => Some(Key::X),
            WinitKeyCode::KeyY => Some(Key::Y),
            WinitKeyCode::KeyZ => Some(Key::Z),
            WinitKeyCode::Escape => Some(Key::Escape),
            WinitKeyCode::F1 => Some(Key::F1),
            WinitKeyCode::F2 => Some(Key::F2),
            WinitKeyCode::F3 => Some(Key::F3),
            WinitKeyCode::F4 => Some(Key::F4),
            WinitKeyCode::F5 => Some(Key::F5),
            WinitKeyCode::F6 => Some(Key::F6),
            WinitKeyCode::F7 => Some(Key::F7),
            WinitKeyCode::F8 => Some(Key::F8),
            WinitKeyCode::F9 => Some(Key::F9),
            WinitKeyCode::F10 => Some(Key::F10),
            WinitKeyCode::F11 => Some(Key::F11),
            WinitKeyCode::F12 => Some(Key::F12),
            WinitKeyCode::Period => Some(Key::Period),
            WinitKeyCode::Comma => Some(Key::Comma),
            WinitKeyCode::Slash => Some(Key::Slash),
            WinitKeyCode::Backslash => Some(Key::Backslash),
            WinitKeyCode::Quote => Some(Key::Quote),
            WinitKeyCode::Semicolon => Some(Key::Semicolon),
            WinitKeyCode::Minus => Some(Key::Hyphen),
            WinitKeyCode::Equal => Some(Key::Equal),
            WinitKeyCode::BracketLeft => Some(Key::LBracket),
            WinitKeyCode::BracketRight => Some(Key::RBracket),
            WinitKeyCode::Backspace => Some(Key::Backspace),
            WinitKeyCode::Tab => Some(Key::Tab),
            WinitKeyCode::Enter => Some(Key::Enter),
            WinitKeyCode::Space => Some(Key::Space),
            WinitKeyCode::Insert => Some(Key::Insert),
            WinitKeyCode::Delete => Some(Key::Delete),
            WinitKeyCode::Home => Some(Key::Home),
            WinitKeyCode::End => Some(Key::End),
            WinitKeyCode::PageUp => Some(Key::PageUp),
            WinitKeyCode::PageDown => Some(Key::PageDown),
            WinitKeyCode::ArrowLeft => Some(Key::Left),
            WinitKeyCode::ArrowRight => Some(Key::Right),
            WinitKeyCode::ArrowUp => Some(Key::Up),
            WinitKeyCode::ArrowDown => Some(Key::Down),
            WinitKeyCode::ShiftLeft => Some(Key::LShift),
            WinitKeyCode::ShiftRight => Some(Key::RShift),
            WinitKeyCode::ControlLeft => Some(Key::LControl),
            WinitKeyCode::ControlRight => Some(Key::RControl),
            WinitKeyCode::AltLeft => Some(Key::LAlt),
            WinitKeyCode::AltRight => Some(Key::RAlt),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
}

impl Modifier {
    pub(crate) fn get_keys(&self) -> Vec<Key> {
        match self {
            Modifier::Shift => vec![Key::LShift, Key::RShift],
            Modifier::Control => vec![Key::LControl, Key::RControl],
            Modifier::Alt => vec![Key::LAlt, Key::RAlt],
        }
    }
}
