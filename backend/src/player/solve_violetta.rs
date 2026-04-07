use std::{  
    cell::RefCell,  
    fmt::{self, Display},  
    rc::Rc,  
};  
  
use anyhow::Result;  
use log::debug;  
use opencv::core::{Point, Rect};  
  
use crate::{  
    bridge::MouseKind,  
    detect::Detector,  
    ecs::Resources,  
    player::{  
        Player, PlayerAction, PlayerEntity, next_action,  
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},  
    },  
    solvers::ViolettaSolver,  
    task::{Task, Update, update_detection_task},  
};  
  
/// Representing the current state of Violetta (e.g. lie detector) solving.  
#[derive(Debug, Clone, Copy, Default)]  
pub enum State {  
    #[default]  
    Waiting,  
    Solving(Timeout),  
    Completed,  
}  
  
#[derive(Debug, Default)]  
pub struct SolvingVioletta {  
    state: State,  
    region: Rect,  
    solver: ViolettaSolver,  
    lie_detector_task: Rc<RefCell<Option<Task<Result<bool>>>>>,  
}  
  
impl Clone for SolvingVioletta {  
    fn clone(&self) -> Self {  
        Self {  
            state: self.state,  
            region: self.region,  
            solver: ViolettaSolver::default(),  
            lie_detector_task: self.lie_detector_task.clone(),  
        }  
    }  
}  
  
impl Display for SolvingVioletta {  
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {  
        match self.state {  
            State::Waiting => write!(f, "Waiting"),  
            State::Solving(_) => write!(f, "Solving"),  
            State::Completed => write!(f, "Completed"),  
        }  
    }  
}  
  
/// Updates the [`Player::SolvingVioletta`] contextual state.  
///  
/// Note: This state does not use any [`Task`], so all detections are blocking. But this should be  
/// acceptable for this state.  
pub fn update_solving_violetta_state(resources: &mut Resources, player: &mut PlayerEntity) {  
    let Player::SolvingVioletta(mut solving_violetta) = player.state.clone() else {  
        panic!("state is not solving violetta");  
    };  
  
    match solving_violetta.state {  
        State::Waiting => update_waiting(resources, &mut solving_violetta),  
        State::Solving(_) => update_solving(resources, &mut solving_violetta),  
        State::Completed => unreachable!(),  
    }  
  
    let player_next_state = if matches!(solving_violetta.state, State::Completed) {  
        Player::Idle  
    } else {  
        Player::SolvingVioletta(solving_violetta)  
    };  
  
    match next_action(&player.context) {  
        Some(PlayerAction::SolveVioletta) => {  
            if matches!(player_next_state, Player::Idle) {  
                player.context.clear_action_completed();  
            }  
  
            player.state = player_next_state;  
        }  
        Some(_) => unreachable!(),  
        None => player.state = Player::Idle, // Force cancel if not from action  
    }  
}  
  
fn update_waiting(resources: &mut Resources, solving_violetta: &mut SolvingVioletta) {  
    const CHECK_INTERVAL: u64 = 30;  
  
    let State::Waiting = solving_violetta.state else {  
        panic!("solving violetta state is not waiting")  
    };  
  
    if !resources.tick.is_multiple_of(CHECK_INTERVAL) {  
        return;  
    }  
    if resources  
        .detector()  
        .detect_lie_detector_violetta_preparing()  
    {  
        return;  
    }  
  
    let title = match resources.detector().detect_lie_detector_violetta() {  
        Ok(val) => val,  
        Err(_) => {  
            solving_violetta.state = State::Completed;  
            return;  
        }  
    };  
  
    let tl = title.tl() + Point::new(-180, 55);  
    let br = tl + Point::new(430, 250);  
    let region = Rect::from_points(tl, br);  
    solving_violetta.region = region;  
    solving_violetta.solver = ViolettaSolver::default();  
    solving_violetta.state = State::Solving(Timeout::default());  
    debug!(target: "backend/player","lie detector violetta region: {region:?}");  
}  
  
fn update_solving(resources: &mut Resources, solving_violetta: &mut SolvingVioletta) {  
    let State::Solving(timeout) = solving_violetta.state else {  
        panic!("solving violetta state is not solving")  
    };  
  
    // Avoids throttling the detection by using task  
    let update = update_detection_task(  
        resources,  
        1000,  
        &mut *solving_violetta.lie_detector_task.borrow_mut(),  
        |detector| Ok(detector.detect_lie_detector_violetta().is_ok()),  
    );  
    if let Update::Ok(has_lie_detector) = update  
        && !has_lie_detector  
    {  
        solving_violetta.state = State::Completed;  
        return;  
    }  
  
    match next_timeout_lifecycle(timeout, 1320) {  
        Lifecycle::Ended => {  
            solving_violetta.state = State::Completed;  
        }  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            if let Some(cursor) =  
                solving_violetta  
                    .solver  
                    .solve(resources.detector(), solving_violetta.region)  
            {  
                resources  
                    .input  
                    .send_mouse(cursor.x, cursor.y, MouseKind::Click);  
            }  
            solving_violetta.state = State::Solving(timeout);  
        }  
    }  
}
