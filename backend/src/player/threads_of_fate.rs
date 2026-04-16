use std::fmt::Display;  
  
use log::info;  
use opencv::core::Rect;  
  
use super::{  
    Player,  
    timeout::{Lifecycle, Timeout, next_timeout_lifecycle},  
};  
use crate::{  
    bridge::{KeyKind, MouseKind},  
    ecs::Resources,  
    player::{PlayerEntity, next_action},  
};  
  
/// Maximum number of down-arrow scroll cycles to find unravelling.  
const MAX_SCROLL_CYCLES: u32 = 3;  
/// Number of down-arrow presses per scroll cycle.  
const DOWN_PRESSES_PER_CYCLE: u32 = 5;  
/// Maximum interact key presses to end a dialog.  
const MAX_INTERACT_PRESSES: u32 = 20;  
/// Interval (in ticks) between interact key presses.  
const INTERACT_PRESS_INTERVAL: u32 = 8;  
/// Maximum consecutive ask failures before permanently stopping.  
const MAX_ASK_FAIL_COUNT: u32 = 3;  
  
/// Internal state machine for Threads of Fate.  
#[derive(Debug, Clone)]  
enum State {  
    /// Step 1: Find and click bulb.png  
    ClickBulb(Timeout),  
    /// Step 2: Wait for maple_mailbox.png to appear  
    WaitMailbox(Timeout),  
    /// Step 3: Look for threads_of_fate_complete in the mailbox  
    FindComplete(Timeout),  
    /// Step 4: Click threads_of_fate_complete and start interacting  
    InteractComplete(Timeout, u32),  
    /// Step 5: Find unravelling.png (with scrolling)  
    FindUnravelling(Timeout, u32),  
    /// Step 6: Click unravelling.png, wait for fate_character_ui.png  
    ClickUnravelling(Timeout),  
    /// Step 7: Wait for fate_character_ui.png  
    WaitFateCharacterUI(Timeout),  
    /// Step 8: Click fate_character.png  
    ClickFateCharacter(Timeout),  
    /// Step 9: Click ask.png  
    ClickAsk(Timeout, u32),  
    /// Step 10: Press interact key to finish dialog  
    InteractDialog(Timeout, u32),  
    /// Step 11: Wait interval before next cycle (mm:ss)  
    WaitInterval(Timeout),
    /// Terminal state  
    Completing(Timeout, bool),  
}  
  
/// Struct for storing Threads of Fate state data.  
#[derive(Debug, Clone)]  
pub struct ThreadsOfFateState {  
    state: State,  
    /// Total chat count remaining  
    remaining_count: u32,  
    /// Wait interval in ticks between cycles  
    wait_interval_ticks: u32,  
    /// Interact key configured by user  
    interact_key: KeyKind,  
    /// Whether the operation was successful  
    success: bool,  
    /// Consecutive ask failures (ask clicked but no dialog appeared)  
    ask_fail_count: u32,  
    /// Whether we found a complete quest this cycle  
    found_complete: bool,  
}  
  
impl Display for ThreadsOfFateState {  
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {  
        match self.state {  
            State::ClickBulb(_) => write!(f, "ClickBulb"),  
            State::WaitMailbox(_) => write!(f, "WaitMailbox"),  
            State::FindComplete(_) => write!(f, "FindComplete"),  
            State::InteractComplete(_, _) => write!(f, "InteractComplete"),  
            State::FindUnravelling(_, _) => write!(f, "FindUnravelling"),  
            State::ClickUnravelling(_) => write!(f, "ClickUnravelling"),  
            State::WaitFateCharacterUI(_) => write!(f, "WaitFateCharacterUI"),  
            State::ClickFateCharacter(_) => write!(f, "ClickFateCharacter"),  
            State::ClickAsk(_, _) => write!(f, "ClickAsk"),  
            State::InteractDialog(_, _) => write!(f, "InteractDialog"),  
            State::WaitInterval(_) => write!(f, "WaitInterval"),  
            State::Completing(_, _) => write!(f, "Completing"),  
        }  
    }  
}  
  
impl ThreadsOfFateState {  
    pub fn new(count: u32, wait_interval_ticks: u32, interact_key: KeyKind) -> Self {  
        Self {  
            state: State::ClickBulb(Timeout::default()),  
            remaining_count: count,  
            wait_interval_ticks,  
            interact_key,  
            success: false,  
            ask_fail_count: 0,  
            found_complete: false,  
        }  
    }  
}  
  
