use log::debug;
use opencv::core::Point;

use super::{
    AutoMob, Key, Move, Player, PlayerAction,
    actions::{next_action, update_from_ping_pong_action},
    double_jump::DoubleJumping,
    familiars_swap::FamiliarsSwapping,
    moving::{Moving, find_intermediate_points},
    panic::Panicking,
    use_key::UseKey,
};
use crate::{
    ActionKeyDirection, ActionKeyWith, Position,
    bridge::KeyKind,
    ecs::{Resources, transition, transition_if},
    minimap::Minimap,
    player::{
        ChattingContent, PlayerEntity, SolvingShape, chat::Chatting,
        exchange_booster::ExchangingBooster, solve_violetta::SolvingVioletta,
        transition_from_action, unstuck::Unstucking, use_booster::UsingBooster,
    },
    rng::Rng,
};

/// Updates [`Player::Idle`] contextual state.
///
/// This state does not do much on its own except when auto mobbing. It acts as entry
/// to other state when there is an action and helps clearing keys.
pub fn update_idle_state(resources: &Resources, player: &mut PlayerEntity, minimap_state: Minimap) {
    player.context.last_destinations = None;
    player.context.last_movement = None;
    player.context.stalling_timeout_state = None;
    player.state = Player::Idle; // Sets initial next state first
    resources.input.send_key_up(KeyKind::Up);
    resources.input.send_key_up(KeyKind::Down);
    resources.input.send_key_up(KeyKind::Left);
    resources.input.send_key_up(KeyKind::Right);

    update_from_action(resources, player, minimap_state);
}

fn update_from_action(resources: &Resources, player: &mut PlayerEntity, minimap_state: Minimap) {
    let context = &mut player.context;
    let action = next_action(context);

    match action {
        Some(PlayerAction::AutoMob(AutoMob { position, .. })) => {
            let point = Point::new(position.x, position.y);
            let intermediates = if context.config.auto_mob_platforms_pathing {
                match minimap_state {
                    Minimap::Idle(idle) => find_intermediate_points(
                        &idle.platforms,
                        context.last_known_pos.unwrap(),
                        point,
                        position.allow_adjusting,
                        context.config.auto_mob_platforms_pathing_up_jump_only,
                        false,
                    ),
                    _ => unreachable!(),
                }
            } else {
                None
            };
            let next = match intermediates {
                Some(mut intermediates) => {
                    let (point, exact) = intermediates.next().unwrap();
                    Player::Moving(point, exact, Some(intermediates))
                }
                None => Player::Moving(point, position.allow_adjusting, None),
            };

            context.auto_mob_clear_pathing_task();
            context.last_destinations = intermediates
                .map(|intermediates| {
                    intermediates
                        .inner()
                        .into_iter()
                        .map(|(point, _, _)| point)
                        .collect::<Vec<_>>()
                })
                .or(Some(vec![point]));
            transition!(player, next);
        }

        Some(PlayerAction::Move(Move { position, .. })) => {
            let x = get_x_destination(&resources.rng, position);
            let point = Point::new(x, position.y);

            debug!(target: "player", "handling move: {point:?}");
            transition!(
                player,
                Player::Moving(point, position.allow_adjusting, None)
            )
        }

        Some(PlayerAction::Key(Key {
            position: Some(position),
            ..
        })) => {
            let x = get_x_destination(&resources.rng, position);
            let point = Point::new(x, position.y);

            debug!(target: "player", "handling move: {point:?}");
            transition!(
                player,
                Player::Moving(point, position.allow_adjusting, None)
            );
        }

        Some(PlayerAction::Key(
            key @ Key {
                position: None,
                with: ActionKeyWith::DoubleJump,
                direction,
                ..
            },
        )) => {
            let last_pos = context.last_known_pos.unwrap();
            let last_direction = context.last_known_direction;

            transition_if!(
                player,
                Player::DoubleJumping(DoubleJumping::new(
                    Moving::new(last_pos, last_pos, false, None,),
                    true,
                    true,
                )),
                Player::UseKey(UseKey::from_key(key)),
                matches!(direction, ActionKeyDirection::Any) || direction == last_direction
            );
        }

        Some(PlayerAction::Key(
            key @ Key {
                position: None,
                with: ActionKeyWith::Any | ActionKeyWith::Stationary,
                ..
            },
        )) => transition!(player, Player::UseKey(UseKey::from_key(key))),

        Some(PlayerAction::SolveRune) => {
            let idle = match minimap_state {
                Minimap::Idle(idle) => idle,
                _ => transition_from_action!(player, Player::Idle),
            };
            let rune = match idle.rune() {
                Some(rune) => rune,
                None => transition_from_action!(player, Player::Idle),
            };

            context.last_destinations = Some(vec![rune]);
            transition_if!(
                player,
                Player::Moving(rune, false, None),
                !context.config.rune_platforms_pathing
            );
            transition_if!(!context.is_stationary);

            let intermediates = find_intermediate_points(
                &idle.platforms,
                context.last_known_pos.unwrap(),
                rune,
                true,
                context.config.rune_platforms_pathing_up_jump_only,
                true,
            );
            match intermediates {
                Some(mut intermediates) => {
                    context.last_destinations = Some(
                        intermediates
                            .inner()
                            .into_iter()
                            .map(|(point, _, _)| point)
                            .collect(),
                    );
                    let (point, exact) = intermediates.next().unwrap();
                    transition!(player, Player::Moving(point, exact, Some(intermediates)));
                }
                None => transition!(player, Player::Moving(rune, false, None)),
            }
        }

        Some(PlayerAction::PingPong(ping_pong)) => {
            let last_pos = context.last_known_pos.unwrap();
            update_from_ping_pong_action(resources, player, minimap_state, ping_pong, last_pos)
        }

        Some(PlayerAction::FamiliarsSwap(swapping)) => transition!(
            player,
            Player::FamiliarsSwapping(FamiliarsSwapping::new(
                swapping.swappable_slots,
                swapping.swappable_rarities
            ))
        ),

        Some(PlayerAction::Panic(panic)) => {
            transition!(player, Player::Panicking(Panicking::new(panic.to)))
        }

        Some(PlayerAction::Chat(chat)) => transition!(
            player,
            Player::Chatting(Chatting::new(ChattingContent::from_string(chat.content)))
        ),

        Some(PlayerAction::UseBooster(using)) => {
            transition!(player, Player::UsingBooster(UsingBooster::new(using.kind)))
        }

        Some(PlayerAction::ExchangeBooster(exchanging)) => {
            transition!(
                player,
                Player::ExchangingBooster(ExchangingBooster::new(
                    exchanging.amount,
                    exchanging.all
                ))
            )
        }

        Some(PlayerAction::Unstuck) => {
            transition!(player, Player::Unstucking(Unstucking::new_esc()))
        }

        Some(PlayerAction::SolveShape) => {
            transition!(player, Player::SolvingShape(SolvingShape::default()))
        }

        Some(PlayerAction::SolveVioletta) => {
            transition!(player, Player::SolvingVioletta(SolvingVioletta::default()))
        }

        None => (),
    }
}

fn get_x_destination(rng: &Rng, position: Position) -> i32 {
    let x_min = position.x.saturating_sub(position.x_random_range).max(0);
    let x_max = position.x.saturating_add(position.x_random_range + 1);
    rng.random_range(x_min..x_max)
}
