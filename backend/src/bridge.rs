use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, hash_map::Entry},
    fmt::Debug,
};

use anyhow::{Result, anyhow};
use futures::stream::BoxStream;
use log::info;
#[cfg(test)]
use mockall::automock;
#[cfg(windows)]
use platforms::capture::WindowsCaptureKind;
use platforms::{
    CoordinateRelative, Error, Window,
    capture::{Capture as PlatformCapture, Frame},
    input::{
        Input as PlatformInput, InputKind as PlatformInputKind,
        InputReceiver as PlatformInputReceiver, KeyKind as PlatformKeyKind,
        KeyState as PlatformKeyState, MouseKind as PlatformMouseKind,
    },
};
use serenity::futures::StreamExt;
use strum::Display;

use crate::{
    grpc::{
        InputService,
        input::{
            Coordinate as RpcCoordinate, Key as RpcKeyKind, KeyState as RpcKeyState,
            MouseAction as RpcMouseKind,
        },
    },
    models::{
        CaptureMode, InputMethod as DatabaseInputMethod, KeyBinding, LinkKeyBinding, Settings,
    },
    rng::Rng,
    run::MS_PER_TICK_F32,
};

/// Base mean in milliseconds to generate a pair from.
const BASE_MEAN_MS_DELAY: f32 = 100.0;

/// Base standard deviation in milliseconds to generate a pair from.
const BASE_STD_MS_DELAY: f32 = 20.0;

/// The rate at which generated standard deviation will revert to the base [`BASE_STD_MS_DELAY`]
/// over time.
const MEAN_STD_REVERSION_RATE: f32 = 0.2;

/// The rate at which generated mean will revert to the base [`BASE_MEAN_MS_DELAY`] over time.
const MEAN_STD_VOLATILITY: f32 = 3.0;

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

impl From<KeyBinding> for KeyKind {
    fn from(value: KeyBinding) -> Self {
        match value {
            KeyBinding::A => KeyKind::A,
            KeyBinding::B => KeyKind::B,
            KeyBinding::C => KeyKind::C,
            KeyBinding::D => KeyKind::D,
            KeyBinding::E => KeyKind::E,
            KeyBinding::F => KeyKind::F,
            KeyBinding::G => KeyKind::G,
            KeyBinding::H => KeyKind::H,
            KeyBinding::I => KeyKind::I,
            KeyBinding::J => KeyKind::J,
            KeyBinding::K => KeyKind::K,
            KeyBinding::L => KeyKind::L,
            KeyBinding::M => KeyKind::M,
            KeyBinding::N => KeyKind::N,
            KeyBinding::O => KeyKind::O,
            KeyBinding::P => KeyKind::P,
            KeyBinding::Q => KeyKind::Q,
            KeyBinding::R => KeyKind::R,
            KeyBinding::S => KeyKind::S,
            KeyBinding::T => KeyKind::T,
            KeyBinding::U => KeyKind::U,
            KeyBinding::V => KeyKind::V,
            KeyBinding::W => KeyKind::W,
            KeyBinding::X => KeyKind::X,
            KeyBinding::Y => KeyKind::Y,
            KeyBinding::Z => KeyKind::Z,
            KeyBinding::Zero => KeyKind::Zero,
            KeyBinding::One => KeyKind::One,
            KeyBinding::Two => KeyKind::Two,
            KeyBinding::Three => KeyKind::Three,
            KeyBinding::Four => KeyKind::Four,
            KeyBinding::Five => KeyKind::Five,
            KeyBinding::Six => KeyKind::Six,
            KeyBinding::Seven => KeyKind::Seven,
            KeyBinding::Eight => KeyKind::Eight,
            KeyBinding::Nine => KeyKind::Nine,
            KeyBinding::F1 => KeyKind::F1,
            KeyBinding::F2 => KeyKind::F2,
            KeyBinding::F3 => KeyKind::F3,
            KeyBinding::F4 => KeyKind::F4,
            KeyBinding::F5 => KeyKind::F5,
            KeyBinding::F6 => KeyKind::F6,
            KeyBinding::F7 => KeyKind::F7,
            KeyBinding::F8 => KeyKind::F8,
            KeyBinding::F9 => KeyKind::F9,
            KeyBinding::F10 => KeyKind::F10,
            KeyBinding::F11 => KeyKind::F11,
            KeyBinding::F12 => KeyKind::F12,
            KeyBinding::Up => KeyKind::Up,
            KeyBinding::Down => KeyKind::Down,
            KeyBinding::Left => KeyKind::Left,
            KeyBinding::Right => KeyKind::Right,
            KeyBinding::Home => KeyKind::Home,
            KeyBinding::End => KeyKind::End,
            KeyBinding::PageUp => KeyKind::PageUp,
            KeyBinding::PageDown => KeyKind::PageDown,
            KeyBinding::Insert => KeyKind::Insert,
            KeyBinding::Delete => KeyKind::Delete,
            KeyBinding::Enter => KeyKind::Enter,
            KeyBinding::Space => KeyKind::Space,
            KeyBinding::Tilde => KeyKind::Tilde,
            KeyBinding::Quote => KeyKind::Quote,
            KeyBinding::Semicolon => KeyKind::Semicolon,
            KeyBinding::Comma => KeyKind::Comma,
            KeyBinding::Period => KeyKind::Period,
            KeyBinding::Slash => KeyKind::Slash,
            KeyBinding::Esc => KeyKind::Esc,
            KeyBinding::Shift => KeyKind::Shift,
            KeyBinding::Ctrl => KeyKind::Ctrl,
            KeyBinding::Alt => KeyKind::Alt,
            KeyBinding::Backspace => KeyKind::Backspace,
        }
    }
}