/// Updates [`Player::ThreadsOfFate`] contextual state.  
pub fn update_threads_of_fate_state(resources: &mut Resources, player: &mut PlayerEntity) {  
    let Player::ThreadsOfFate(mut tof) = player.state.clone() else {  
        panic!("state is not threads of fate")  
    };  
  
    match tof.state.clone() {  
        State::ClickBulb(_) => update_click_bulb(resources, &mut tof),  
        State::WaitMailbox(_) => update_wait_mailbox(resources, &mut tof),  
        State::FindComplete(_) => update_find_complete(resources, &mut tof),  
        State::InteractComplete(_, _) => update_interact_complete(resources, &mut tof),  
        State::FindUnravelling(_, _) => update_find_unravelling(resources, &mut tof),  
        State::ClickUnravelling(_) => update_click_unravelling(resources, &mut tof),  
        State::WaitFateCharacterUI(_) => update_wait_fate_character_ui(resources, &mut tof),  
        State::ClickFateCharacter(_) => update_click_fate_character(resources, &mut tof),  
        State::ClickAsk(_, _) => update_click_ask(resources, &mut tof),  
        State::InteractDialog(_, _) => update_interact_dialog(resources, &mut tof),  
        State::WaitInterval(_) => update_wait_interval(resources, &mut tof),  
        State::Completing(_, _) => update_completing(resources, &mut tof),  
    }  
  
    let player_next_state = if matches!(tof.state, State::Completing(_, true)) {  
        Player::Idle  
    } else {  
        Player::ThreadsOfFate(tof.clone())  
    };  
    let is_terminal = matches!(player_next_state, Player::Idle);  
  
    match next_action(&player.context) {  
        Some(_) => {  
            if is_terminal {  
                player.context.clear_action_completed();  
                if tof.success {  
                    player.context.clear_threads_of_fate_fail_count();  
                } else {  
                    player.context.track_threads_of_fate_fail_count();  
                }  
            }  
            player.state = player_next_state;  
        }  
        None => player.state = Player::Idle,  
    }  
}  
  
/// Step 1: Find bulb.png and click it    
fn update_click_bulb(resources: &mut Resources, tof: &mut ThreadsOfFateState) {    
    let State::ClickBulb(timeout) = tof.state else {    
        panic!("threads of fate state is not click bulb")    
    };    
  
    match next_timeout_lifecycle(timeout, 90) {    
        Lifecycle::Started(timeout) => {    
            tof.state = State::ClickBulb(timeout);    
        }    
Lifecycle::Ended => {  
    info!(target: "backend/player", "threads of fate: mailbox did not appear after clicking bulb");  
    tof.state = State::Completing(Timeout::default(), false);  
}
        Lifecycle::Updated(timeout) => {    
            // 每 10 tick 检查邮箱是否已出现（说明之前的点击成功了）  
if timeout.current % 10 == 0 && resources.detector().detect_tof_maple_mailbox() {  
    info!(target: "backend/player", "threads of fate: mailbox detected, looking for complete");  
    tof.state = State::FindComplete(Timeout::default());  
    return;  
}
            // 每 15 tick 重试检测并点击灯泡  
if timeout.current % 15 == 0  
    && let Ok(bbox) = resources.detector().detect_tof_bulb()  
{  
    let (x, y) = bbox_click_point(bbox);  
    resources.input.send_mouse(x, y, MouseKind::Click);  
}  
            tof.state = State::ClickBulb(timeout);    
        }    
    }    
}
  
/// Step 2: Wait for maple_mailbox.png to appear  
fn update_wait_mailbox(resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::WaitMailbox(timeout) = tof.state else {  
        panic!("threads of fate state is not wait mailbox")  
    };  
  
    match next_timeout_lifecycle(timeout, 60) {  
        Lifecycle::Started(timeout) => {  
            tof.state = State::WaitMailbox(timeout);  
        }  
        Lifecycle::Ended => {  
            info!(target: "backend/player", "threads of fate: mailbox did not appear");  
            tof.state = State::Completing(Timeout::default(), false);  
        }  
        Lifecycle::Updated(timeout) => {  
            if timeout.current % 10 == 0 && resources.detector().detect_tof_maple_mailbox() {  
                tof.state = State::FindComplete(Timeout::default());  
            } else {  
                tof.state = State::WaitMailbox(timeout);  
            }  
        }  
    }  
}  
  
