use platforms::input::{
    KeyKind as PlatformKeyKind, KeyState as PlatformKeyState, MouseKind as PlatformMouseKind,
};
use strum::Display;

use crate::grpc::input::{Key as RpcKeyKind, KeyState as RpcKeyState, MouseAction as RpcMouseKind};
use crate::models::{KeyBinding, LinkKeyBinding};

macro_rules! convert_key {
    ($from:ident, $to:ident) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self {
                match value {
                    $from::A => $to::A,
                    $from::B => $to::B,
                    $from::C => $to::C,
                    $from::D => $to::D,
                    $from::E => $to::E,
                    $from::F => $to::F,
                    $from::G => $to::G,
                    $from::H => $to::H,
                    $from::I => $to::I,
                    $from::J => $to::J,
                    $from::K => $to::K,
                    $from::L => $to::L,
                    $from::M => $to::M,
                    $from::N => $to::N,
                    $from::O => $to::O,
                    $from::P => $to::P,
                    $from::Q => $to::Q,
                    $from::R => $to::R,
                    $from::S => $to::S,
                    $from::T => $to::T,
                    $from::U => $to::U,
                    $from::V => $to::V,
                    $from::W => $to::W,
                    $from::X => $to::X,
                    $from::Y => $to::Y,
                    $from::Z => $to::Z,
                    $from::Zero => $to::Zero,
                    $from::One => $to::One,
                    $from::Two => $to::Two,
                    $from::Three => $to::Three,
                    $from::Four => $to::Four,
                    $from::Five => $to::Five,
                    $from::Six => $to::Six,
                    $from::Seven => $to::Seven,
                    $from::Eight => $to::Eight,
                    $from::Nine => $to::Nine,
                    $from::F1 => $to::F1,
                    $from::F2 => $to::F2,
                    $from::F3 => $to::F3,
                    $from::F4 => $to::F4,
                    $from::F5 => $to::F5,
                    $from::F6 => $to::F6,
                    $from::F7 => $to::F7,
                    $from::F8 => $to::F8,
                    $from::F9 => $to::F9,
                    $from::F10 => $to::F10,
                    $from::F11 => $to::F11,
                    $from::F12 => $to::F12,
                    $from::Up => $to::Up,
                    $from::Down => $to::Down,
                    $from::Left => $to::Left,
                    $from::Right => $to::Right,
                    $from::Home => $to::Home,
                    $from::End => $to::End,
                    $from::PageUp => $to::PageUp,
                    $from::PageDown => $to::PageDown,
                    $from::Insert => $to::Insert,
                    $from::Delete => $to::Delete,
                    $from::Enter => $to::Enter,
                    $from::Space => $to::Space,
                    $from::Tilde => $to::Tilde,
                    $from::Quote => $to::Quote,
                    $from::Semicolon => $to::Semicolon,
                    $from::Comma => $to::Comma,
                    $from::Period => $to::Period,
                    $from::Slash => $to::Slash,
                    $from::Esc => $to::Esc,
                    $from::Shift => $to::Shift,
                    $from::Ctrl => $to::Ctrl,
                    $from::Alt => $to::Alt,
                    $from::Backspace => $to::Backspace,
                }
            }
        }
    };
}

/// The current of key state.
///
/// This is a bridge enum between platform-specific and gRPC.
#[derive(Debug)]
pub enum KeyState {
    Pressed,
    Released,
}

impl From<PlatformKeyState> for KeyState {
    fn from(value: PlatformKeyState) -> Self {
        match value {
            PlatformKeyState::Pressed => KeyState::Pressed,
            PlatformKeyState::Released => KeyState::Released,
        }
    }
}

impl From<RpcKeyState> for KeyState {
    fn from(value: RpcKeyState) -> Self {
        match value {
            RpcKeyState::Pressed => KeyState::Pressed,
            RpcKeyState::Released => KeyState::Released,
        }
    }
}

/// The kind of mouse movement/action to perform.
///
/// This is a bridge enum between platform-specific and gRPC.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MouseKind {
    Move,
    Click,
    Scroll,
}

impl From<MouseKind> for RpcMouseKind {
    fn from(value: MouseKind) -> Self {
        match value {
            MouseKind::Move => RpcMouseKind::Move,
            MouseKind::Click => RpcMouseKind::Click,
            MouseKind::Scroll => RpcMouseKind::ScrollDown,
        }
    }
}

impl From<MouseKind> for PlatformMouseKind {
    fn from(value: MouseKind) -> Self {
        match value {
            MouseKind::Move => PlatformMouseKind::Move,
            MouseKind::Click => PlatformMouseKind::Click,
            MouseKind::Scroll => PlatformMouseKind::Scroll,
        }
    }
}

/// The kind of key to sent.
///
/// This is a bridge enum between platform-specific, gRPC and database.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Display, Default)]
pub enum KeyKind {
    #[default]
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

    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,

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

    Up,
    Down,
    Left,
    Right,

    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Ctrl,
    Enter,
    Space,
    Tilde,
    Quote,
    Semicolon,
    Comma,
    Period,
    Slash,
    Esc,
    Shift,
    Alt,
    Backspace,
}

convert_key!(KeyBinding, KeyKind);
convert_key!(PlatformKeyKind, KeyKind);

convert_key!(KeyKind, PlatformKeyKind);
convert_key!(KeyKind, RpcKeyKind);
convert_key!(KeyKind, KeyBinding);

#[derive(Clone, Copy, Debug, Default)]
pub enum LinkKeyKind {
    #[default]
    None,
    Before(KeyKind),
    AtTheSame(KeyKind),
    After(KeyKind),
    Along(KeyKind),
}

impl From<LinkKeyBinding> for LinkKeyKind {
    fn from(value: LinkKeyBinding) -> Self {
        match value {
            LinkKeyBinding::None => LinkKeyKind::None,
            LinkKeyBinding::Before(key) => LinkKeyKind::Before(key.into()),
            LinkKeyBinding::AtTheSame(key) => LinkKeyKind::AtTheSame(key.into()),
            LinkKeyBinding::After(key) => LinkKeyKind::After(key.into()),
            LinkKeyBinding::Along(key) => LinkKeyKind::Along(key.into()),
        }
    }
}