impl From<PlatformKeyKind> for KeyKind {
    fn from(value: PlatformKeyKind) -> Self {
        match value {
            PlatformKeyKind::A => KeyKind::A,
            PlatformKeyKind::B => KeyKind::B,
            PlatformKeyKind::C => KeyKind::C,
            PlatformKeyKind::D => KeyKind::D,
            PlatformKeyKind::E => KeyKind::E,
            PlatformKeyKind::F => KeyKind::F,
            PlatformKeyKind::G => KeyKind::G,
            PlatformKeyKind::H => KeyKind::H,
            PlatformKeyKind::I => KeyKind::I,
            PlatformKeyKind::J => KeyKind::J,
            PlatformKeyKind::K => KeyKind::K,
            PlatformKeyKind::L => KeyKind::L,
            PlatformKeyKind::M => KeyKind::M,
            PlatformKeyKind::N => KeyKind::N,
            PlatformKeyKind::O => KeyKind::O,
            PlatformKeyKind::P => KeyKind::P,
            PlatformKeyKind::Q => KeyKind::Q,
            PlatformKeyKind::R => KeyKind::R,
            PlatformKeyKind::S => KeyKind::S,
            PlatformKeyKind::T => KeyKind::T,
            PlatformKeyKind::U => KeyKind::U,
            PlatformKeyKind::V => KeyKind::V,
            PlatformKeyKind::W => KeyKind::W,
            PlatformKeyKind::X => KeyKind::X,
            PlatformKeyKind::Y => KeyKind::Y,
            PlatformKeyKind::Z => KeyKind::Z,
            PlatformKeyKind::Zero => KeyKind::Zero,
            PlatformKeyKind::One => KeyKind::One,
            PlatformKeyKind::Two => KeyKind::Two,
            PlatformKeyKind::Three => KeyKind::Three,
            PlatformKeyKind::Four => KeyKind::Four,
            PlatformKeyKind::Five => KeyKind::Five,
            PlatformKeyKind::Six => KeyKind::Six,
            PlatformKeyKind::Seven => KeyKind::Seven,
            PlatformKeyKind::Eight => KeyKind::Eight,
            PlatformKeyKind::Nine => KeyKind::Nine,
            PlatformKeyKind::F1 => KeyKind::F1,
            PlatformKeyKind::F2 => KeyKind::F2,
            PlatformKeyKind::F3 => KeyKind::F3,
            PlatformKeyKind::F4 => KeyKind::F4,
            PlatformKeyKind::F5 => KeyKind::F5,
            PlatformKeyKind::F6 => KeyKind::F6,
            PlatformKeyKind::F7 => KeyKind::F7,
            PlatformKeyKind::F8 => KeyKind::F8,
            PlatformKeyKind::F9 => KeyKind::F9,
            PlatformKeyKind::F10 => KeyKind::F10,
            PlatformKeyKind::F11 => KeyKind::F11,
            PlatformKeyKind::F12 => KeyKind::F12,
            PlatformKeyKind::Up => KeyKind::Up,
            PlatformKeyKind::Down => KeyKind::Down,
            PlatformKeyKind::Left => KeyKind::Left,
            PlatformKeyKind::Right => KeyKind::Right,
            PlatformKeyKind::Home => KeyKind::Home,
            PlatformKeyKind::End => KeyKind::End,
            PlatformKeyKind::PageUp => KeyKind::PageUp,
            PlatformKeyKind::PageDown => KeyKind::PageDown,
            PlatformKeyKind::Insert => KeyKind::Insert,
            PlatformKeyKind::Delete => KeyKind::Delete,
            PlatformKeyKind::Ctrl => KeyKind::Ctrl,
            PlatformKeyKind::Enter => KeyKind::Enter,
            PlatformKeyKind::Space => KeyKind::Space,
            PlatformKeyKind::Tilde => KeyKind::Tilde,
            PlatformKeyKind::Quote => KeyKind::Quote,
            PlatformKeyKind::Semicolon => KeyKind::Semicolon,
            PlatformKeyKind::Comma => KeyKind::Comma,
            PlatformKeyKind::Period => KeyKind::Period,
            PlatformKeyKind::Slash => KeyKind::Slash,
            PlatformKeyKind::Esc => KeyKind::Esc,
            PlatformKeyKind::Shift => KeyKind::Shift,
            PlatformKeyKind::Alt => KeyKind::Alt,
            PlatformKeyKind::Backspace => KeyKind::Backspace,
        }
    }
}

