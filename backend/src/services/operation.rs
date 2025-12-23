use std::{
    fmt::Debug,
    time::{Duration, Instant},
};

use log::info;
use tokio::{
    spawn,
    sync::broadcast::{self, Receiver, Sender},
    task::JoinHandle,
    time::sleep,
};

use super::EventContext;
use crate::{
    CycleRunStopMode, OperationUpdate,
    ecs::Resources,
    operation::{Operation, OperationConfiguration},
    player::{Panic, PanicTo, PlayerAction},
    services::{Event, EventHandler},
};

const PENDING_HALT_SECS: u64 = 12;

#[derive(Debug, Clone, Copy)]
pub struct Halt {
    pub go_to_town: bool,
    pub check_for_navigation: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum OperationEvent {
    Halt(Halt),
    Update,
    Configuration,
}

impl Event for OperationEvent {}

/// A service to handle operation-related incoming requests.
pub trait OperationService: Debug {
    /// Polls for any pending [`OperationEvent`].
    fn poll(&mut self) -> Option<OperationEvent>;

    /// Applies the new `update` to `resources` and sends an [`OperationEvent::Update`] event.
    fn update(&self, resources: &mut Resources, update: OperationUpdate);

    /// Applies the new `config` to `resources` and sends an [`OperationEvent::Configuration`]
    /// event.
    fn config(&self, resources: &mut Resources, config: OperationConfiguration);

    /// Queues a [`OperationEvent::Halt`] event.
    fn queue_halt(&mut self, immediate: bool, halt: Halt);

    /// Aborts the previous [`OperationService::queue_halt`] if possible.
    fn abort_halt(&mut self);
}

#[derive(Debug)]
pub struct DefaultOperationService {
    pending_halt: Option<JoinHandle<()>>,
    event_rx: Receiver<OperationEvent>, // TODO: Remove this field
    event_tx: Sender<OperationEvent>,
}

impl Default for DefaultOperationService {
    fn default() -> Self {
        let (tx, rx) = broadcast::channel(5);

        Self {
            pending_halt: None,
            event_rx: rx,
            event_tx: tx,
        }
    }
}

impl OperationService for DefaultOperationService {
    fn poll(&mut self) -> Option<OperationEvent> {
        self.event_rx.try_recv().ok()
    }

    fn update(&self, resources: &mut Resources, update: OperationUpdate) {
        resources.operation = update_operation(resources.operation, update);
        let _ = self.event_tx.send(OperationEvent::Update);
    }

    fn config(&self, resources: &mut Resources, config: OperationConfiguration) {
        resources.operation = config_operation(resources.operation, config);
        let _ = self.event_tx.send(OperationEvent::Configuration);
    }

    fn queue_halt(&mut self, immediate: bool, halt: Halt) {
        self.abort_halt();

        let event = OperationEvent::Halt(halt);
        let tx = self.event_tx.clone();

        if immediate {
            let _ = tx.send(event);
        } else {
            let duration = Duration::from_secs(PENDING_HALT_SECS);
            let handle = spawn(async move {
                sleep(duration).await;
                let _ = tx.send(event);
            });

            self.pending_halt = Some(handle);
        }
    }

    fn abort_halt(&mut self) {
        if let Some(handle) = self.pending_halt.take() {
            handle.abort();
        }
    }
}

fn update_operation(operation: Operation, update: OperationUpdate) -> Operation {
    match update {
        OperationUpdate::TemporaryHalt => {
            if let Operation::RunUntil { instant, config } = operation {
                Operation::TemporaryHalting {
                    resume: instant.saturating_duration_since(Instant::now()),
                    config,
                }
            } else {
                Operation::Halting
            }
        }
        OperationUpdate::Run => match operation {
            Operation::TemporaryHalting { resume, config } => Operation::RunUntil {
                instant: Instant::now() + resume,
                config,
            },
            Operation::HaltUntil { config, .. } => Operation::run_until(config),
            _ => {
                info!(target: "operation", "invalid run update provided for the current state");
                operation
            }
        },
        OperationUpdate::Halt => Operation::Halting,
    }
}

fn config_operation(operation: Operation, config: OperationConfiguration) -> Operation {
    match operation {
        Operation::HaltUntil {
            instant,
            config: current_config,
        } => match config.mode {
            CycleRunStopMode::None | CycleRunStopMode::Once => Operation::Halting,
            CycleRunStopMode::Repeat => {
                if current_config.stop_duration_millis == config.stop_duration_millis {
                    Operation::HaltUntil { instant, config }
                } else {
                    Operation::halt_until(config)
                }
            }
        },
        Operation::TemporaryHalting {
            resume,
            config: current_config,
        } => {
            if current_config.run_duration_millis != config.run_duration_millis
                || matches!(config.mode, CycleRunStopMode::None)
            {
                Operation::Halting
            } else {
                Operation::TemporaryHalting { resume, config }
            }
        }
        Operation::Halting => Operation::Halting,
        Operation::Running | Operation::RunUntil { .. } => match config.mode {
            CycleRunStopMode::None => Operation::Running,
            CycleRunStopMode::Once | CycleRunStopMode::Repeat => Operation::run_until(config),
        },
    }
}

pub struct OperationEventHandler;

impl EventHandler<OperationEvent> for OperationEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: OperationEvent) {
        match event {
            OperationEvent::Halt(Halt {
                go_to_town,
                check_for_navigation,
            }) => {
                if check_for_navigation && context.navigator.was_last_point_available_or_completed()
                {
                    return;
                }

                context.resources.operation =
                    update_operation(context.resources.operation, OperationUpdate::TemporaryHalt);
                context.rotator.reset_queue();
                context
                    .world
                    .player
                    .context
                    .clear_actions_aborted(!go_to_town);

                if go_to_town {
                    context
                        .rotator
                        .inject_action(PlayerAction::Panic(Panic { to: PanicTo::Town }));
                }
            }
            OperationEvent::Update | OperationEvent::Configuration => {
                if context.resources.operation.halting() {
                    context.rotator.reset_queue();
                    context.world.player.context.clear_actions_aborted(true);
                }
            }
        }
    }
}
