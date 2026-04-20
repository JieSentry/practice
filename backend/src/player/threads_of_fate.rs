use std::fmt::Display;

use log::info;
use opencv::core::{Point, Rect};

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
/// Interval (in ticks) between interact key presses.
const INTERACT_PRESS_INTERVAL: u32 = 8;
/// Maximum consecutive ask failures before permanently stopping.
const MAX_ASK_FAIL_COUNT: u32 = 2;
/// Maximum consecutive "not visible" checks before concluding dialog has truly ended.
const MAX_DIALOG_GRACE_CHECKS: u32 = 3;
/// Maximum interact key presses before forcing dialog end.
const MAX_INTERACT_PRESS_COUNT: u32 = 11;

/// Internal state machine for Threads of Fate.
#[derive(Debug, Clone)]
enum State {
    /// Step 1: Find and click bulb.png, wait for maple_mailbox.png
    /// (timeout, interact_press_count)
    ClickBulb(Timeout, u32),
    /// Step 2: Look for threads_of_fate_complete in the mailbox (only once per cycle)
    FindComplete(Timeout),
    /// Step 3: Click threads_of_fate_complete and start interacting
    /// (timeout, press_count, miss_count)
    InteractComplete(Timeout, u32, u32),
    /// Step 4: Find unravelling.png (with scrolling)
    FindUnravelling(Timeout, u32),
    /// Step 5: Click unravelling.png, wait for fate_character_ui.png
    ClickUnravelling(Timeout),
    /// Step 6: Wait for fate_character_ui.png
    WaitFateCharacterUI(Timeout),
    /// Step 7: Click fate_character.png
    ClickFateCharacter(Timeout),
    /// Step 8: Click ask.png (max 2 attempts, then complete if no dialog)
    /// (timeout, retry_count, ask_clicked)
    ClickAsk(Timeout, u32, bool),
    /// Step 9: Press interact key to finish dialog
    /// (timeout, press_count, miss_count)
    InteractDialog(Timeout, u32, u32),
}

/// Struct for storing Threads of Fate state data.
#[derive(Debug, Clone)]
pub struct ThreadsOfFateState {
    state: State,
    /// Total number of complete quests to execute (target count)
    target_complete_count: u32,
    /// Interact key configured by user
    interact_key: KeyKind,
    /// Mouse rest point for avoiding UI overlap
    mouse_rest: Point,
    /// Whether the operation was successful
    success: bool,
    /// Whether we found a complete quest this cycle
    found_complete: bool,
    /// Whether complete quest has been used this cycle (prevents infinite loop)
    complete_used: bool,
    /// Number of complete quests executed so far
    complete_executed_count: u32,
    /// Whether to permanently stop (reached target or failed condition)
    permanently_stopped: bool,
    /// Whether the task is completed and should return to Idle
    completed: bool,
}

impl Display for ThreadsOfFateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.state {
            State::ClickBulb(_, _) => write!(f, "ClickBulb"),
            State::FindComplete(_) => write!(f, "FindComplete"),
            State::InteractComplete(_, _, _) => write!(f, "InteractComplete"),
            State::FindUnravelling(_, _) => write!(f, "FindUnravelling"),
            State::ClickUnravelling(_) => write!(f, "ClickUnravelling"),
            State::WaitFateCharacterUI(_) => write!(f, "WaitFateCharacterUI"),
            State::ClickFateCharacter(_) => write!(f, "ClickFateCharacter"),
            State::ClickAsk(_, _, _) => write!(f, "ClickAsk"),
            State::InteractDialog(_, _, _) => write!(f, "InteractDialog"),
        }
    }
}

impl ThreadsOfFateState {
    pub fn new(target_count: u32, _wait_interval_ticks: u32, interact_key: KeyKind) -> Self {
        Self {
            state: State::ClickBulb(Timeout::default(), 0),
            target_complete_count: target_count,
            interact_key,
            mouse_rest: Point::new(1100, 550),
            success: false,
            found_complete: false,
            complete_used: false,
            complete_executed_count: 0,
            permanently_stopped: false,
            completed: false,
        }
    }
}

