use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use super::{KeyBinding, LinkKeyBinding, deserialize_with_ok_or_default};

/// A persistent model representing a user-provided action for the bot to perform.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize, EnumIter, Display, EnumString)]
pub enum Action {
    /// An action that moves to a specific location.
    Move(ActionMove),
    /// An action that uses a specific key with or without a location.
    Key(ActionKey),
}

impl Action {
    pub fn condition(&self) -> ActionCondition {
        match self {
            Action::Move(action) => action.condition,
            Action::Key(action) => action.condition,
        }
    }

    pub fn with_condition(&self, condition: ActionCondition) -> Action {
        match self {
            Action::Move(action) => Action::Move(ActionMove {
                condition,
                ..*action
            }),
            Action::Key(action) => Action::Key(ActionKey {
                condition,
                ..*action
            }),
        }
    }
}

/// A persistent model for the [`Action::Move`] action.
#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct ActionMove {
    pub position: Position,
    pub condition: ActionCondition,
    pub wait_after_move_millis: u64,
}

/// A persistent model for the [`Action::Key`] action.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ActionKey {
    pub key: KeyBinding,
    #[serde(default)]
    pub key_hold_millis: u64,
    #[serde(default)]
    pub key_hold_buffered_to_wait_after: bool,
    #[serde(default, deserialize_with = "deserialize_with_ok_or_default")]
    pub link_key: LinkKeyBinding,
    #[serde(default = "count_default")]
    pub count: u32,
    pub position: Option<Position>,
    pub condition: ActionCondition,
    pub direction: ActionKeyDirection,
    pub with: ActionKeyWith,
    pub wait_before_use_millis: u64,
    pub wait_before_use_millis_random_range: u64,
    pub wait_after_use_millis: u64,
    pub wait_after_use_millis_random_range: u64,
    #[serde(default, deserialize_with = "deserialize_with_ok_or_default")]
    pub wait_after_buffered: WaitAfterBuffered,
    pub queue_to_front: Option<bool>,
}

impl Default for ActionKey {
    fn default() -> Self {
        Self {
            key: KeyBinding::default(),
            key_hold_millis: 0,
            key_hold_buffered_to_wait_after: false,
            link_key: LinkKeyBinding::None,
            count: count_default(),
            position: None,
            condition: ActionCondition::default(),
            direction: ActionKeyDirection::default(),
            with: ActionKeyWith::default(),
            wait_before_use_millis: 0,
            wait_before_use_millis_random_range: 0,
            wait_after_use_millis: 0,
            wait_after_use_millis_random_range: 0,
            wait_after_buffered: WaitAfterBuffered::None,
            queue_to_front: None,
        }
    }
}

fn count_default() -> u32 {
    1
}

#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub x_random_range: i32,
    pub y: i32,
    pub allow_adjusting: bool,
}

#[derive(
    Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize, EnumIter, Display, EnumString,
)]
pub enum ActionCondition {
    #[default]
    Any,
    EveryMillis(u64),
    ErdaShowerOffCooldown,
    Linked,
}

#[derive(
    Clone, Copy, Default, PartialEq, Debug, Serialize, Deserialize, EnumIter, Display, EnumString,
)]
pub enum ActionKeyWith {
    #[default]
    Any,
    Stationary,
    DoubleJump,
}

#[derive(
    Clone, Copy, PartialEq, Default, Debug, Serialize, Deserialize, EnumIter, Display, EnumString,
)]
pub enum ActionKeyDirection {
    #[default]
    Any,
    Left,
    Right,
}

#[derive(
    Clone, Copy, Display, EnumString, EnumIter, PartialEq, Debug, Serialize, Deserialize, Default,
)]
pub enum WaitAfterBuffered {
    #[default]
    None,
    Interruptible,
    Uninterruptible,
}

/// A persistent model representing a user-provided [`Action`] that is fixed per character.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct ActionConfiguration {
    pub key: KeyBinding,
    #[serde(default)]
    pub key_hold_millis: u64,
    #[serde(default)]
    pub key_hold_buffered_to_wait_after: bool,
    #[serde(default, deserialize_with = "deserialize_with_ok_or_default")]
    pub link_key: LinkKeyBinding,
    pub count: u32,
    pub condition: ActionConfigurationCondition,
    pub with: ActionKeyWith,
    pub wait_before_millis: u64,
    pub wait_before_millis_random_range: u64,
    pub wait_after_millis: u64,
    pub wait_after_millis_random_range: u64,
    #[serde(default, deserialize_with = "deserialize_with_ok_or_default")]
    pub wait_after_buffered: WaitAfterBuffered,
    pub enabled: bool,
}

impl Default for ActionConfiguration {
    fn default() -> Self {
        // Template for a buff
        Self {
            key: KeyBinding::default(),
            key_hold_millis: 0,
            key_hold_buffered_to_wait_after: false,
            link_key: LinkKeyBinding::None,
            count: count_default(),
            condition: ActionConfigurationCondition::default(),
            with: ActionKeyWith::Stationary,
            wait_before_millis: 500,
            wait_before_millis_random_range: 0,
            wait_after_millis: 500,
            wait_after_millis_random_range: 0,
            wait_after_buffered: WaitAfterBuffered::None,
            enabled: false,
        }
    }
}

impl From<ActionConfiguration> for Action {
    fn from(value: ActionConfiguration) -> Self {
        Self::Key(ActionKey {
            key: value.key,
            key_hold_millis: value.key_hold_millis,
            key_hold_buffered_to_wait_after: value.key_hold_buffered_to_wait_after,
            link_key: value.link_key,
            count: value.count,
            position: None,
            condition: match value.condition {
                ActionConfigurationCondition::EveryMillis(millis) => {
                    ActionCondition::EveryMillis(millis)
                }
                ActionConfigurationCondition::Linked => ActionCondition::Linked,
            },
            direction: ActionKeyDirection::Any,
            with: value.with,
            queue_to_front: Some(true),
            wait_before_use_millis: value.wait_before_millis,
            wait_before_use_millis_random_range: value.wait_before_millis_random_range,
            wait_after_use_millis: value.wait_after_millis,
            wait_after_use_millis_random_range: value.wait_after_millis_random_range,
            wait_after_buffered: value.wait_after_buffered,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize, EnumIter, Display, EnumString)]
pub enum ActionConfigurationCondition {
    EveryMillis(u64),
    Linked,
}

impl Default for ActionConfigurationCondition {
    fn default() -> Self {
        ActionConfigurationCondition::EveryMillis(180000)
    }
}