impl From<KeyKind> for PlatformKeyKind {
    fn from(value: KeyKind) -> Self {
        match value {
            KeyKind::A => PlatformKeyKind::A,
            KeyKind::B => PlatformKeyKind::B,
            KeyKind::C => PlatformKeyKind::C,
            KeyKind::D => PlatformKeyKind::D,
            KeyKind::E => PlatformKeyKind::E,
            KeyKind::F => PlatformKeyKind::F,
            KeyKind::G => PlatformKeyKind::G,
            KeyKind::H => PlatformKeyKind::H,
            KeyKind::I => PlatformKeyKind::I,
            KeyKind::J => PlatformKeyKind::J,
            KeyKind::K => PlatformKeyKind::K,
            KeyKind::L => PlatformKeyKind::L,
            KeyKind::M => PlatformKeyKind::M,
            KeyKind::N => PlatformKeyKind::N,
            KeyKind::O => PlatformKeyKind::O,
            KeyKind::P => PlatformKeyKind::P,
            KeyKind::Q => PlatformKeyKind::Q,
            KeyKind::R => PlatformKeyKind::R,
            KeyKind::S => PlatformKeyKind::S,
            KeyKind::T => PlatformKeyKind::T,
            KeyKind::U => PlatformKeyKind::U,
            KeyKind::V => PlatformKeyKind::V,
            KeyKind::W => PlatformKeyKind::W,
            KeyKind::X => PlatformKeyKind::X,
            KeyKind::Y => PlatformKeyKind::Y,
            KeyKind::Z => PlatformKeyKind::Z,
            KeyKind::Zero => PlatformKeyKind::Zero,
            KeyKind::One => PlatformKeyKind::One,
            KeyKind::Two => PlatformKeyKind::Two,
            KeyKind::Three => PlatformKeyKind::Three,
            KeyKind::Four => PlatformKeyKind::Four,
            KeyKind::Five => PlatformKeyKind::Five,
            KeyKind::Six => PlatformKeyKind::Six,
            KeyKind::Seven => PlatformKeyKind::Seven,
            KeyKind::Eight => PlatformKeyKind::Eight,
            KeyKind::Nine => PlatformKeyKind::Nine,
            KeyKind::F1 => PlatformKeyKind::F1,
            KeyKind::F2 => PlatformKeyKind::F2,
            KeyKind::F3 => PlatformKeyKind::F3,
            KeyKind::F4 => PlatformKeyKind::F4,
            KeyKind::F5 => PlatformKeyKind::F5,
            KeyKind::F6 => PlatformKeyKind::F6,
            KeyKind::F7 => PlatformKeyKind::F7,
            KeyKind::F8 => PlatformKeyKind::F8,
            KeyKind::F9 => PlatformKeyKind::F9,
            KeyKind::F10 => PlatformKeyKind::F10,
            KeyKind::F11 => PlatformKeyKind::F11,
            KeyKind::F12 => PlatformKeyKind::F12,
            KeyKind::Up => PlatformKeyKind::Up,
            KeyKind::Down => PlatformKeyKind::Down,
            KeyKind::Left => PlatformKeyKind::Left,
            KeyKind::Right => PlatformKeyKind::Right,
            KeyKind::Home => PlatformKeyKind::Home,
            KeyKind::End => PlatformKeyKind::End,
            KeyKind::PageUp => PlatformKeyKind::PageUp,
            KeyKind::PageDown => PlatformKeyKind::PageDown,
            KeyKind::Insert => PlatformKeyKind::Insert,
            KeyKind::Delete => PlatformKeyKind::Delete,
            KeyKind::Ctrl => PlatformKeyKind::Ctrl,
            KeyKind::Enter => PlatformKeyKind::Enter,
            KeyKind::Space => PlatformKeyKind::Space,
            KeyKind::Tilde => PlatformKeyKind::Tilde,
            KeyKind::Quote => PlatformKeyKind::Quote,
            KeyKind::Semicolon => PlatformKeyKind::Semicolon,
            KeyKind::Comma => PlatformKeyKind::Comma,
            KeyKind::Period => PlatformKeyKind::Period,
            KeyKind::Slash => PlatformKeyKind::Slash,
            KeyKind::Esc => PlatformKeyKind::Esc,
            KeyKind::Shift => PlatformKeyKind::Shift,
            KeyKind::Alt => PlatformKeyKind::Alt,
            KeyKind::Backspace => PlatformKeyKind::Backspace,
        }
    }
}

