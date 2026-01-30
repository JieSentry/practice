use nalgebra::Vector4;
use opencv::core::Rect;

mod bytetracker;
mod kalman_filter;
mod strack;

pub use bytetracker::{ByteTracker, IouGating};
pub use strack::STrack;

#[derive(Clone, Debug)]
pub struct Detection {
    bbox: Rect,
}

impl Detection {
    pub fn new(bbox: Rect) -> Self {
        Self { bbox }
    }
}

fn tlwh_to_xyah(tlwh: [f32; 4]) -> Vector4<f32> {
    let cx = tlwh[0] + tlwh[2] / 2.0;
    let cy = tlwh[1] + tlwh[3] / 2.0;
    let a = tlwh[2] / tlwh[3];
    let h = tlwh[3];
    Vector4::new(cx, cy, a, h)
}
