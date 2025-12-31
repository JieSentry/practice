use std::sync::atomic::{AtomicU64, Ordering};

use opencv::core::Rect;

use super::kalman_filter::KalmanXYAH;
use crate::tracker::tlwh_to_xyah;

static TRACK_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackState {
    Tracked,
    Lost,
}

/// [STrack] implementation by GPT-5.
///
/// [STrack]: https://github.com/ultralytics/ultralytics/blob/004d9730060e560c86ad79aaa1ab97167443be25/ultralytics/trackers/byte_tracker.py#L16
#[derive(Debug, Clone)]
pub struct STrack {
    pub(super) track_id: u64,
    tracklet_len: usize,
    pub(super) frame_id: u64,
    pub(super) state: TrackState,
    pub(super) kalman: KalmanXYAH,
    pub(super) tlwh: [f32; 4],
}

impl STrack {
    pub fn new(bbox: Rect) -> Self {
        let tlwh = [
            bbox.x as f32,
            bbox.y as f32,
            bbox.width as f32,
            bbox.height as f32,
        ];

        Self {
            track_id: 0,
            tracklet_len: 0,
            frame_id: 0,
            state: TrackState::Lost,
            kalman: KalmanXYAH::new(),
            tlwh,
        }
    }

    pub fn track_id(&self) -> u64 {
        self.track_id
    }

    pub fn tracklet_len(&self) -> usize {
        self.tracklet_len
    }

    pub(super) fn activate(&mut self, frame_id: u64) {
        self.track_id = TRACK_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.tracklet_len = 0;
        self.frame_id = frame_id;
        self.state = TrackState::Tracked;

        let meas = tlwh_to_xyah(self.tlwh);
        self.kalman.initiate(meas);
    }

    pub(super) fn reactivate(&mut self, tlwh: [f32; 4], frame_id: u64) {
        self.update(tlwh, frame_id);
        self.tracklet_len = 0;
    }

    pub(super) fn predict(&mut self) {
        if self.state != TrackState::Tracked {
            self.kalman.mean[6] = 0.0;
            self.kalman.mean[7] = 0.0;
        }
        self.kalman.predict();
    }

    pub(super) fn update(&mut self, tlwh: [f32; 4], frame_id: u64) {
        self.frame_id = frame_id;
        self.tracklet_len += 1;
        self.tlwh = tlwh;
        self.state = TrackState::Tracked;

        let meas = tlwh_to_xyah(self.tlwh);
        self.kalman.update(meas);
    }

    pub(super) fn mark_lost(&mut self) {
        self.state = TrackState::Lost;
    }

    pub fn rect(&self) -> Rect {
        Rect::new(
            self.tlwh[0] as i32,
            self.tlwh[1] as i32,
            self.tlwh[2] as i32,
            self.tlwh[3] as i32,
        )
    }

    pub(super) fn kalman_tlwh(&self) -> [f32; 4] {
        self.kalman.tlwh()
    }

    pub fn kalman_rect(&self) -> Rect {
        let slice = self.kalman_tlwh();
        Rect::new(
            slice[0] as i32,
            slice[1] as i32,
            slice[2] as i32,
            slice[3] as i32,
        )
    }

    pub fn kalman_velocity(&self) -> (f32, f32) {
        let vx = self.kalman.mean[4];
        let vy = self.kalman.mean[5];
        (vx, vy)
    }
}