impl From<KeyKind> for RpcKeyKind {
    fn from(value: KeyKind) -> Self {
        match value {
            KeyKind::A => RpcKeyKind::A,
            KeyKind::B => RpcKeyKind::B,
            KeyKind::C => RpcKeyKind::C,
            KeyKind::D => RpcKeyKind::D,
            KeyKind::E => RpcKeyKind::E,
            KeyKind::F => RpcKeyKind::F,
            KeyKind::G => RpcKeyKind::G,
            KeyKind::H => RpcKeyKind::H,
            KeyKind::I => RpcKeyKind::I,
            KeyKind::J => RpcKeyKind::J,
            KeyKind::K => RpcKeyKind::K,
            KeyKind::L => RpcKeyKind::L,
            KeyKind::M => RpcKeyKind::M,
            KeyKind::N => RpcKeyKind::N,
            KeyKind::O => RpcKeyKind::O,
            KeyKind::P => RpcKeyKind::P,
            KeyKind::Q => RpcKeyKind::Q,
            KeyKind::R => RpcKeyKind::R,
            KeyKind::S => RpcKeyKind::S,
            KeyKind::T => RpcKeyKind::T,
            KeyKind::U => RpcKeyKind::U,
            KeyKind::V => RpcKeyKind::V,
            KeyKind::W => RpcKeyKind::W,
            KeyKind::X => RpcKeyKind::X,
            KeyKind::Y => RpcKeyKind::Y,
            KeyKind::Z => RpcKeyKind::Z,
            KeyKind::Zero => RpcKeyKind::Zero,
            KeyKind::One => RpcKeyKind::One,
            KeyKind::Two => RpcKeyKind::Two,
            KeyKind::Three => RpcKeyKind::Three,
            KeyKind::Four => RpcKeyKind::Four,
            KeyKind::Five => RpcKeyKind::Five,
            KeyKind::Six => RpcKeyKind::Six,
            KeyKind::Seven => RpcKeyKind::Seven,
            KeyKind::Eight => RpcKeyKind::Eight,
            KeyKind::Nine => RpcKeyKind::Nine,
            KeyKind::F1 => RpcKeyKind::F1,
            KeyKind::F2 => RpcKeyKind::F2,
            KeyKind::F3 => RpcKeyKind::F3,
            KeyKind::F4 => RpcKeyKind::F4,
            KeyKind::F5 => RpcKeyKind::F5,
            KeyKind::F6 => RpcKeyKind::F6,
            KeyKind::F7 => RpcKeyKind::F7,
            KeyKind::F8 => RpcKeyKind::F8,
            KeyKind::F9 => RpcKeyKind::F9,
            KeyKind::F10 => RpcKeyKind::F10,
            KeyKind::F11 => RpcKeyKind::F11,
            KeyKind::F12 => RpcKeyKind::F12,
            KeyKind::Up => RpcKeyKind::Up,
            KeyKind::Down => RpcKeyKind::Down,
            KeyKind::Left => RpcKeyKind::Left,
            KeyKind::Right => RpcKeyKind::Right,
            KeyKind::Home => RpcKeyKind::Home,
            KeyKind::End => RpcKeyKind::End,
            KeyKind::PageUp => RpcKeyKind::PageUp,
            KeyKind::PageDown => RpcKeyKind::PageDown,
            KeyKind::Insert => RpcKeyKind::Insert,
            KeyKind::Delete => RpcKeyKind::Delete,
            KeyKind::Ctrl => RpcKeyKind::Ctrl,
            KeyKind::Enter => RpcKeyKind::Enter,
            KeyKind::Space => RpcKeyKind::Space,
            KeyKind::Tilde => RpcKeyKind::Tilde,
            KeyKind::Quote => RpcKeyKind::Quote,
            KeyKind::Semicolon => RpcKeyKind::Semicolon,
            KeyKind::Comma => RpcKeyKind::Comma,
            KeyKind::Period => RpcKeyKind::Period,
            KeyKind::Slash => RpcKeyKind::Slash,
            KeyKind::Esc => RpcKeyKind::Esc,
            KeyKind::Shift => RpcKeyKind::Shift,
            KeyKind::Alt => RpcKeyKind::Alt,
            KeyKind::Backspace => RpcKeyKind::Backspace,
        }
    }
}

