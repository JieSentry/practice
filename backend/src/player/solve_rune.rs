use log::info;

use super::{
    Player,
    actions::PlayerAction,
    timeout::{Lifecycle, next_timeout_lifecycle},
};
use crate::{
    bridge::KeyKind,
    ecs::Resources,
    player::{PlayerContext, PlayerEntity, next_action, timeout::Timeout},
    solvers::{RuneSolver, SolvingState},
};

/// Representing the current state of rune solving.
#[derive(Debug, Clone, Copy)]
pub enum State {
    /// Ensures stationary and all keys cleared before solving.
    Precondition(Timeout),
    /// Calibrates rune arrows for possible spinning arrows.
    Calibrating(Timeout),
    /// Solves for the rune arrows that possibly include spinning arrows.
    Solving(Timeout),
    /// Presses the keys.
    PressKeys(Timeout, [KeyKind; 4], usize),
    /// Terminal stage.
    Completed,
}

#[derive(Clone, Debug)]  
pub struct SolvingRune {  
    state: State,  
    solver: RuneSolver,  
    /// Consecutive precondition ticks where the player failed to become stationary.  
    /// A stuck movement key keeps the player moving so this keeps growing.  
    stuck_count: u32,  
}  
  
impl Default for SolvingRune {  
    fn default() -> Self {  
        Self {  
            state: State::Precondition(Timeout::default()),  
            solver: RuneSolver::default(),  
            stuck_count: 0,  
        }  
    }  
}

/// Updates the [`Player::SolvingRune`] contextual state.
///
/// Note: This state does not use any [`Task`], so all detections are blocking. But this should be
/// acceptable for this state.
pub fn update_solving_rune_state(resources: &mut Resources, player: &mut PlayerEntity) {  
    let Player::SolvingRune(mut solving_rune) = player.state.clone() else {  
        panic!("state is not solving rune");  
    };  
  
    let mut stuck_abort = false;  
    match solving_rune.state {  
        State::Precondition(_) => {  
            stuck_abort = update_precondition(resources, &player.context, &mut solving_rune)  
        }  
        State::Calibrating(_) => update_calibrating(  
            resources,  
            &mut solving_rune,  
            player.context.config.interact_key,  
        ),  
        State::Solving(_) => update_solving(resources, &mut solving_rune),  
        State::PressKeys(_, _, _) => update_press_keys(resources, &mut solving_rune),  
        State::Completed => unreachable!(),  
    }  
  
    // 卡键自愈：仅在通过 priority action `SolveRune` 解符文时生效。  
    // 若人物长时间无法静止（某个方向键卡住导致一直移动），松开四个方向键、  
    // 清理动作并回到 Idle，让 rotator 以当前位置重新下发 SolveRune。  
    // 注意：这里不调用 start_validating_rune —— 符文没解，需要被重新检测并重解。  
    if stuck_abort && matches!(next_action(&player.context), Some(PlayerAction::SolveRune)) {  
        info!(target:"backend/rune","abort solving rune due to stuck movement key, re-pathing");  
        resources.input.send_key_up(KeyKind::Up);  
        resources.input.send_key_up(KeyKind::Down);  
        resources.input.send_key_up(KeyKind::Left);  
        resources.input.send_key_up(KeyKind::Right);  
        player.context.clear_action_completed();  
        player.state = Player::Idle;  
        return;  
    }  
  
    let player_next_state = if matches!(solving_rune.state, State::Completed) {  
        Player::Idle  
    } else {  
        Player::SolvingRune(solving_rune)  
    };  
  
    match next_action(&player.context) {  
        Some(PlayerAction::SolveRune) => {  
            let is_terminal = matches!(player_next_state, Player::Idle);  
            if is_terminal {  
                player.context.clear_action_completed();  
                player.context.start_validating_rune();  
            }  
  
            player.state = player_next_state;  
        }  
        Some(_) => unreachable!(),  
        None => player.state = Player::Idle, // Force cancel if not from action  
    }  
}

