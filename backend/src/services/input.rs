use std::fmt::Debug;

#[cfg(test)]
use mockall::automock;
use platforms::Window;

use crate::bridge::{Input, InputMethod, InputReceiver};

/// A service to handle input-related incoming requests.
#[cfg_attr(test, automock)]
pub trait InputService: Debug {
    /// Updates `input` and `input_rx` to use the new `window`.
    fn apply_window(&self, input: &mut dyn Input, input_rx: &mut dyn InputReceiver, window: Window);

    /// Updates `input` and `input_rx` to use the new `method`.
    fn apply_method(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        method: InputMethod,
    );
}

#[derive(Debug, Default)]
pub struct DefaultInputService;

impl InputService for DefaultInputService {
    fn apply_window(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        window: Window,
    ) {
        input.set_window(window);
        input_rx.set_window(window);
    }

    fn apply_method(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        method: InputMethod,
    ) {
        input.set_method(method.clone());
        input_rx.set_method(method);
    }
}
