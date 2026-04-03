use serde::{Deserialize, Serialize};

use super::impl_identifiable;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seeds {
    #[serde(skip_serializing, default)]
    pub id: Option<i64>,
    #[serde(alias = "seed")]
    pub rng_seed: [u8; 32],
    #[serde(default = "perlin_seed_default")]
    pub perlin_seed: u32,
}

impl_identifiable!(Seeds);

impl Default for Seeds {
    fn default() -> Self {
        Self {
            id: None,
            rng_seed: rand::random(),
            perlin_seed: perlin_seed_default(),
        }
    }
}

fn perlin_seed_default() -> u32 {
    rand::random()
}