fn update_precondition(  
    resources: &mut Resources,  
    player_context: &PlayerContext,  
    solving_rune: &mut SolvingRune,  
) -> bool {  
    // 连续多少 tick 无法静止就判定为卡键。Precondition 在超时后每 tick 复检一次，  
    // 所以这里单位是 tick。正常解符文时人物很快静止，stuck_count 不会累积到阈值，  
    // 因此不影响正常流程。需按实际帧率微调（默认约 300 tick）。  
    const STUCK_ABORT_COUNT: u32 = 300;  
  
    let State::Precondition(timeout) = solving_rune.state else {  
        panic!("solving rune state is not precondition")  
    };  
  
    match next_timeout_lifecycle(timeout, 15) {  
        Lifecycle::Ended => {  
            if player_context.is_stationary && resources.input.is_all_keys_cleared() {  
                solving_rune.stuck_count = 0;  
                solving_rune.state = State::Calibrating(Timeout::default());  
            } else {  
                // 无法静止：保持 maxed timeout，下一 tick 继续复检（与原逻辑一致），  
                // 同时累加卡键计数。  
                solving_rune.stuck_count = solving_rune.stuck_count.saturating_add(1);  
                solving_rune.state = State::Precondition(timeout);  
                if solving_rune.stuck_count >= STUCK_ABORT_COUNT {  
                    return true;  
                }  
            }  
        }  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            solving_rune.state = State::Precondition(timeout);  
        }  
    }  
  
    false  
}

fn update_calibrating(
    resources: &mut Resources,
    solving_rune: &mut SolvingRune,
    interact_key: KeyKind,
) {
    const COOLDOWN_AND_SOLVE_TIMEOUT: u32 = 125;
    const SOLVE_INTERVAL: u32 = 30;

    let State::Calibrating(timeout) = solving_rune.state else {
        panic!("solving rune state is not finding region")
    };

    match next_timeout_lifecycle(timeout, COOLDOWN_AND_SOLVE_TIMEOUT) {
        Lifecycle::Started(timeout) => {
            resources.input.send_key(interact_key);
            solving_rune.state = State::Calibrating(timeout);
        }
        Lifecycle::Ended => {
            solving_rune.state = State::Completed;
        }
        Lifecycle::Updated(timeout) => {
            if timeout.current.is_multiple_of(SOLVE_INTERVAL) {
                match solving_rune.solver.solve(resources.detector()) {
                    SolvingState::Calibrating => {
                        solving_rune.state = State::Calibrating(timeout);
                        return;
                    }
                    SolvingState::Solving => {
                        solving_rune.state = State::Solving(Timeout::default());
                        return;
                    }
                    SolvingState::Complete(_) | SolvingState::Error => unreachable!(),
                }
            }

            solving_rune.state = State::Calibrating(timeout);
        }
    }
}

fn update_solving(resources: &mut Resources, solving_rune: &mut SolvingRune) {
    let State::Solving(timeout) = solving_rune.state else {
        panic!("solving rune state is not solving")
    };

    match next_timeout_lifecycle(timeout, 150) {
        Lifecycle::Started(timeout) => {
            solving_rune.state = State::Solving(timeout);
        }
        Lifecycle::Ended => {
            solving_rune.state = State::Completed;
        }
        Lifecycle::Updated(timeout) => match solving_rune.solver.solve(resources.detector()) {
            SolvingState::Calibrating => {
                unreachable!()
            }
            SolvingState::Solving => {
                solving_rune.state = State::Solving(timeout);
            }
            SolvingState::Complete(arrows) => {
                info!(target:"backend/rune","solve result {arrows:?}");
                #[cfg(debug_assertions)]
                resources
                    .debug
                    .set_last_rune_result(resources.detector_cloned(), arrows);
                solving_rune.state =
                    State::PressKeys(Timeout::default(), arrows.map(|arrow| arrow.key), 0);
            }
            SolvingState::Error => {
                solving_rune.state = State::Completed;
            }
        },
    }
}

fn update_press_keys(resources: &mut Resources, solving_rune: &mut SolvingRune) {
    const PRESS_KEY_INTERVAL: u32 = 8;

    let State::PressKeys(timeout, keys, key_index) = solving_rune.state else {
        panic!("solving rune state is not pressing keys")
    };

    match next_timeout_lifecycle(timeout, PRESS_KEY_INTERVAL) {
        Lifecycle::Started(timeout) => {
            resources.input.send_key(keys[key_index]);
            solving_rune.state = State::PressKeys(timeout, keys, key_index);
        }
        Lifecycle::Ended => {
            solving_rune.state = if key_index + 1 < keys.len() {
                State::PressKeys(Timeout::default(), keys, key_index + 1)
            } else {
                State::Completed
            };
        }
        Lifecycle::Updated(timeout) => {
            solving_rune.state = State::PressKeys(timeout, keys, key_index);
        }
    }
}