/// Step 3: Look for threads_of_fate_complete    
fn update_find_complete(resources: &mut Resources, tof: &mut ThreadsOfFateState) {    
    let State::FindComplete(timeout) = tof.state else {    
        panic!("threads of fate state is not find complete")    
    };    
  
    match next_timeout_lifecycle(timeout, 30) {    
          Lifecycle::Started(timeout) => {
     // 立即检测 complete
     match resources.detector().detect_tof_complete() {
         Ok(bbox) => {
             let (x, y) = bbox_click_point(bbox);
             resources.input.send_mouse(x, y, MouseKind::Click);
            tof.found_complete = true;
             tof.state = State::InteractComplete(Timeout::default(), 0);
         }
         Err(_) => {
             tof.state = State::FindComplete(timeout);
         }
     }
        }    
        Lifecycle::Ended => {    
            // No complete quest found, look for unravelling    
            tof.found_complete = false;    
            tof.state = State::FindUnravelling(Timeout::default(), 0);    
        }    
        Lifecycle::Updated(timeout) => {    
            // 每 10 tick 重试检测（30 tick 内有 3 次机会）  
            if timeout.current % 10 == 0 {    
                match resources.detector().detect_tof_complete() {    
                    Ok(bbox) => {    
                        let (x, y) = bbox_click_point(bbox);    
                        resources.input.send_mouse(x, y, MouseKind::Click);    
                        tof.found_complete = true;    
                        tof.state = State::InteractComplete(Timeout::default(), 0);    
                    }    
                    Err(_) => {    
                        tof.state = State::FindComplete(timeout);    
                    }    
                }    
            } else {    
                tof.state = State::FindComplete(timeout);    
            }    
        }    
    }    
}
  
/// Step 4: Press interact key multiple times to finish complete quest dialog  
fn update_interact_complete(resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::InteractComplete(timeout, press_count) = tof.state else {  
        panic!("threads of fate state is not interact complete")  
    };  
  
    match next_timeout_lifecycle(timeout, INTERACT_PRESS_INTERVAL) {  
        Lifecycle::Started(timeout) => {  
            resources.input.send_key(tof.interact_key);  
            tof.state = State::InteractComplete(timeout, press_count);  
        }  
        Lifecycle::Ended => {  
            let new_count = press_count + 1;  
            if new_count >= MAX_INTERACT_PRESSES {  
                // Dialog should be closed by now, go back to step 1 (click bulb again)  
                tof.state = State::ClickBulb(Timeout::default());  
            } else {  
                // Check if fate_character.png dialog is still visible  
                // If not visible, dialog ended - go back to click bulb  
                if resources.detector().detect_tof_dialog_visible() {
                    tof.state = State::InteractComplete(Timeout::default(), new_count);  
                } else {  
                    // Dialog ended  
                    tof.state = State::ClickBulb(Timeout::default());  
                }  
            }  
        }  
        Lifecycle::Updated(timeout) => {  
            tof.state = State::InteractComplete(timeout, press_count);  
        }  
    }  
}  
  
/// Step 5: Find unravelling.png (with scrolling)  
fn update_find_unravelling(resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::FindUnravelling(timeout, scroll_cycle) = tof.state else {  
        panic!("threads of fate state is not find unravelling")  
    };  
  
    match next_timeout_lifecycle(timeout, 30) {  
        Lifecycle::Started(timeout) => {  
            // First check if unravelling is visible  
            if let Ok(bbox) = resources.detector().detect_tof_unravelling() {  
                let (x, y) = bbox_click_point(bbox);  
                resources.input.send_mouse(x, y, MouseKind::Click);  
                tof.state = State::ClickUnravelling(Timeout::default());  
                return;  
            }  
            tof.state = State::FindUnravelling(timeout, scroll_cycle);  
        }  
        Lifecycle::Ended => {  
            // Try to find unravelling after scrolling  
            match resources.detector().detect_tof_unravelling() {  
                Ok(bbox) => {  
                    let (x, y) = bbox_click_point(bbox);  
                    resources.input.send_mouse(x, y, MouseKind::Click);  
                    tof.state = State::ClickUnravelling(Timeout::default());  
                }  
                Err(_) => {  
                    let new_cycle = scroll_cycle + 1;  
                    if new_cycle >= MAX_SCROLL_CYCLES {  
                        info!(target: "backend/player", "threads of fate: unravelling not found after {} scroll cycles", MAX_SCROLL_CYCLES);  
                        resources.input.send_key(KeyKind::Esc);  
                        tof.state = State::Completing(Timeout::default(), false);  
                    } else {  
                        for _ in 0..DOWN_PRESSES_PER_CYCLE {  
                            resources.input.send_key(KeyKind::Down);  
                        }  
                        tof.state = State::FindUnravelling(Timeout::default(), new_cycle);  
                    }  
                }  
            }  
        }  
        Lifecycle::Updated(timeout) => {  
            tof.state = State::FindUnravelling(timeout, scroll_cycle);  
        }  
    }  
}
  