impl From<KeyKind> for KeyBinding {
    fn from(value: KeyKind) -> Self {
        match value {
            KeyKind::A => KeyBinding::A,
            KeyKind::B => KeyBinding::B,
            KeyKind::C => KeyBinding::C,
            KeyKind::D => KeyBinding::D,
            KeyKind::E => KeyBinding::E,
            KeyKind::F => KeyBinding::F,
            KeyKind::G => KeyBinding::G,
            KeyKind::H => KeyBinding::H,
            KeyKind::I => KeyBinding::I,
            KeyKind::J => KeyBinding::J,
            KeyKind::K => KeyBinding::K,
            KeyKind::L => KeyBinding::L,
            KeyKind::M => KeyBinding::M,
            KeyKind::N => KeyBinding::N,
            KeyKind::O => KeyBinding::O,
            KeyKind::P => KeyBinding::P,
            KeyKind::Q => KeyBinding::Q,
            KeyKind::R => KeyBinding::R,
            KeyKind::S => KeyBinding::S,
            KeyKind::T => KeyBinding::T,
            KeyKind::U => KeyBinding::U,
            KeyKind::V => KeyBinding::V,
            KeyKind::W => KeyBinding::W,
            KeyKind::X => KeyBinding::X,
            KeyKind::Y => KeyBinding::Y,
            KeyKind::Z => KeyBinding::Z,
            KeyKind::Zero => KeyBinding::Zero,
            KeyKind::One => KeyBinding::One,
            KeyKind::Two => KeyBinding::Two,
            KeyKind::Three => KeyBinding::Three,
            KeyKind::Four => KeyBinding::Four,
            KeyKind::Five => KeyBinding::Five,
            KeyKind::Six => KeyBinding::Six,
            KeyKind::Seven => KeyBinding::Seven,
            KeyKind::Eight => KeyBinding::Eight,
            KeyKind::Nine => KeyBinding::Nine,
            KeyKind::F1 => KeyBinding::F1,
            KeyKind::F2 => KeyBinding::F2,
            KeyKind::F3 => KeyBinding::F3,
            KeyKind::F4 => KeyBinding::F4,
            KeyKind::F5 => KeyBinding::F5,
            KeyKind::F6 => KeyBinding::F6,
            KeyKind::F7 => KeyBinding::F7,
            KeyKind::F8 => KeyBinding::F8,
            KeyKind::F9 => KeyBinding::F9,
            KeyKind::F10 => KeyBinding::F10,
            KeyKind::F11 => KeyBinding::F11,
            KeyKind::F12 => KeyBinding::F12,
            KeyKind::Up => KeyBinding::Up,
            KeyKind::Down => KeyBinding::Down,
            KeyKind::Left => KeyBinding::Left,
            KeyKind::Right => KeyBinding::Right,
            KeyKind::Home => KeyBinding::Home,
            KeyKind::End => KeyBinding::End,
            KeyKind::PageUp => KeyBinding::PageUp,
            KeyKind::PageDown => KeyBinding::PageDown,
            KeyKind::Insert => KeyBinding::Insert,
            KeyKind::Delete => KeyBinding::Delete,
            KeyKind::Enter => KeyBinding::Enter,
            KeyKind::Space => KeyBinding::Space,
            KeyKind::Tilde => KeyBinding::Tilde,
            KeyKind::Quote => KeyBinding::Quote,
            KeyKind::Semicolon => KeyBinding::Semicolon,
            KeyKind::Comma => KeyBinding::Comma,
            KeyKind::Period => KeyBinding::Period,
            KeyKind::Slash => KeyBinding::Slash,
            KeyKind::Esc => KeyBinding::Esc,
            KeyKind::Shift => KeyBinding::Shift,
            KeyKind::Ctrl => KeyBinding::Ctrl,
            KeyKind::Alt => KeyBinding::Alt,
            KeyKind::Backspace => KeyBinding::Backspace,
        }
    }
}

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

#[cfg_attr(test, automock)]
pub trait InputReceiver: Debug + 'static {
    fn set_window(&mut self, window: Window);

    fn set_method(&mut self, method: InputMethod);

    fn as_stream(&self) -> BoxStream<'static, KeyKind>;
}

#[derive(Debug)]
pub struct DefaultInputReceiver {
    window: Window,
    kind: PlatformInputKind,
    inner: PlatformInputReceiver,
}

impl DefaultInputReceiver {
    pub fn new(window: Window, kind: PlatformInputKind) -> Self {
        Self {
            window,
            kind,
            inner: PlatformInputReceiver::new(window, kind).expect("supported platform"),
        }
    }
}

impl InputReceiver for DefaultInputReceiver {
    fn set_window(&mut self, window: Window) {
        self.window = window;
        self.inner = PlatformInputReceiver::new(window, self.kind).expect("supported platform")
    }

    fn set_method(&mut self, method: InputMethod) {
        self.kind = match method {
            InputMethod::ForegroundRpc(_) | InputMethod::ForegroundDefault => {
                PlatformInputKind::Foreground
            }
            InputMethod::FocusedRpc(_) | InputMethod::FocusedDefault => PlatformInputKind::Focused,
        };
        self.inner =
            PlatformInputReceiver::new(self.window, self.kind).expect("supported platform");
    }

    fn as_stream(&self) -> BoxStream<'static, KeyKind> {
        self.inner
            .as_stream()
            .expect("supported platform")
            .map(KeyKind::from)
            .boxed()
    }
}

/// Options for key down input.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(PartialEq))]
pub struct InputKeyDownOptions {
    /// Whether the down stroke can be repeated even if the key is already down.
    ///
    /// Currently supports only [`InputMethod::Default`].
    repeatable: bool,
}

impl InputKeyDownOptions {
    pub fn repeatable(mut self) -> InputKeyDownOptions {
        self.repeatable = true;
        self
    }
}

/// Input method to use.
///
/// This is a bridge enum between platform-specific, database and gRPC input options.
#[derive(Clone, Debug)]
pub enum InputMethod {
    ForegroundRpc(String),
    FocusedRpc(String),
    ForegroundDefault,
    FocusedDefault,
}

