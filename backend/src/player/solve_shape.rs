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
    ecs::Resources,  
    player::{  
        Player, PlayerAction, PlayerEntity, next_action,  
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},  
    },  
    solvers::TransparentShapeSolver,  
    task::{Task, Update, update_detection_task},  
};  
  
/// Representing the current state of transparent shape (e.g. lie detector) solving.  
#[derive(Debug, Clone, Copy, Default)]  
pub enum State {  
    #[default]  
    Waiting,  
    Solving(Timeout),  
    Completed,  
}  
  
#[derive(Debug, Default)]  
pub struct SolvingShape {  
    state: State,  
    region: Rect,  
    solver: TransparentShapeSolver,  
    lie_detector_task: Rc<RefCell<Option<Task<Result<bool>>>>>,  
    /// 最近一次真实检测得到的目标位置（每 1/4 秒更新一次）  
    target_cursor: Option<Point>,  
    /// 每帧输出的平滑后位置  
    smoothed_cursor: Option<Point>,  
}
  
impl Clone for SolvingShape {  
    fn clone(&self) -> Self {  
        Self {  
            state: self.state,  
            region: self.region,  
            solver: TransparentShapeSolver::default(),  
            lie_detector_task: self.lie_detector_task.clone(),  
            target_cursor: self.target_cursor,  
            smoothed_cursor: self.smoothed_cursor,  
        }  
    }  
}
  
impl Display for SolvingShape {  
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {  
        match self.state {  
            State::Waiting => write!(f, "Waiting"),  
            State::Solving(_) => write!(f, "Solving"),  
            State::Completed => write!(f, "Completed"),  
        }  
    }  
}  
  
/// Updates the [`Player::SolvingShape`] contextual state.  
///  
/// Note: This state does not use any [`Task`], so all detections are blocking. But this should be  
/// acceptable for this state.  
pub fn update_solving_shape_state(resources: &mut Resources, player: &mut PlayerEntity) {  
    let Player::SolvingShape(mut solving_shape) =  
        std::mem::replace(&mut player.state, Player::Idle) 
    else {
        panic!("state is not solving shape");
    };  
  
    match solving_shape.state {  
        State::Waiting => update_waiting(resources, &mut solving_shape),  
        State::Solving(_) => update_solving(resources, &mut solving_shape),  
        State::Completed => unreachable!(),  
    }  
  
    let player_next_state = if matches!(solving_shape.state, State::Completed) {  
        Player::Idle  
    } else {  
        Player::SolvingShape(solving_shape)  
    };  
  
    match next_action(&player.context) {  
        Some(PlayerAction::SolveShape) => {  
            if matches!(player_next_state, Player::Idle) {  
                player.context.clear_action_completed();  
            }  
  
            player.state = player_next_state;  
        }  
        Some(_) => unreachable!(),  
        None => player.state = Player::Idle, // Force cancel if not from action  
    }  
}  
  
fn update_waiting(resources: &mut Resources, solving_shape: &mut SolvingShape) {  
    const CHECK_INTERVAL: u64 = 30;  
  
    let State::Waiting = solving_shape.state else {  
        panic!("solving shape state is not waiting")  
    };  
  
    if !resources.tick.is_multiple_of(CHECK_INTERVAL) {  
        return;  
    }  
    if resources.detector().detect_lie_detector_shape_preparing() {  
        return;  
    }  
  
    let title = match resources.detector().detect_lie_detector_shape() {  
        Ok(val) => val,  
        Err(_) => {  
            solving_shape.state = State::Completed;  
            return;  
        }  
    };  
  
    let tl = title.tl() + Point::new(0, 20);  
    let br = tl + Point::new(755, 505);  
    let region = Rect::from_points(tl, br);  
    solving_shape.region = region;  
    solving_shape.solver = TransparentShapeSolver::default();  
    solving_shape.state = State::Solving(Timeout::default());  
    debug!(target:"backend/player","lie detector transparent shape region: {region:?}");  
}  
  
fn update_solving(resources: &mut Resources, solving_shape: &mut SolvingShape) {  
    let State::Solving(timeout) = solving_shape.state else {  
        panic!("solving shape state is not solving")  
    };  
  
    // Avoids throttling the detection by using task  
    let update = update_detection_task(  
        resources,  
        1000,  
        &mut *solving_shape.lie_detector_task.borrow_mut(),  
        |detector| Ok(detector.detect_lie_detector_shape().is_ok()),  
    );  
    if let Update::Ok(has_lie_detector) = update  
        && !has_lie_detector  
    {  
        solving_shape.state = State::Completed;  
        return;  
    }  
  
    match next_timeout_lifecycle(timeout, 545) {  
        Lifecycle::Ended => {  
            solving_shape.state = State::Completed;  
        }  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            // 每 1/4 秒（≈8 帧 @30FPS）才跑一次 YOLO 检测，其余帧只做平滑输出。  
            // 30FPS → 250ms ≈ 7.5 帧，取 8。若 solving 期实际是 10FPS，请改成 3。  
            const SOLVE_INTERVAL: u64 = 8;  
            // 平滑系数：越大越跟手（滞后小、抖动大），越小越平滑（滞后大）。0.3~0.5 之间调。  
            const ALPHA: f64 = 0.35;  
  
            if resources.tick.is_multiple_of(SOLVE_INTERVAL)  
                && let Some(cursor) =  
                    solving_shape.solver.solve(resources.detector(), solving_shape.region)  
            {  
                solving_shape.target_cursor = Some(cursor);  
            }  
  
            // 每帧朝目标做指数平滑（EMA），让鼠标连续顺滑地追向最近一次检测位置。  
            if let Some(target) = solving_shape.target_cursor {  
                let current = solving_shape.smoothed_cursor.unwrap_or(target);  
                let next = Point::new(  
                    (current.x as f64 + (target.x - current.x) as f64 * ALPHA).round() as i32,  
                    (current.y as f64 + (target.y - current.y) as f64 * ALPHA).round() as i32,  
                );  
                solving_shape.smoothed_cursor = Some(next);  
                resources  
                    .input  
                    .send_mouse(next.x, next.y, MouseKind::Move);  
            }  
  
            solving_shape.state = State::Solving(timeout);  
        } 
    }  
}