/// Step 6: After clicking unravelling, wait briefly  
fn update_click_unravelling(_resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::ClickUnravelling(timeout) = tof.state else {  
        panic!("threads of fate state is not click unravelling")  
    };  
  
    match next_timeout_lifecycle(timeout, 30) {  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            tof.state = State::ClickUnravelling(timeout);  
        }  
        Lifecycle::Ended => {  
            tof.state = State::WaitFateCharacterUI(Timeout::default());  
        }  
    }  
}  
  
/// Step 7: Wait for fate_character_ui.png  
fn update_wait_fate_character_ui(resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::WaitFateCharacterUI(timeout) = tof.state else {  
        panic!("threads of fate state is not wait fate character ui")  
    };  
  
    match next_timeout_lifecycle(timeout, 60) {  
        Lifecycle::Started(timeout) => {  
            tof.state = State::WaitFateCharacterUI(timeout);  
        }  
        Lifecycle::Ended => {  
            info!(target: "backend/player", "threads of fate: fate character UI did not appear");  
            resources.input.send_key(KeyKind::Esc);  
            tof.state = State::Completing(Timeout::default(), false);  
        }  
        Lifecycle::Updated(timeout) => {  
            if timeout.current % 10 == 0 && resources.detector().detect_tof_fate_character_ui() {  
                tof.state = State::ClickFateCharacter(Timeout::default());  
            } else {  
                tof.state = State::WaitFateCharacterUI(timeout);  
            }  
        }  
    }  
}  
  
/// Step 8: Click fate_character.png (user-customizable via localization)    
fn update_click_fate_character(resources: &mut Resources, tof: &mut ThreadsOfFateState) {    
    let State::ClickFateCharacter(timeout) = tof.state else {    
        panic!("threads of fate state is not click fate character")    
    };    
  
    match next_timeout_lifecycle(timeout, 60) {    
        Lifecycle::Started(timeout) => {    
            tof.state = State::ClickFateCharacter(timeout);    
        }    
        Lifecycle::Ended => {    
            info!(target: "backend/player", "threads of fate: failed to find fate character");    
            resources.input.send_key(KeyKind::Esc);    
            tof.state = State::Completing(Timeout::default(), false);    
        }    
        Lifecycle::Updated(timeout) => {    
            // 每 15 tick 重试检测并点击（60 tick 内有 4 次机会）  
            if timeout.current % 15 == 0 {    
                match resources.detector().detect_tof_fate_character() {    
                    Ok(bbox) => {    
                        let (x, y) = bbox_click_point(bbox);    
                        resources.input.send_mouse(x, y, MouseKind::Click);    
                        tof.state = State::ClickAsk(Timeout::default(), 0);    
                    }    
                    Err(_) => {    
                        tof.state = State::ClickFateCharacter(timeout);    
                    }    
                }    
            } else {    
                tof.state = State::ClickFateCharacter(timeout);    
            }    
        }    
    }    
}
  
