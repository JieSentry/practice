use std::fmt::Debug;

use futures::StreamExt;
#[cfg(test)]
use mockall::automock;
use platforms::Window;
use tokio::{
    spawn,
    sync::broadcast::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::{
    KeyBinding, OperationUpdate,
    bridge::{Input, InputMethod, InputReceiver, KeyKind},
    services::{Event, EventContext, EventHandler},
};

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    KeyReceived(KeyKind),
}

impl Event for InputEvent {}

/// A service to handle input-related incoming requests.
#[cfg_attr(test, automock)]
pub trait InputService: Debug {
    fn subscribe_event(&self) -> Receiver<InputEvent>;

    fn subscribe_key(&self) -> Receiver<KeyBinding>;

    /// Updates `input` to use the new `window`.
    fn apply_window(&mut self, input: &mut dyn Input, window: Window);

    /// Updates `input` to use the new `method`.
    fn apply_method(&mut self, input: &mut dyn Input, method: InputMethod);
}

#[derive(Debug)]
pub struct DefaultInputService {
    input_tx: Sender<KeyBinding>,
    input_rx: Box<dyn InputReceiver>,
    event_tx: Sender<InputEvent>,
    event_task: Option<JoinHandle<()>>,
}

impl DefaultInputService {
    pub fn new(input_rx: impl InputReceiver) -> Self {
        Self {
            input_tx: broadcast::channel(1).0,
            input_rx: Box::new(input_rx),
            event_tx: broadcast::channel(5).0,
            event_task: None,
        }
    }

    fn run_task(&mut self) {
        if let Some(handle) = self.event_task.take() {
            handle.abort();
        }

        let input_tx = self.input_tx.clone();
        let event_tx = self.event_tx.clone();
        let mut input_stream = self.input_rx.as_stream();
        let task = spawn(async move {
            while let Some(key) = input_stream.next().await {
                let _ = event_tx.send(InputEvent::KeyReceived(key));
                let _ = input_tx.send(key.into());
            }
        });

        self.event_task = Some(task);
    }
}

impl InputService for DefaultInputService {
    fn subscribe_event(&self) -> Receiver<InputEvent> {
        self.event_tx.subscribe()
    }

    fn subscribe_key(&self) -> Receiver<KeyBinding> {
        self.input_tx.subscribe()
    }

    fn apply_window(&mut self, input: &mut dyn Input, window: Window) {
        input.set_window(window);
        self.input_rx.set_window(window);
        self.run_task();
    }

    fn apply_method(&mut self, input: &mut dyn Input, method: InputMethod) {
        input.set_method(method.clone());
        self.input_rx.set_method(method);
        self.run_task();
    }
}

pub struct InputEventHandler;

impl EventHandler<InputEvent> for InputEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: InputEvent) {
        match event {
            InputEvent::KeyReceived(received_key) => {
                let toggle_actions_key = context.settings_service.settings().toggle_actions_key;
                if !toggle_actions_key.enabled {
                    return;
                }

                if toggle_actions_key.key == received_key.into() {
                    let update = if context.resources.operation.halting() {
                        OperationUpdate::Run
                    } else {
                        OperationUpdate::TemporaryHalt
                    };

                    context.operation_service.update(context.resources, update);
                }
            }
        }
    }
}