impl From<&Settings> for InputMethod {
    fn from(settings: &Settings) -> Self {
        match (settings.input_method, settings.capture_mode) {
            (DatabaseInputMethod::Default, CaptureMode::BitBltArea) => {
                InputMethod::ForegroundDefault
            }
            (DatabaseInputMethod::Default, _) => InputMethod::FocusedDefault,
            (DatabaseInputMethod::Rpc, CaptureMode::BitBltArea) => {
                InputMethod::ForegroundRpc(settings.input_method_rpc_server_url.clone())
            }
            (DatabaseInputMethod::Rpc, _) => {
                InputMethod::FocusedRpc(settings.input_method_rpc_server_url.clone())
            }
        }
    }
}

/// Inner kind of [`InputMethod`].
///
/// The above [`InputMethod`] will be converted to this inner kind that contains the actual
/// sending structure.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum InputMethodInner {
    Rpc(RefCell<InputService>),
    Default(PlatformInput),
}

/// States of input delay tracking.
#[derive(Debug)]
enum InputDelay {
    Untracked,
    Tracked,
    AlreadyTracked,
}

/// A trait for sending inputs.
#[cfg_attr(test, automock)]
pub trait Input: Send + Debug {
    /// Performs a tick update.
    fn update_tick(&mut self, tick: u64);

    /// Overwrites the current input window with new `window`.
    fn set_window(&mut self, window: Window);

    /// Overwrites the current input method with new `method`.
    fn set_method(&mut self, method: InputMethod);

    /// The current state of input represented as a [`String`].
    fn state(&self) -> String;

    /// Sends mouse `kind` to `(x, y)` relative to the client coordinate (e.g. capture area).
    ///
    /// `(0, 0)` is top-left and `(width, height)` is bottom-right.
    fn send_mouse(&self, x: i32, y: i32, kind: MouseKind);

    /// Presses a single key `kind`.
    fn send_key(&self, kind: KeyKind);

    /// Releases a held key `kind`.
    fn send_key_up(&self, kind: KeyKind);

    /// Holds down key `kind`.
    ///
    /// This key stroke is sent with the default options.
    fn send_key_down(&self, kind: KeyKind) {
        self.send_key_down_with_options(kind, InputKeyDownOptions::default());
    }

    /// Same as [`Self::send_key_down`] but with the provided `options`.
    fn send_key_down_with_options(&self, kind: KeyKind, options: InputKeyDownOptions);

    /// Whether the key `kind` is cleared.
    fn is_key_cleared(&self, kind: KeyKind) -> bool;

    /// Whether all keys are cleared.
    fn all_keys_cleared(&self) -> bool;

    #[cfg(debug_assertions)]
    fn clone(&self) -> Box<dyn Input>;
}

/// Default implementation of [`Input`].
#[derive(Debug)]
pub struct DefaultInput {
    window: Window,
    method: InputMethod,
    method_inner: InputMethodInner,
    delay_rng: Rng,
    delay_mean_std_pair: (f32, f32),
    delay_map: RefCell<HashMap<KeyKind, (u32, bool)>>,
}

impl DefaultInput {
    pub fn new(window: Window, method: InputMethod, rng: Rng) -> Self {
        Self {
            window,
            method: method.clone(),
            method_inner: input_method_inner_from(window, method, rng.rng_seed()),
            delay_rng: rng,
            delay_mean_std_pair: (BASE_MEAN_MS_DELAY, BASE_STD_MS_DELAY),
            delay_map: RefCell::new(HashMap::new()),
        }
    }

    #[inline]
    fn key_state(&self, kind: KeyKind) -> Result<KeyState> {
        match &self.method_inner {
            InputMethodInner::Rpc(service) => service
                .borrow_mut()
                .key_state(kind.into())
                .map(KeyState::from)
                .ok_or(anyhow!("service not connected")),
            InputMethodInner::Default(input) => Ok(input.key_state(kind.into())?.into()),
        }
    }

    #[inline]
    fn send_key_inner(&self, kind: KeyKind) -> Result<()> {
        match &self.method_inner {
            InputMethodInner::Rpc(service) => {
                service
                    .borrow_mut()
                    .send_key(kind.into(), self.random_input_delay_tick_count().0);
            }
            InputMethodInner::Default(input) => match self.track_input_delay(kind) {
                InputDelay::Untracked => input.send_key(kind.into())?,
                InputDelay::Tracked => input.send_key_down(kind.into(), false)?,
                InputDelay::AlreadyTracked => (),
            },
        }

        Ok(())
    }

    #[inline]
    fn send_key_up_inner(&self, kind: KeyKind, forced: bool) -> Result<()> {
        match &self.method_inner {
            InputMethodInner::Rpc(service) => {
                service.borrow_mut().send_key_up(kind.into());
            }
            InputMethodInner::Default(input) => {
                if forced || !self.has_input_delay(kind) {
                    input.send_key_up(kind.into())?;
                }
            }
        }

        Ok(())
    }

