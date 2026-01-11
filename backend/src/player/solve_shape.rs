use log::debug;
use opencv::core::{Point, Rect};

use crate::{
    bridge::MouseKind,
    ecs::{Resources, transition, transition_if, try_ok_transition},
    player::{
        Player, PlayerAction, PlayerContext, PlayerEntity, next_action,
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},
        transition_from_action,
    },
    solvers::TransparentShapeSolver,
    tracker::ByteTracker,
};

/// Representing the current state of transparent shape (e.g. lie detector) solving.
#[derive(Debug, Clone, Copy, Default)]
pub enum State {
    #[default]
    Waiting,
    Solving(Timeout),
    Completed,
}

#[derive(Clone, Debug, Default)]
pub struct SolvingShape {
    state: State,
    region: Option<Rect>,
    solver: TransparentShapeSolver,
}

/// Updates the [`Player::SolvingShape`] contextual state.
///
/// Note: This state does not use any [`Task`], so all detections are blocking. But this should be
/// acceptable for this state.
pub fn update_solving_shape_state(resources: &Resources, player: &mut PlayerEntity) {
    let Player::SolvingShape(mut solving_shape) = player.state.clone() else {
        panic!("state is not solving shape");
    };

    match solving_shape.state {
        State::Waiting => update_waiting(resources, &mut player.context, &mut solving_shape),
        State::Solving(_) => update_solving(
            resources,
            player.context.shape_tracker(),
            &mut solving_shape,
        ),
        State::Completed => unreachable!(),
    }

    let player_next_state = if matches!(solving_shape.state, State::Completed) {
        Player::Idle
    } else {
        Player::SolvingShape(solving_shape)
    };

    match next_action(&player.context) {
        Some(PlayerAction::SolveShape) => transition_from_action!(
            player,
            player_next_state,
            matches!(player_next_state, Player::Idle)
        ),
        Some(_) => unreachable!(),
        None => transition!(player, Player::Idle), // Force cancel if not from action
    }
}

fn update_waiting(
    resources: &Resources,
    player_context: &mut PlayerContext,
    solving_shape: &mut SolvingShape,
) {
    const CHECK_INTERVAL: u64 = 30;

    let State::Waiting = solving_shape.state else {
        panic!("solving shape state is not waiting")
    };

    if !resources.tick.is_multiple_of(CHECK_INTERVAL) {
        return;
    }
    if resources.detector().detect_lie_detector_preparing() {
        return;
    }

    let title = try_ok_transition!(
        solving_shape,
        State::Completed,
        resources.detector().detect_lie_detector()
    );

    transition!(solving_shape, State::Solving(Timeout::default()), {
        let tl = title.tl();
        let br = title.br() + Point::new(660, 530);
        let region = Rect::from_points(tl, br);
        player_context.reset_shape_tracker();
        solving_shape.region = Some(region);
        debug!(target: "player", "lie detector transparent shape region: {region:?}");
    });
}

fn update_solving(
    resources: &Resources,
    tracker: &mut ByteTracker,
    solving_shape: &mut SolvingShape,
) {
    const CHECK_INTERVAL: u64 = 30;

    let State::Solving(timeout) = solving_shape.state else {
        panic!("solving shape state is not solving")
    };

    if resources.tick.is_multiple_of(CHECK_INTERVAL) {
        transition_if!(
            solving_shape,
            State::Completed,
            resources.detector().detect_lie_detector().is_err()
        );
    }

    match next_timeout_lifecycle(timeout, 545) {
        Lifecycle::Ended => transition!(solving_shape, State::Completed),
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            transition!(solving_shape, State::Solving(timeout), {
                let cursor = solving_shape.solver.solve(
                    resources.detector(),
                    tracker,
                    solving_shape.region.expect("set"),
                );
                resources
                    .input
                    .send_mouse(cursor.x, cursor.y, MouseKind::Move);
            })
        }
    }
}