/// Updates [`Player::ThreadsOfFate`] contextual state.
pub fn update_threads_of_fate_state(resources: &mut Resources, player: &mut PlayerEntity) {
    let Player::ThreadsOfFate(mut tof) = player.state.clone() else {
        panic!("state is not threads of fate")
    };

    match tof.state.clone() {
        State::ClickBulb(_, _) => update_click_bulb(resources, &mut tof),
        State::FindComplete(_) => update_find_complete(resources, &mut tof),
        State::InteractComplete(_, _, _) => update_interact_complete(resources, &mut tof),
        State::FindUnravelling(_, _) => update_find_unravelling(resources, &mut tof),
        State::ClickUnravelling(_) => update_click_unravelling(resources, &mut tof),
        State::WaitFateCharacterUI(_) => update_wait_fate_character_ui(resources, &mut tof),
        State::ClickFateCharacter(_) => update_click_fate_character(resources, &mut tof),
        State::ClickAsk(_, _, _) => update_click_ask(resources, &mut tof),
        State::InteractDialog(_, _, _) => update_interact_dialog(resources, &mut tof),
    }

    let player_next_state = if tof.completed {
        Player::Idle
    } else {
        Player::ThreadsOfFate(tof.clone())
    };
    let is_terminal = matches!(player_next_state, Player::Idle);

    match next_action(&player.context) {
        Some(_) => {
            if is_terminal {
                player.context.clear_action_completed();
                // Update permanently stopped flag in context if needed
                if tof.permanently_stopped {
                    player.context.set_threads_of_fate_permanently_stopped();
                }
                if tof.success {
                    // Success: clear fail count
                    player.context.clear_threads_of_fate_fail_count();
                } else {
                    // Any failure: track fail count
                    // Two consecutive failures (any type) will trigger permanent stop
                    player.context.track_threads_of_fate_fail_count();
                    if player.context.is_threads_of_fate_fail_count_limit_reached() {
                        player.context.set_threads_of_fate_permanently_stopped();
                    }
                }
            }
            player.state = player_next_state;
        }
        None => player.state = Player::Idle,
    }
}

/// Step 1: Find bulb.png and click it
fn update_click_bulb(resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::ClickBulb(timeout, press_count) = tof.state else {
        panic!("threads of fate state is not click bulb")
    };

    match next_timeout_lifecycle(timeout, 30) {
        Lifecycle::Started(timeout) => {
            resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
            tof.state = State::ClickBulb(timeout, press_count);
        }
        Lifecycle::Ended => {
            info!(target: "backend/player", "threads of fate: mailbox did not appear after clicking bulb");
            tof.completed = true;
        }
        Lifecycle::Updated(timeout) => {
            // Check for dialog elements first (only once at the beginning)
            if press_count < 1 && resources.detector().detect_tof_dialog_visible() {
                resources.input.send_key(tof.interact_key);
                tof.state = State::ClickBulb(timeout, press_count + 1);
                return;
            }
            // Check mailbox every 10 ticks
            if timeout.current % 10 == 0 && resources.detector().detect_tof_maple_mailbox() {
                info!(target: "backend/player", "threads of fate: mailbox detected, looking for complete");
                tof.state = State::FindComplete(Timeout::default());
                return;
            }
            // Click bulb when mailbox not detected
            if timeout.current >= 5 && timeout.current % 5 == 0
                && let Ok(bbox) = resources.detector().detect_tof_bulb()
            {
                let (x, y) = bbox_click_point(bbox);
                resources.input.send_mouse(x, y, MouseKind::Click);
                resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
            }
            tof.state = State::ClickBulb(timeout, press_count);
        }
    }
}