    #[inline]
    fn send_key_down_inner(&self, kind: KeyKind, repeatable: bool) -> Result<()> {
        match &self.method_inner {
            // NOTE: For unknown reason, hardware custom input (e.g. KMBox, Arduino) seems to only
            // require sending down stroke once and it will continue correctly. But `SendInput`
            // requires repeatedly sending the stroke to simulate flying for some classes.
            InputMethodInner::Rpc(service) => {
                service.borrow_mut().send_key_down(kind.into());
            }
            InputMethodInner::Default(input) => {
                if !self.has_input_delay(kind) {
                    input.send_key_down(kind.into(), repeatable)?;
                }
            }
        }

        Ok(())
    }

    #[inline]
    fn has_input_delay(&self, kind: KeyKind) -> bool {
        self.delay_map.borrow().contains_key(&kind)
    }

    fn track_input_delay(&self, kind: KeyKind) -> InputDelay {
        let mut map = self.delay_map.borrow_mut();
        let entry = map.entry(kind);
        if matches!(entry, Entry::Occupied(_)) {
            return InputDelay::AlreadyTracked;
        }

        let (_, delay_tick_count) = self.random_input_delay_tick_count();
        if delay_tick_count == 0 {
            return InputDelay::Untracked;
        }

        let _ = entry.insert_entry((delay_tick_count, false));
        InputDelay::Tracked
    }

    #[inline]
    fn update(&mut self, game_tick: u64) {
        const UPDATE_MEAN_STD_PAIR_INTERVAL: u64 = 200;

        if game_tick > 0 && game_tick.is_multiple_of(UPDATE_MEAN_STD_PAIR_INTERVAL) {
            let (mean, std) = self.delay_mean_std_pair;
            self.delay_mean_std_pair = self.delay_rng.random_mean_std_pair(
                BASE_MEAN_MS_DELAY,
                mean,
                BASE_STD_MS_DELAY,
                std,
                MEAN_STD_REVERSION_RATE,
                MEAN_STD_VOLATILITY,
            )
        }

        let mut map = self.delay_map.borrow_mut();
        if map.is_empty() {
            return;
        }

        let mut keys_to_remove = HashSet::new();
        for (kind, (delay, did_send_up)) in map.iter_mut() {
            *delay = delay.saturating_sub(1);

            if *delay == 0 {
                if !*did_send_up {
                    *did_send_up = true;
                    let _ = self.send_key_up_inner(*kind, true);
                }

                if matches!(self.key_state(*kind), Ok(KeyState::Released)) {
                    keys_to_remove.insert(*kind);
                }
            }
        }

        if !keys_to_remove.is_empty() {
            map.retain(|kind, _| !keys_to_remove.contains(kind));
        }
    }

    fn random_input_delay_tick_count(&self) -> (f32, u32) {
        let (mean, std) = self.delay_mean_std_pair;
        self.delay_rng
            .random_delay_tick_count(mean, std, MS_PER_TICK_F32, 80.0, 120.0)
    }
}

impl Input for DefaultInput {
    fn update_tick(&mut self, tick: u64) {
        self.update(tick);
    }

    fn set_window(&mut self, window: Window) {
        self.window = window;
        self.set_method(self.method.clone());
    }

    fn set_method(&mut self, method: InputMethod) {
        self.method = method;
        self.method_inner =
            input_method_inner_from(self.window, self.method.clone(), self.delay_rng.rng_seed());
    }

    fn state(&self) -> String {
        match &self.method_inner {
            InputMethodInner::Rpc(service) => format!("RPC({})", service.borrow().state()),
            InputMethodInner::Default(_) => "SendInput".to_string(),
        }
    }

    fn send_mouse(&self, x: i32, y: i32, kind: MouseKind) {
        match &self.method_inner {
            InputMethodInner::Rpc(service) => {
                let mut borrow = service.borrow_mut();
                let relative = match borrow.mouse_coordinate() {
                    RpcCoordinate::Screen => CoordinateRelative::Monitor,
                    RpcCoordinate::Relative => CoordinateRelative::Window,
                };
                let Ok(coordinates) = self.window.convert_coordinate(x, y, relative) else {
                    return;
                };

                borrow.send_mouse(
                    coordinates.width,
                    coordinates.height,
                    coordinates.x,
                    coordinates.y,
                    kind.into(),
                );
            }
            InputMethodInner::Default(keys) => {
                let kind = match kind {
                    MouseKind::Move => PlatformMouseKind::Move,
                    MouseKind::Click => PlatformMouseKind::Click,
                    MouseKind::Scroll => PlatformMouseKind::Scroll,
                };
                let _ = keys.send_mouse(x, y, kind);
            }
        }
    }

    fn send_key(&self, kind: KeyKind) {
        let _ = self.send_key_inner(kind);
    }

    fn send_key_up(&self, kind: KeyKind) {
        let _ = self.send_key_up_inner(kind, false);
    }

    fn send_key_down_with_options(&self, kind: KeyKind, options: InputKeyDownOptions) {
        let _ = self.send_key_down_inner(kind, options.repeatable);
    }

