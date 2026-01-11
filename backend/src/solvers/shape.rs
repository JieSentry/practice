use std::ops::Div;

use log::debug;
use opencv::core::{Point, Point_, Point2d, Rect};

use crate::{
    detect::Detector,
    tracker::{ByteTracker, Detection, STrack},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct TransparentShapeSolver {
    current_track_id: Option<u64>,
    candidate_track_id: Option<u64>,
    candidate_track_count: u32,
    last_cursor: Option<Point>,
    last_velocity: Option<Point2d>,
    bg_direction: Point2d,
    #[cfg(debug_assertions)]
    debugging: bool,
}

impl TransparentShapeSolver {
    #[cfg(debug_assertions)]
    pub fn debug() -> Self {
        Self {
            debugging: true,
            ..Default::default()
        }
    }

    pub fn solve(
        &mut self,
        detector: &dyn Detector,
        tracker: &mut ByteTracker,
        region: Rect,
    ) -> Point {
        let shapes = detector.detect_transparent_shapes(region);
        let tracks = tracker.update(shapes.into_iter().map(Detection::new).collect());

        self.update_initial_track_if_needed(region, &tracks);
        self.update_background_direction(&tracks);

        match self.update_and_find_best_track(&tracks) {
            Some(track) => {
                let next_cursor = predicted_center(track);
                if self.current_track_id != Some(track.track_id()) {
                    debug!(target: "player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());
                }
                self.current_track_id = Some(track.track_id());
                self.last_cursor = Some(next_cursor);
                self.last_velocity = Some(track_velocity(track));

                #[cfg(debug_assertions)]
                if self.debugging {
                    debug_transparent_shapes(
                        detector,
                        &tracks,
                        region,
                        next_cursor,
                        self.bg_direction,
                    );
                }

                region.tl() + next_cursor
            }
            None => {
                let last_cursor = self.last_cursor.expect("set");
                let last_velocity = self.last_velocity.expect("set") * 1.5;
                let next_cursor = last_cursor
                    + Point::new(
                        last_velocity.x.round() as i32,
                        last_velocity.y.round() as i32,
                    );
                self.last_cursor = Some(next_cursor);

                #[cfg(debug_assertions)]
                if self.debugging {
                    debug_transparent_shapes(
                        detector,
                        &tracks,
                        region,
                        next_cursor,
                        self.bg_direction,
                    );
                }

                region.tl() + next_cursor
            }
        }
    }

    fn update_initial_track_if_needed(&mut self, region: Rect, tracks: &[STrack]) {
        if self.current_track_id.is_none() {
            let region_mid = mid_point(Rect::new(0, 0, region.width, region.height));
            if let Some(track) = find_track_closest_to(region_mid, tracks) {
                self.current_track_id = Some(track.track_id());
                self.last_cursor = Some(mid_point(track.rect()));
                self.last_velocity = Some(track_velocity(track));
            }
        }
    }

    fn update_background_direction(&mut self, tracks: &[STrack]) {
        if let Some(direction) = estimate_background_direction(tracks) {
            self.bg_direction = direction;
        }
    }

    fn update_and_find_best_track<'a>(&mut self, tracks: &'a [STrack]) -> Option<&'a STrack> {
        let current_track_id = self.current_track_id?;
        let bg_direction = self.bg_direction;
        let match_track = tracks
            .iter()
            .filter(|track| track.tracklet_len() >= 2)
            .filter_map(|track| {
                let degree = track_background_degree(track, bg_direction)?;
                if degree <= 20.0 {
                    return None;
                }

                Some((track, degree))
            })
            .max_by(|(_, a_degree), (_, b_degree)| a_degree.partial_cmp(b_degree).unwrap())
            .map(|(track, _)| track);

        if let Some(track) = match_track {
            if track.track_id() == current_track_id {
                self.candidate_track_id = None;
                self.candidate_track_count = 0;
            }

            if self.candidate_track_id == Some(track.track_id()) {
                self.candidate_track_count += 1;
            } else {
                self.candidate_track_id = Some(track.track_id());
                self.candidate_track_count = 0;
            }

            if self.candidate_track_count >= 3 {
                self.candidate_track_id = None;
                self.candidate_track_count = 0;
                return Some(track);
            }
        }

        tracks
            .iter()
            .find(|track| track.track_id() == current_track_id)
    }
}

#[cfg(debug_assertions)]
fn debug_transparent_shapes(
    detector: &dyn Detector,
    tracks: &[STrack],
    region: Rect,
    last_cursor: Point,
    bg_direction: Point2d,
) {
    use opencv::core::MatTraitConst;

    use crate::debug::debug_tracks;

    debug_tracks(
        &detector.mat().roi(region).unwrap(),
        tracks.to_vec(),
        last_cursor,
        bg_direction,
    );
}

fn find_track_closest_to(point: Point, tracks: &[STrack]) -> Option<&STrack> {
    tracks.iter().min_by_key(|track| {
        let track_region = track.rect();
        let track_mid =
            track_region.tl() + Point::new(track_region.width / 2, track_region.height / 2);

        (point - track_mid).norm() as i32
    })
}

fn mid_point(rect: Rect) -> Point {
    rect.tl() + Point::new(rect.width / 2, rect.height / 2)
}

fn predicted_center(track: &STrack) -> Point {
    let (vx, vy) = track.kalman_velocity();
    let point = mid_point(track.kalman_rect());

    Point::new(
        (point.x as f32 + vx).round() as i32,
        (point.y as f32 + vy).round() as i32,
    )
}

fn track_background_degree(track: &STrack, bg_direction: Point2d) -> Option<f64> {
    let velocity = unit(track_velocity(track))?;
    let dot = velocity.dot(bg_direction);
    let det = velocity.cross(bg_direction);
    Some(det.atan2(dot).to_degrees().abs())
}

fn estimate_background_direction(tracks: &[STrack]) -> Option<Point2d> {
    let filtered = tracks
        .iter()
        .filter(|track| track.tracklet_len() >= 5)
        .map(track_velocity)
        .collect::<Vec<Point2d>>();
    if filtered.len() < 3 {
        return None;
    }

    let velocity_sum = filtered
        .into_iter()
        .fold(Point2d::default(), |acc, v| acc + v);
    let velocity_unit = unit(velocity_sum)?;

    Some(velocity_unit)
}

fn track_velocity(track: &STrack) -> Point2d {
    let (vx, vy) = track.kalman_velocity();
    Point2d::new(vx as f64, vy as f64)
}

fn unit<T>(point: Point_<T>) -> Option<Point_<T>>
where
    T: Copy,
    Point_<T>: Div<f64, Output = Point_<T>>,
    f64: From<T>,
{
    let norm = point.norm();
    if norm < 1e-3 {
        return None;
    }

    Some(point / norm)
}