/// Step 2: Look for threads_of_fate_complete (only once per cycle)
fn update_find_complete(resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::FindComplete(timeout) = tof.state else {
        panic!("threads of fate state is not find complete")
    };

    // If complete was already used this cycle, skip directly to unravelling
    if tof.complete_used {
        info!(target: "backend/player", "threads of fate: complete already used this cycle, skipping to unravelling");
        tof.found_complete = false;
        tof.state = State::FindUnravelling(Timeout::default(), 0);
        return;
    }

    match next_timeout_lifecycle(timeout, 30) {
        Lifecycle::Started(timeout) => {
            // Check if complete is available
            match resources.detector().detect_tof_complete() {
                Ok(bbox) => {
                    let (x, y) = bbox_click_point(bbox);
                    resources.input.send_mouse(x, y, MouseKind::Click);
                    resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
                    tof.found_complete = true;
                    tof.complete_used = true; // Mark complete as used for this cycle
                    tof.state = State::InteractComplete(Timeout::default(), 0, 0);
                }
                Err(_) => {
                    tof.state = State::FindComplete(timeout);
                }
            }
        }
        Lifecycle::Ended => {
            // No complete quest found, look for unravelling
            tof.found_complete = false;
            tof.complete_used = true; // Mark complete as used even if not found
            tof.state = State::FindUnravelling(Timeout::default(), 0);
        }
        Lifecycle::Updated(timeout) => {
            // Retry detection every 10 ticks (3 chances within 30 ticks)
            if timeout.current % 10 == 0 {
                match resources.detector().detect_tof_complete() {
                    Ok(bbox) => {
                        let (x, y) = bbox_click_point(bbox);
                        resources.input.send_mouse(x, y, MouseKind::Click);
                        resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
                        tof.found_complete = true;
                        tof.complete_used = true; // Mark complete as used for this cycle
                        tof.state = State::InteractComplete(Timeout::default(), 0, 0);
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

/// Step 3: Press interact key to finish complete quest dialog
fn update_interact_complete(resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::InteractComplete(timeout, press_count, miss_count) = tof.state else {
        panic!("threads of fate state is not interact complete")
    };

    match next_timeout_lifecycle(timeout, INTERACT_PRESS_INTERVAL / 2) {
        Lifecycle::Started(timeout) => {
            // Only press interact if not in grace period (miss_count == 0)
            if miss_count == 0 {
                resources.input.send_key(tof.interact_key);
            }
            tof.state = State::InteractComplete(timeout, press_count, miss_count);
        }
        Lifecycle::Ended => {
            let new_count = press_count + 1;
            if press_count >= 4 {
                // Force end after 4 presses regardless of dialog state (prevent infinite loop)
                info!(target: "backend/player", "threads of fate: complete dialog press count reached limit (4), ending");
                tof.complete_executed_count += 1;
                tof.complete_used = true;
                tof.state = State::ClickBulb(Timeout::default(), 0);
            } else if resources.detector().detect_tof_dialog_visible() {
                // Dialog still visible, press interact and reset miss_count
                resources.input.send_key(tof.interact_key);
                tof.state = State::InteractComplete(Timeout::default(), new_count, 0);
            } else if miss_count < MAX_DIALOG_GRACE_CHECKS {
        // Grace period: dialog might be transitioning between pages
        // Don't press interact, just wait and re-check
        tof.state = State::InteractComplete(Timeout::default(), new_count, miss_count + 1);
    } else {
                tof.complete_executed_count += 1;
                info!(target: "backend/player", "threads of fate: complete quest finished ({}/{})", tof.complete_executed_count, tof.target_complete_count);
                tof.complete_used = true; // Mark as used to skip FindComplete next
                tof.state = State::ClickBulb(Timeout::default(), 0);
            }
        }
        Lifecycle::Updated(timeout) => {
            tof.state = State::InteractComplete(timeout, press_count, miss_count);
        }
    }
}

/// Step 4: Find unravelling.png (with scrolling)
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
                resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
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
                    resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
                    tof.state = State::ClickUnravelling(Timeout::default());
                }
                Err(_) => {
                    let new_cycle = scroll_cycle + 1;
                    if new_cycle >= MAX_SCROLL_CYCLES {
                        info!(target: "backend/player", "threads of fate: unravelling not found, found_complete={}", tof.found_complete);
                        tof.completed = true;
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

/// Step 5: After clicking unravelling, wait briefly
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

/// Step 6: Wait for fate_character_ui.png
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
            // Task failed, go directly to completing
            tof.completed = true;
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

/// Step 7: Click fate_character.png (user-customizable via localization)
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
            // Task failed, go directly to completing
            tof.completed = true;
        }
        Lifecycle::Updated(timeout) => {
            if timeout.current % 15 == 0 {
                match resources.detector().detect_tof_fate_character() {
                    Ok(bbox) => {
                        let (x, y) = bbox_click_point(bbox);
                        resources.input.send_mouse(x, y, MouseKind::Click);
                        resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
                        tof.state = State::ClickAsk(Timeout::default(), 0, false);
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

/// Step 8: Click ask.png (max 2 attempts with shorter interval, then terminate)
fn update_click_ask(resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::ClickAsk(timeout, retry_count, ask_clicked) = tof.state else {
        panic!("threads of fate state is not click ask")
    };

    // Shorter timeout: 15 ticks (~0.5s) instead of 30
    match next_timeout_lifecycle(timeout, 10) {
        Lifecycle::Started(timeout) => {
            // Delay first click to tick 2
            tof.state = State::ClickAsk(timeout, retry_count, ask_clicked);
        }
        Lifecycle::Ended => {
            // Only check dialog if we have clicked ask
            if ask_clicked && resources.detector().detect_tof_dialog_visible() {
                tof.state = State::InteractDialog(Timeout::default(), 0, 0);
                return;
            }
            // Dialog did not appear, retry
            let new_retry = retry_count + 1;
            if new_retry >= MAX_ASK_FAIL_COUNT {
                // Ask failed MAX_ASK_FAIL_COUNT times in this cycle
                // Mark as failed and end this cycle
                info!(target: "backend/player", "threads of fate: ask failed {} times in this cycle", MAX_ASK_FAIL_COUNT);
                tof.completed = true;
            } else {
                tof.state = State::ClickAsk(Timeout::default(), new_retry, false);
            }
        }
        Lifecycle::Updated(timeout) => {
            // Only check dialog if we have clicked ask
            if ask_clicked && resources.detector().detect_tof_dialog_visible() {
                tof.state = State::InteractDialog(Timeout::default(), 0, 0);
                return;
            }
            // Click ask button at tick 2 (first click) and tick 8 (second click)
            if (timeout.current == 2 || timeout.current % 8 == 0)
                && let Ok(bbox) = resources.detector().detect_tof_ask_button()
            {
                let (x, y) = bbox_click_point(bbox);
                resources.input.send_mouse(x, y, MouseKind::Click);
                resources.input.send_mouse(tof.mouse_rest.x, tof.mouse_rest.y, MouseKind::Move);
                // Mark ask as clicked
                tof.state = State::ClickAsk(timeout, retry_count, true);
            } else {
                tof.state = State::ClickAsk(timeout, retry_count, ask_clicked);
            }
        }
    }
}

/// Step 9: Press interact key to finish dialog
/// Detects tof_next, tof_yes, tof_blue_position and presses interact key
fn update_interact_dialog(resources: &mut Resources, tof: &mut ThreadsOfFateState) {
    let State::InteractDialog(timeout, press_count, miss_count) = tof.state else {
        panic!("threads of fate state is not interact dialog")
    };

    match next_timeout_lifecycle(timeout, INTERACT_PRESS_INTERVAL) {
        Lifecycle::Started(timeout) => {
            if miss_count == 0 && resources.detector().detect_tof_dialog_visible() {
                resources.input.send_key(tof.interact_key);
            }
            tof.state = State::InteractDialog(timeout, press_count, miss_count);
        }
        Lifecycle::Ended => {
            let new_count = press_count + 1;
            if resources.detector().detect_tof_dialog_visible() {
                // Check press count limit even when dialog is visible
                if press_count >= MAX_INTERACT_PRESS_COUNT {
                    info!(target: "backend/player", "threads of fate: interact press count reached limit ({}), forcing end", MAX_INTERACT_PRESS_COUNT);
                    tof.success = true;
                    tof.completed = true;
                } else {
                    resources.input.send_key(tof.interact_key);
                    tof.state = State::InteractDialog(Timeout::default(), new_count, 0);
                }
            } else if press_count >= MAX_INTERACT_PRESS_COUNT {
                info!(target: "backend/player", "threads of fate: interact press count reached limit ({}), ending dialog", MAX_INTERACT_PRESS_COUNT);
                tof.success = true;
                tof.completed = true;
            } else if miss_count >= MAX_DIALOG_GRACE_CHECKS {
                info!(target: "backend/player", "threads of fate: dialog ended after grace period");
                tof.success = true;
                tof.completed = true;
            } else {
                tof.state = State::InteractDialog(Timeout::default(), new_count, miss_count + 1);
            }
        }
        Lifecycle::Updated(timeout) => {
            tof.state = State::InteractDialog(timeout, press_count, miss_count);
        }
    }
}

/// Computes the click point (center) of a bounding box.
#[inline]
fn bbox_click_point(bbox: Rect) -> (i32, i32) {
    (bbox.x + bbox.width / 2, bbox.y + bbox.height / 2)
}