    fn is_key_cleared(&self, kind: KeyKind) -> bool {
        !self.delay_map.borrow().contains_key(&kind)
    }

    #[inline]
    fn all_keys_cleared(&self) -> bool {
        self.delay_map.borrow().is_empty()
    }

    #[cfg(debug_assertions)]
    fn clone(&self) -> Box<dyn Input> {
        Box::new(DefaultInput::new(
            self.window,
            self.method.clone(),
            self.delay_rng.clone(),
        ))
    }
}

/// A trait for managing different capture modes.
///
/// A bridge trait between platform-specific and database.
#[cfg_attr(test, automock)]
pub trait Capture: Debug + 'static {
    fn grab(&mut self) -> Result<Frame, Error>;

    fn window(&self) -> Window;

    fn set_window(&mut self, window: Window);

    fn set_mode(&mut self, mode: CaptureMode);
}

#[derive(Debug)]
pub struct DefaultCapture {
    inner: PlatformCapture,
}

impl DefaultCapture {
    pub fn new(window: Window) -> Self {
        Self {
            inner: PlatformCapture::new(window).expect("supported platform"),
        }
    }
}

impl Capture for DefaultCapture {
    #[inline]
    fn grab(&mut self) -> Result<Frame, Error> {
        self.inner.grab()
    }

    #[inline]
    fn window(&self) -> Window {
        self.inner.window().expect("supported platform")
    }

    #[inline]
    fn set_window(&mut self, window: Window) {
        self.inner.set_window(window).expect("supported platform");
    }

    #[inline]
    fn set_mode(&mut self, mode: CaptureMode) {
        if cfg!(windows) {
            let kind = match mode {
                CaptureMode::BitBlt => WindowsCaptureKind::BitBlt,
                CaptureMode::WindowsGraphicsCapture => WindowsCaptureKind::Wgc,
                CaptureMode::BitBltArea => WindowsCaptureKind::BitBltArea,
            };
            let _ = self.inner.windows_capture_kind(kind);
        }
    }
}

#[inline]
fn input_method_inner_from(window: Window, method: InputMethod, seed: &[u8]) -> InputMethodInner {
    match method {
        InputMethod::ForegroundRpc(url) | InputMethod::FocusedRpc(url) => {
            let result = InputService::new(url, seed.to_vec());
            if result.is_err() {
                info!(target: "rpc", "failed to connect to input server possibly because of incorrect URL, fallback to default input method...");

                return InputMethodInner::Default(
                    PlatformInput::new(window, PlatformInputKind::Focused)
                        .expect("supported platform"),
                );
            }

            InputMethodInner::Rpc(RefCell::new(result.unwrap()))
        }
        InputMethod::ForegroundDefault => InputMethodInner::Default(
            PlatformInput::new(window, PlatformInputKind::Foreground).expect("supported platform"),
        ),
        InputMethod::FocusedDefault => InputMethodInner::Default(
            PlatformInput::new(window, PlatformInputKind::Focused).expect("supported platform"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches::assert_matches;

    use super::*;

    const SEED: [u8; 32] = [
        64, 241, 206, 219, 49, 21, 218, 145, 254, 152, 68, 176, 242, 238, 152, 14, 176, 241, 153,
        64, 44, 192, 172, 191, 191, 157, 107, 206, 193, 55, 115, 68,
    ];

    fn test_key_sender() -> DefaultInput {
        DefaultInput::new(
            Window::new("Handle"),
            InputMethod::FocusedDefault,
            Rng::new(SEED, 1337),
        )
    }

    #[test]
    fn track_input_delay_tracked() {
        let sender = test_key_sender();

        // Force rng to generate delay > 0
        let result = sender.track_input_delay(KeyKind::Ctrl);
        assert_matches!(result, InputDelay::Tracked);
        assert!(sender.has_input_delay(KeyKind::Ctrl));
    }

    #[test]
    fn track_input_delay_already_tracked() {
        let sender = test_key_sender();
        sender
            .delay_map
            .borrow_mut()
            .insert(KeyKind::Ctrl, (3, false));

        let result = sender.track_input_delay(KeyKind::Ctrl);
        assert_matches!(result, InputDelay::AlreadyTracked);
    }

    #[test]
    fn update_input_delay_decrement_and_release_key() {
        let mut sender = test_key_sender();
        let count = 50;
        sender
            .delay_map
            .borrow_mut()
            .insert(KeyKind::Ctrl, (count, false));

        for _ in 0..count {
            sender.update(0);
        }
        // After `count` updates, key should be released and removed
        assert!(!sender.has_input_delay(KeyKind::Ctrl));
    }

    #[test]
    fn update_input_delay_refresh_mean_std_pair_every_interval() {
        let mut sender = test_key_sender();
        let original_pair = sender.delay_mean_std_pair;

        // Simulate tick before the interval: should NOT update
        sender.update(199);
        assert_eq!(sender.delay_mean_std_pair, original_pair);

        // Simulate tick AT the interval: should update
        sender.update(200);
        assert_ne!(sender.delay_mean_std_pair, original_pair);
    }
}