/// Step 9: Click ask.png    
fn update_click_ask(resources: &mut Resources, tof: &mut ThreadsOfFateState) {    
    let State::ClickAsk(timeout, retry_count) = tof.state else {    
        panic!("threads of fate state is not click ask")    
    };    
  
    match next_timeout_lifecycle(timeout, 30) {    
        Lifecycle::Started(timeout) => {    
            // 首次进入时检测并点击  
            if let Ok(bbox) = resources.detector().detect_tof_ask_button() {    
                let (x, y) = bbox_click_point(bbox);    
                resources.input.send_mouse(x, y, MouseKind::Click);    
            }    
            tof.state = State::ClickAsk(timeout, retry_count);    
        }    
        Lifecycle::Ended => {    
            // 检查对话框是否出现（说明 ask 点击成功了）  
            if resources.detector().detect_tof_dialog_visible() {    
                tof.state = State::InteractDialog(Timeout::default(), 0);    
                return;    
            }    
            // 对话框未出现，重试  
            let new_retry = retry_count + 1;    
            if new_retry >= MAX_ASK_FAIL_COUNT {    
                info!(target: "backend/player", "threads of fate: ask failed {} times, stopping", MAX_ASK_FAIL_COUNT);    
                tof.ask_fail_count = new_retry;    
                resources.input.send_key(KeyKind::Esc);    
                tof.state = State::Completing(Timeout::default(), false);    
            } else {    
                tof.state = State::ClickAsk(Timeout::default(), new_retry);    
            }    
        }    
        Lifecycle::Updated(timeout) => {    
            // 每 10 tick 检查对话框是否已出现  
            if timeout.current % 10 == 0 && resources.detector().detect_tof_dialog_visible() {   
                tof.state = State::InteractDialog(Timeout::default(), 0);    
                return;    
            }    
            // 每 15 tick 重试点击 ask 按钮  
            if timeout.current % 15 == 0  
                && let Ok(bbox) = resources.detector().detect_tof_ask_button()  
            {  
                let (x, y) = bbox_click_point(bbox);  
                resources.input.send_mouse(x, y, MouseKind::Click);  
            }
            tof.state = State::ClickAsk(timeout, retry_count);    
        }    
     } 
} 
  
/// Step 10: Press interact key to finish dialog  
fn update_interact_dialog(resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::InteractDialog(timeout, press_count) = tof.state else {  
        panic!("threads of fate state is not interact dialog")  
    };  

    match next_timeout_lifecycle(timeout, INTERACT_PRESS_INTERVAL) {  
        Lifecycle::Started(timeout) => {  
            resources.input.send_key(tof.interact_key);  
            tof.state = State::InteractDialog(timeout, press_count);  
        }  
        Lifecycle::Ended => {  
            let new_count = press_count + 1;  
            if new_count >= MAX_INTERACT_PRESSES {  
                // Assume dialog ended  
                tof.remaining_count = tof.remaining_count.saturating_sub(1);  
                if tof.remaining_count == 0 {  
                    tof.success = true;  
                    tof.state = State::Completing(Timeout::default(), false);  
                } else {  
                    tof.state = State::WaitInterval(Timeout::default());  
                }  
            } else {  
                // Check if dialog is still visible  
                // During ThreadsOfFate, next.png detection should press interact instead of ESC  
if resources.detector().detect_tof_dialog_visible() {    
    resources.input.send_key(tof.interact_key);    
    tof.state = State::InteractDialog(Timeout::default(), new_count);    
} else if resources.detector().detect_tof_fate_character_dialog() {    
    tof.state = State::InteractDialog(Timeout::default(), new_count);    
} else {    
    // Dialog ended    
    tof.remaining_count = tof.remaining_count.saturating_sub(1);    
    if tof.remaining_count == 0 {    
        tof.success = true;    
        tof.state = State::Completing(Timeout::default(), false);    
    } else {    
        tof.state = State::WaitInterval(Timeout::default());    
    }    
}
            }  
        }  
        Lifecycle::Updated(timeout) => {  
            tof.state = State::InteractDialog(timeout, press_count);  
        }  
    }  
}  
  
/// Step 11: Wait interval before next cycle  
fn update_wait_interval(_resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::WaitInterval(timeout) = tof.state else {  
        panic!("threads of fate state is not wait interval")  
    };  
  
    match next_timeout_lifecycle(timeout, tof.wait_interval_ticks.max(1)) {  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            tof.state = State::WaitInterval(timeout);  
        }  
        Lifecycle::Ended => {  
            // Go back to step 1: click bulb  
            tof.state = State::ClickBulb(Timeout::default());  
        }  
    }  
}  
/// Terminal state  
fn update_completing(_resources: &mut Resources, tof: &mut ThreadsOfFateState) {  
    let State::Completing(timeout, completed) = tof.state else {  
        panic!("threads of fate state is not completing")  
    };  
  
    match next_timeout_lifecycle(timeout, 20) {  
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {  
            tof.state = State::Completing(timeout, completed);  
        }  
        Lifecycle::Ended => {  
            tof.state = State::Completing(timeout, true);  
        }  
    }  
}  
  
/// Computes the click point (center) of a bounding box.  
#[inline]  
fn bbox_click_point(bbox: Rect) -> (i32, i32) {  
    (bbox.x + bbox.width / 2, bbox.y + bbox.height / 2)  
}
