use std::collections::HashMap;

use opencv::core::Rect;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use super::{Action, MobbingKey, deserialize_with_ok_or_default, impl_identifiable};
use crate::pathing;

/// A persistent model representing a map-related data.
#[derive(PartialEq, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Map {
    #[serde(skip_serializing)]
    pub id: Option<i64>,
    pub name: String,
    pub width: i32,
    pub height: i32,
    #[serde(default, deserialize_with = "deserialize_with_ok_or_default")]
    pub rotation_mode: RotationMode,
    #[serde(default)]
    pub rotation_ping_pong_bound: Bound,
    #[serde(default)]
    pub rotation_auto_mob_bound: Bound,
    #[serde(default)]
    pub rotation_mobbing_key: MobbingKey,
    pub platforms: Vec<Platform>,
    pub rune_platforms_pathing: bool,
    pub rune_platforms_pathing_up_jump_only: bool,
    pub auto_mob_platforms_pathing: bool,
    pub auto_mob_platforms_pathing_up_jump_only: bool,
    #[serde(default)]
    pub auto_mob_use_key_when_pathing: bool,
    #[serde(default)]
    pub auto_mob_use_key_when_pathing_update_millis: u64,
    pub actions_any_reset_on_erda_condition: bool,
    pub actions: HashMap<String, Vec<Action>>,
}

impl_identifiable!(Map);

#[derive(Clone, Copy, PartialEq, Default, Debug, Serialize, Deserialize)]
pub struct Bound {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// TODO: Should be part of auto-mobbing or ping-pong logics, not here
impl From<Bound> for Rect {
    fn from(value: Bound) -> Self {
        Self::new(value.x, value.y, value.width, value.height)
    }
}

impl From<Rect> for Bound {
    fn from(value: Rect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
        }
    }
}

#[derive(
    Clone, Copy, PartialEq, Default, Debug, Serialize, Deserialize, EnumIter, Display, EnumString,
)]
pub enum RotationMode {
    StartToEnd,
    #[default]
    StartToEndThenReverse,
    AutoMobbing,
    PingPong,
}

#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Platform {
    pub x_start: i32,
    pub x_end: i32,
    pub y: i32,
}

// TODO: Should be part of pathing logics, not here
impl From<Platform> for pathing::Platform {
    fn from(value: Platform) -> Self {
        Self::new(value.x_start..value.x_end, value.y)
    }
}
