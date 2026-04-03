use serde::{Deserialize, Deserializer};
use serde_json::Value;

mod actions;
mod character;
mod keys;
mod localization;
mod map;
mod seeds;
mod settings;

pub use actions::*;
pub use character::*;
pub use keys::*;
pub use localization::*;
pub use map::*;
pub use seeds::*;
pub use settings::*;

pub trait Identifiable {
    fn id(&self) -> Option<i64>;

    fn set_id(&mut self, id: i64);
}

macro_rules! impl_identifiable {
    ($type:ty) => {
        impl $crate::models::Identifiable for $type {
            fn id(&self) -> Option<i64> {
                self.id
            }

            fn set_id(&mut self, id: i64) {
                self.id = Some(id);
            }
        }
    };
}

use impl_identifiable;

fn deserialize_with_ok_or_default<'a, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'a> + Default,
    D: Deserializer<'a>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}
