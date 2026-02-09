use std::fmt::Debug;

use opencv::{
    core::{ToInputArray, Vector},
    imgcodecs::imencode_def,
};
use serenity::all::{CreateAttachment, EditInteractionResponse};
use strum::EnumMessage;
use tokio::{sync::mpsc::UnboundedReceiver, task::spawn_blocking};

use super::EventContext;
use crate::{
    ActionKeyDirection, ActionKeyWith, OperationUpdate, Settings, WaitAfterBuffered,
    bridge::{KeyKind, LinkKeyKind},
    control::{BotAction, CommandKind, ControlEvent, DiscordBot},
    ecs::{Resources, World},
    player::{Chat, ChattingContent, Key, PlayerAction},
    services::{EventHandler, operation::Halt},
};

/// A service to handle control-related (e.g., Discord Bot) incoming requests.
pub trait ControlService: Debug {
    /// Updates the currently in use control settings with provided `settings`.
    fn update(&mut self, settings: &Settings);
}

#[derive(Debug)]
pub struct DefaultControlService {
    bot: DiscordBot,
}

impl DefaultControlService {
    pub fn new() -> (Self, UnboundedReceiver<ControlEvent>) {
        let (bot, event_rx) = DiscordBot::new();
        let service = Self { bot };
        (service, event_rx)
    }
}

impl ControlService for DefaultControlService {
    fn update(&mut self, settings: &Settings) {
        self.bot.start(settings.discord_bot_access_token.clone());
    }
}

pub struct ControlEventHandler;

impl EventHandler<ControlEvent> for ControlEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: ControlEvent) {
        match event.kind {
            CommandKind::Start => {
                if !context.resources.operation.halting() {
                    let _ = event
                        .sender
                        .send(EditInteractionResponse::new().content("Bot already running."));
                    return;
                }

                if context.map_service.map().is_none()
                    || context.character_service.character().is_none()
                {
                    let _ = event.sender.send(
                        EditInteractionResponse::new().content("No map or character data set."),
                    );
                    return;
                }

                let _ = event
                    .sender
                    .send(EditInteractionResponse::new().content("Bot started running."));
                context
                    .operation_service
                    .update(context.resources, OperationUpdate::Run);
            }
            CommandKind::Stop { go_to_town } => {
                let _ = event
                    .sender
                    .send(EditInteractionResponse::new().content("Bot stopped running."));
                context
                    .operation_service
                    .queue_halt(true, Halt { go_to_town });
            }
            CommandKind::Suspend => {
                let _ = event
                    .sender
                    .send(EditInteractionResponse::new().content("Bot attempted to suspend."));
                context
                    .operation_service
                    .update(context.resources, OperationUpdate::TemporaryHalt);
            }
            CommandKind::Status => {
                let provider = state_and_frame_provider(context.resources, context.world);

                spawn_blocking(move || {
                    let (status, frame) = provider();
                    let attachment =
                        frame.map(|bytes| CreateAttachment::bytes(bytes, "image.webp"));

                    let mut builder = EditInteractionResponse::new().content(status);
                    if let Some(attachment) = attachment {
                        builder = builder.new_attachment(attachment);
                    }

                    let _ = event.sender.send(builder);
                });
            }
            CommandKind::Chat { content } => {
                if content.chars().count() >= ChattingContent::MAX_LENGTH {
                    let builder = EditInteractionResponse::new().content(format!(
                        "Message length must be less than {} characters.",
                        ChattingContent::MAX_LENGTH
                    ));
                    let _ = event.sender.send(builder);
                    return;
                }

                let _ = event
                    .sender
                    .send(EditInteractionResponse::new().content("Queued a chat action."));
                let action = PlayerAction::Chat(Chat { content });
                context.rotator.inject_action(action);
            }
            CommandKind::Action { action, count } => {
                // Emulate these actions through keys instead to avoid requiring position
                let player_action = match action {
                    BotAction::Jump => PlayerAction::Key(Key {
                        key: context.world.player.context.config.jump_key,
                        key_hold_ticks: 0,
                        key_hold_buffered_to_wait_after: false,
                        link_key: LinkKeyKind::None,
                        count,
                        position: None,
                        direction: ActionKeyDirection::Any, // Must always be Any
                        with: ActionKeyWith::Any,           // Must always be Any
                        wait_before_use_ticks: 0,
                        wait_before_use_ticks_random_range: 5,
                        wait_after_use_ticks: 15,
                        wait_after_use_ticks_random_range: 0,
                        wait_after_buffered: WaitAfterBuffered::None,
                    }),
                    BotAction::DoubleJump => {
                        PlayerAction::Key(Key {
                            key: context.world.player.context.config.jump_key,
                            key_hold_ticks: 0,
                            key_hold_buffered_to_wait_after: false,
                            link_key: LinkKeyKind::Before(
                                context.world.player.context.config.jump_key,
                            ),
                            count,
                            position: None,
                            direction: ActionKeyDirection::Any, // Must always be Any
                            with: ActionKeyWith::Any,           // Must always be Any
                            wait_before_use_ticks: 0,
                            wait_before_use_ticks_random_range: 0,
                            wait_after_use_ticks: 0,
                            wait_after_use_ticks_random_range: 55,
                            wait_after_buffered: WaitAfterBuffered::None,
                        })
                    }
                    BotAction::Crouch => {
                        PlayerAction::Key(Key {
                            key: KeyKind::Down,
                            key_hold_ticks: 4,
                            key_hold_buffered_to_wait_after: false,
                            link_key: LinkKeyKind::None,
                            count,
                            position: None,
                            direction: ActionKeyDirection::Any, // Must always be Any
                            with: ActionKeyWith::Any,           // Must always be Any
                            wait_before_use_ticks: 0,
                            wait_before_use_ticks_random_range: 0,
                            wait_after_use_ticks: 10,
                            wait_after_use_ticks_random_range: 0,
                            wait_after_buffered: WaitAfterBuffered::None,
                        })
                    }
                };
                context.rotator.inject_action(player_action.clone());
                let _ = event
                    .sender
                    .send(EditInteractionResponse::new().content(format!(
                        "Queued `{}` x {count}",
                        action.get_message().expect("has message")
                    )));
            }
        }
    }
}

fn state_and_frame_provider(
    resources: &Resources,
    world: &World,
) -> impl FnOnce() -> (String, Option<Vec<u8>>) + Send + 'static {
    #[inline]
    fn frame_from(mat: &impl ToInputArray) -> Option<Vec<u8>> {
        let mut vector = Vector::new();
        imencode_def(".webp", mat, &mut vector).ok()?;
        Some(Vec::from_iter(vector))
    }

    let detector = resources.detector.as_ref().cloned();
    let state = world.player.state.to_string();
    let operation = resources.operation.to_string();

    move || {
        let frame = detector.and_then(|detector| frame_from(&detector.mat()));
        let info = [
            format!("- State: ``{state}``"),
            format!("- Operation: ``{operation}``"),
        ]
        .join("\n");

        (info, frame)
    }
}
