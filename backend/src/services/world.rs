use std::fmt::Debug;

use tokio::sync::broadcast::Receiver;

use super::EventContext;
use crate::{
    OperationUpdate,
    ecs::WorldEvent,
    notification::NotificationKind,
    player::{PanicTo, Panicking, Player},
    services::{EventHandler, operation::Halt},
};

/// A service to handle world-related incoming requests.
pub trait WorldService: Debug {
    /// Polls for any pending [`WorldEvent`].
    fn poll(&mut self) -> Option<WorldEvent>;
}

#[derive(Debug)]
pub struct DefaultWorldService {
    event_rx: Receiver<WorldEvent>,
}

impl DefaultWorldService {
    pub fn new(event_rx: Receiver<WorldEvent>) -> Self {
        Self { event_rx }
    }
}

impl WorldService for DefaultWorldService {
    fn poll(&mut self) -> Option<WorldEvent> {
        self.event_rx.try_recv().ok()
    }
}

pub struct WorldEventHandler;

impl EventHandler<WorldEvent> for WorldEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: WorldEvent) {
        match event {
            WorldEvent::CycledToHalt => {
                context.operation_service.queue_halt(
                    true,
                    Halt {
                        go_to_town: true,
                        check_for_navigation: false,
                    },
                );
                if context
                    .settings_service
                    .settings()
                    .notifications
                    .notify_on_cycle_run_stop
                {
                    let _ = context
                        .resources
                        .notification
                        .schedule_notification(NotificationKind::CycledToHalt);
                }
            }
            WorldEvent::CycledToRun => {
                if context
                    .settings_service
                    .settings()
                    .notifications
                    .notify_on_cycle_run_stop
                {
                    let _ = context
                        .resources
                        .notification
                        .schedule_notification(NotificationKind::CycledToRun);
                }
            }
            WorldEvent::PlayerDied => {
                if context.settings_service.settings().stop_on_player_die {
                    context.operation_service.queue_halt(
                        true,
                        Halt {
                            go_to_town: false,
                            check_for_navigation: false,
                        },
                    );
                }
            }
            WorldEvent::MinimapChanged => {
                if context.resources.operation.halting() {
                    return;
                }

                let _ = context
                    .resources
                    .notification
                    .schedule_notification(NotificationKind::FailOrMapChange);
                context.operation_service.abort_halt();

                if !context
                    .settings_service
                    .settings()
                    .stop_on_fail_or_change_map
                {
                    return;
                }

                let is_panicking = matches!(
                    context.world.player.state,
                    Player::Panicking(Panicking {
                        to: PanicTo::Channel,
                        ..
                    })
                );
                if is_panicking {
                    return;
                }

                context.operation_service.queue_halt(
                    false,
                    Halt {
                        go_to_town: true,
                        check_for_navigation: true,
                    },
                );
            }
            WorldEvent::CaptureFailed => {
                if context.resources.operation.halting() {
                    return;
                }

                if context
                    .settings_service
                    .settings()
                    .stop_on_fail_or_change_map
                {
                    context
                        .operation_service
                        .update(context.resources, OperationUpdate::TemporaryHalt);
                }
                let _ = context
                    .resources
                    .notification
                    .schedule_notification(NotificationKind::FailOrMapChange);
            }
            WorldEvent::LieDetectorAppeared => {
                if !context.resources.operation.halting() {
                    let _ = context
                        .resources
                        .notification
                        .schedule_notification(NotificationKind::LieDetectorAppear);
                }
            }
            WorldEvent::EliteBossAppeared => {
                if !context.resources.operation.halting() {
                    let _ = context
                        .resources
                        .notification
                        .schedule_notification(NotificationKind::EliteBossAppear);
                }
            }
        }
    }
}
