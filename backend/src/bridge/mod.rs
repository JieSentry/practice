use std::fmt::Debug;

use anyhow::{Result, anyhow};
use futures::StreamExt;
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
        InputReceiver as PlatformInputReceiver, MouseKind as PlatformMouseKind,
    },
};

use crate::{
    grpc::{InputService, input::Coordinate as RpcCoordinate},
    models::{CaptureMode, InputMethod as DatabaseInputMethod, Settings},
    rng::Rng,
};

mod convert;

pub use convert::*;

/// Base mean in milliseconds to generate a pair from.
const BASE_MEAN_MS_DELAY: f32 = 100.0;

/// Base standard deviation in milliseconds to generate a pair from.
const BASE_STD_MS_DELAY: f32 = 20.0;

/// The rate at which generated standard deviation will revert to the base [`BASE_STD_MS_DELAY`]
/// over time.
const MEAN_STD_REVERSION_RATE: f32 = 0.2;

/// The rate at which generated mean will revert to the base [`BASE_MEAN_MS_DELAY`] over time.
const MEAN_STD_VOLATILITY: f32 = 3.0;

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
    repeatable: bool,
}

impl InputKeyDownOptions {
    /// Marks the down stroke as repeatable and allows it to be sent again even if the key
    /// is already down.
    ///
    /// Currently supports only [`InputMethod::Default`].
    pub fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }
}

/// Options for key input.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(PartialEq))]
pub struct InputKeyOptions {
    down_ms: Option<u64>,
}

impl InputKeyOptions {
    /// Specifies the duration of down stroke.
    ///
    /// By default, the duration is randomized.
    pub fn down_ms(mut self, ms: u64) -> Self {
        self.down_ms = Some(ms);
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
    Rpc(InputService),
    Default(PlatformInput),
}

impl InputMethodInner {
    fn key_state(&mut self, kind: KeyKind) -> Result<KeyState> {
        match self {
            InputMethodInner::Rpc(service) => service
                .key_state(kind.into())
                .map(KeyState::from)
                .ok_or(anyhow!("service not connected")),
            InputMethodInner::Default(input) => Ok(input.key_state(kind.into())?.into()),
        }
    }

    fn is_all_keys_cleared(&self) -> bool {
        match self {
            InputMethodInner::Rpc(service) => service.is_all_keys_cleared(),
            InputMethodInner::Default(input) => {
                input.is_all_keys_cleared().expect("supported platform")
            }
        }
    }

    fn send_key(&mut self, kind: KeyKind, down_ms: u64) {
        match self {
            InputMethodInner::Rpc(service) => {
                service.send_key(kind.into(), down_ms as f32);
            }
            InputMethodInner::Default(input) => {
                let _ = input.send_key(kind.into(), down_ms);
            }
        }
    }

    fn send_key_down(&mut self, kind: KeyKind, repeatable: bool) {
        match self {
            InputMethodInner::Rpc(service) => {
                // NOTE: For unknown reason, hardware custom input (e.g. KMBox, Arduino) seems to
                // only require sending down stroke once and it will continue correctly.
                // But `SendInput` requires repeatedly sending the stroke to simulate flying for
                // some classes.
                service.send_key_down(kind.into());
            }
            InputMethodInner::Default(input) => {
                let _ = input.send_key_down(kind.into(), repeatable);
            }
        }
    }

    fn send_key_up(&mut self, kind: KeyKind) {
        match self {
            InputMethodInner::Rpc(service) => {
                service.send_key_up(kind.into());
            }
            InputMethodInner::Default(input) => {
                let _ = input.send_key_up(kind.into());
            }
        }
    }
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
    fn send_mouse(&mut self, x: i32, y: i32, kind: MouseKind);

    /// Presses a single key `kind` using default options.
    fn send_key(&mut self, kind: KeyKind) {
        self.send_key_with_options(kind, InputKeyOptions::default());
    }

    /// Same as [`Self::send_key`] but with the provided `options`.
    fn send_key_with_options(&mut self, kind: KeyKind, options: InputKeyOptions);

    /// Releases a held key `kind`.
    fn send_key_up(&mut self, kind: KeyKind);

    /// Holds down key `kind` using default options.
    fn send_key_down(&mut self, kind: KeyKind) {
        self.send_key_down_with_options(kind, InputKeyDownOptions::default());
    }

    /// Same as [`Self::send_key_down`] but with the provided `options`.
    fn send_key_down_with_options(&mut self, kind: KeyKind, options: InputKeyDownOptions);

    /// Whether the key `kind` is cleared.
    fn is_key_cleared(&mut self, kind: KeyKind) -> bool;

    /// Whether all keys are cleared.
    fn is_all_keys_cleared(&self) -> bool;

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
}

impl DefaultInput {
    pub fn new(window: Window, method: InputMethod, rng: Rng) -> Self {
        Self {
            window,
            method: method.clone(),
            method_inner: input_method_inner_from(window, method, rng.rng_seed()),
            delay_rng: rng,
            delay_mean_std_pair: (BASE_MEAN_MS_DELAY, BASE_STD_MS_DELAY),
        }
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
    }

    fn random_key_delay(&mut self) -> u64 {
        let (mean, std) = self.delay_mean_std_pair;
        self.delay_rng.random_key_delay(mean, std, 80.0, 120.0) as u64
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
            InputMethodInner::Rpc(service) => format!("RPC({})", service.state()),
            InputMethodInner::Default(_) => "SendInput".to_string(),
        }
    }

    fn send_mouse(&mut self, x: i32, y: i32, kind: MouseKind) {
        match &mut self.method_inner {
            InputMethodInner::Rpc(service) => {
                let relative = match service.mouse_coordinate() {
                    RpcCoordinate::Screen => CoordinateRelative::Monitor,
                    RpcCoordinate::Relative => CoordinateRelative::Window,
                };
                let Ok(coordinates) = self.window.convert_coordinate(x, y, relative) else {
                    return;
                };

                service.send_mouse(
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

    fn send_key_with_options(&mut self, kind: KeyKind, options: InputKeyOptions) {
        let delay = options.down_ms.unwrap_or_else(|| self.random_key_delay());
        self.method_inner.send_key(kind, delay);
    }

    fn send_key_up(&mut self, kind: KeyKind) {
        self.method_inner.send_key_up(kind);
    }

    fn send_key_down_with_options(&mut self, kind: KeyKind, options: InputKeyDownOptions) {
        self.method_inner.send_key_down(kind, options.repeatable);
    }

    fn is_key_cleared(&mut self, kind: KeyKind) -> bool {
        self.method_inner
            .key_state(kind)
            .is_ok_and(|state| matches!(state, KeyState::Released))
    }

    #[inline]
    fn is_all_keys_cleared(&self) -> bool {
        self.method_inner.is_all_keys_cleared()
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
                info!(target: "backend/rpc", "failed to connect to input server possibly because of incorrect URL, fallback to default input method...");

                return InputMethodInner::Default(
                    PlatformInput::new(window, PlatformInputKind::Focused)
                        .expect("supported platform"),
                );
            }

            InputMethodInner::Rpc(result.unwrap())
        }
        InputMethod::ForegroundDefault => InputMethodInner::Default(
            PlatformInput::new(window, PlatformInputKind::Foreground).expect("supported platform"),
        ),
        InputMethod::FocusedDefault => InputMethodInner::Default(
            PlatformInput::new(window, PlatformInputKind::Focused).expect("supported platform"),
        ),
    }
}
