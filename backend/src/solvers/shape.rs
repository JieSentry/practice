use std::ops::Div;  
  
use log::debug;  
use opencv::core::{Point, Point_, Point2d, Rect};  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack},  
};  
  
#[derive(Debug)]  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
    current_track_id: Option<u64>,  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    current_low_angle_frames: u32,  
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}  
  
impl Default for TransparentShapeSolver {  
    fn default() -> Self {  
        Self {  
            tracker: ByteTracker::new(FPS as u64, 0.25, 0.1, 0.25, IouGating::Position),  
            current_track_id: None,  
            candidate_track_id: None,  
            candidate_track_count: 0,  
            last_cursor: None,  
            last_velocity: None,  
            bg_direction: Point2d::default(),  
            current_low_angle_frames: 0,  
            #[cfg(debug_assertions)]  
            is_debugging: false,  
        }  
    }  
}  
  
impl TransparentShapeSolver {  
    #[cfg(debug_assertions)]  
    pub fn debug() -> Self {  
        let mut default = Self::default();  
        default.is_debugging = true;  
        default  
    }  
  
    pub fn solve(&mut self, detector: &dyn Detector, region: Rect) -> Option<Point> {  
        let shapes = detector.detect_transparent_shapes(region);  
        let tracks = self.tracker.update(  
            shapes  
                .into_iter()  
                .map(|(bbox, score)| Detection::new(bbox, score))  
                .collect(),  
        );  
  
        self.update_initial_track_if_needed(region, &tracks);  
        self.update_background_direction(&tracks);  
  
        match self.update_and_find_best_track(&tracks, region) {  
            Some(track) => {  
                let next_cursor = predicted_center(track);  
                if let Some(last_v) = self.last_velocity {  
                    let current_v = track.kalman_velocity();  
                    let dot = last_v.dot(current_v);  
                    if last_v.norm() > 1e-3 && current_v.norm() > 1e-3 && dot < 0.0 {  
                        let last_cursor = self.last_cursor?;  
                        let inertial_v = last_v * 1.5;  
                        let inertial_cursor = last_cursor  
                            + Point::new(inertial_v.x.round() as i32, inertial_v.y.round() as i32);  
                        let absolute = region.tl() + inertial_cursor;  
                        if region.contains(absolute) {  
                            self.last_cursor = Some(inertial_cursor);  
                            return Some(absolute);  
                        }  
                    }  
                }  
                if self.current_track_id != Some(track.track_id()) {  
                    debug!(target: "backend/player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());  
                }  
                self.current_track_id = Some(track.track_id());  
                self.last_cursor = Some(next_cursor);  
                self.last_velocity = Some(track.kalman_velocity());  
  
                #[cfg(debug_assertions)]  
                if self.is_debugging {  
                    debug_transparent_shapes(  
                        detector,  
                        &tracks,  
                        region,  
                        next_cursor,  
                        self.bg_direction,  
                    );  
                }  
  
                Some(region.tl() + next_cursor)  
            }  
            None => {  
                let last_cursor = self.last_cursor?;  
                let last_velocity = self.last_velocity.expect("set if last_cursor set") * 1.5;  
                let next_cursor = last_cursor  
                    + Point::new(  
                        last_velocity.x.round() as i32,  
                        last_velocity.y.round() as i32,  
                    );  
                let absolute_next_cursor = region.tl() + next_cursor;  
                if !region.contains(absolute_next_cursor) {  
                    return None;  
                }  
  
                self.last_cursor = Some(next_cursor);  
  
                #[cfg(debug_assertions)]  
                if self.is_debugging {  
                    debug_transparent_shapes(  
                        detector,  
                        &tracks,  
                        region,  
                        next_cursor,  
                        self.bg_direction,  
                    );  
                }  
  
                Some(absolute_next_cursor)  
            }  
        }  
    }  
  
    fn update_initial_track_if_needed(&mut self, region: Rect, tracks: &[STrack]) {  
        if self.current_track_id.is_none() {  
            let region_mid = mid_point(Rect::new(0, 0, region.width, region.height));  
            if let Some(track) = find_track_closest_to(region_mid, tracks) {  
                self.current_track_id = Some(track.track_id());  
                self.last_cursor = Some(mid_point(track.rect()));  
                self.last_velocity = Some(track.kalman_velocity());  
            }  
        }  
    }  
  
    fn update_background_direction(&mut self, tracks: &[STrack]) {  
        if let Some(direction) = estimate_background_direction(self.last_cursor, tracks)  
            .and_then(|direction| unit(self.bg_direction * 0.5 + direction * 0.5))  
        {  
            self.bg_direction = direction;  
        }  
    }  
  
    fn update_low_angle_count(&mut self, tracks: &[STrack], bg_direction: Point2d) {  
        let current_id = match self.current_track_id {  
            Some(id) => id,  
            None => {  
                self.current_low_angle_frames = 0;  
                return;  
            }  
        };  
  
        if let Some(track) = tracks.iter().find(|t| t.track_id() == current_id) {  
            if let Some(angle) = track_background_degree(track, bg_direction) {  
                if angle <= 45.0 {  
                    self.current_low_angle_frames += 1;  
                } else {  
                    self.current_low_angle_frames = 0;  
                }  
            }  
        } else {  
            self.current_low_angle_frames = 0;  
        }  
    }  
  
    fn update_and_find_best_track<'a>(  
        &mut self,  
        tracks: &'a [STrack],  
        region: Rect,  
    ) -> Option<&'a STrack> {  
        let current_track_id = self.current_track_id?;  
        let last_cursor = self.last_cursor?;  
        let bg_direction = self.bg_direction;  
  
        self.update_low_angle_count(tracks, bg_direction);  
        let low_angle_frames = self.current_low_angle_frames;  
  
        // 计算所有候选分数  
        let scored_tracks: Vec<_> = tracks  
            .iter()  
            .filter(|track| {  
                track.track_id() == current_track_id || track.tracklet_len() >= 3  
            })  
            .filter_map(|track| {  
                let is_current = track.track_id() == current_track_id;  
                let score = track_background_score(  
                    track,  
                    last_cursor,  
                    bg_direction,  
                    region,  
                    is_current,  
                    low_angle_frames,  
                )?;  
                Some((track, score, is_current))  
            })  
            .collect();  
  
        // 找出最高分  
        let best_track_info = scored_tracks  
            .iter()  
            .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap());  
        let (best_track, best_score, is_best_current) = match best_track_info {  
            Some(info) => info,  
            None => return tracks.iter().find(|t| t.track_id() == current_track_id),  
        };  
  
        // 如果最佳仍是当前目标，重置候选  
        if *is_best_current {  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(best_track);  
        }  
  
        // 更新候选计数  
        let is_same_candidate = self.candidate_track_id == Some(best_track.track_id());  
        if is_same_candidate {  
            self.candidate_track_count += 1;  
        } else {  
            self.candidate_track_id = Some(best_track.track_id());  
            self.candidate_track_count = 0;  
        }  
  
        // 获取当前目标的分数  
        let current_score = scored_tracks  
            .iter()  
            .find(|(_, _, is_cur)| *is_cur)  
            .map(|(_, s, _)| *s)  
            .unwrap_or(0.0);  
  
        // 切换条件：当前目标表现像背景时放宽，否则保守  
        let should_switch = if low_angle_frames >= 3 {  
            self.candidate_track_count >= 2 && *best_score > current_score  
        } else {  
            self.candidate_track_count >= 3 && best_score - current_score > 0.1  
        };  
  
        if should_switch {  
            debug!(target: "backend/player", "Switch from {:?} to {}", self.current_track_id, best_track.track_id());  
            self.current_track_id = Some(best_track.track_id());  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            self.current_low_angle_frames = 0;  
            return Some(best_track);  
        }  
  
        // 默认返回当前目标  
        tracks.iter().find(|t| t.track_id() == current_track_id)  
    }  
}  
  
impl Drop for TransparentShapeSolver {  
    fn drop(&mut self) {  
        #[cfg(debug_assertions)]  
        if self.is_debugging {  
            use opencv::highgui::destroy_all_windows;  
  
            let _ = destroy_all_windows();  
        }  
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
  
    use crate::debug::debug_shape_tracks;  
  
    debug_shape_tracks(  
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
    let v = track.kalman_velocity();  
    let point = mid_point(track.kalman_rect());  
  
    Point::new(  
        (point.x as f64 + v.x).round() as i32,  
        (point.y as f64 + v.y).round() as i32,  
    )  
}  
  
fn track_background_score(  
    track: &STrack,  
    last_cursor: Point,  
    bg_direction: Point2d,  
    region: Rect,  
    is_current_track: bool,  
    current_low_angle_frames: u32,  
) -> Option<f64> {  
    let angle = track_background_degree(track, bg_direction)?;  
  
    // 当前目标连续多帧低角度时，用更严格的阈值过滤它  
    let threshold = if is_current_track && current_low_angle_frames >= 3 {  
        45.0  
    } else {  
        35.0  
    };  
    if angle <= threshold {  
        return None;  
    }  
  
    let angle_score = angle / 180.0;  
  
    // 乘法评分：距离惩罚  
    let distance_penalty = if angle >= 60.0 {  
        1.0  
    } else {  
        let cursor_dir = mid_point(track.rect()) - last_cursor;  
        let dist_squared = (cursor_dir.x.pow(2) + cursor_dir.y.pow(2)) as f64;  
        let sigma = 0.25 * diag(region);  
        (-dist_squared / (2.0 * sigma.powi(2))).exp()  
    };  
  
    if distance_penalty <= 0.3 {  
        return None;  
    }  
  
    let mut score = angle_score * distance_penalty;  
  
    // 当前目标加分：只有在非低角度状态时才加分  
    if is_current_track && current_low_angle_frames < 3 {  
        score += 0.15;  
    }  
  
    if score <= 0.2 {  
        return None;  
    }  
  
    Some(score)  
}  
  
fn track_background_degree(track: &STrack, bg_direction: Point2d) -> Option<f64> {  
    let dir = unit(track.kalman_velocity())?;  
    let dot = dir.dot(bg_direction);  
    let det = dir.cross(bg_direction);  
    Some(det.atan2(dot).to_degrees().abs())  
}  
  
fn estimate_background_direction(last_cursor: Option<Point>, tracks: &[STrack]) -> Option<Point2d> {  
    let mut last_rect_contains_cursor = None;  
    let filtered = tracks  
        .iter()  
        .filter(|track| {  
            if track.tracklet_len() < 5 {  
                return false;  
            }  
  
            if last_rect_contains_cursor.is_some_and(|rect: Rect| (rect & track.rect()).area() > 0)  
            {  
                return false;  
            }  
  
            let Some(last_cursor) = last_cursor else {  
                return true;  
            };  
  
            let rect = track.rect();  
            if rect.contains(last_cursor) {  
                if last_rect_contains_cursor.is_none() {  
                    last_rect_contains_cursor = Some(rect);  
                }  
  
                return false;  
            }  
  
            let norm = (mid_point(track.rect()) - last_cursor).norm();  
            norm >= diag(track.rect())  
        })  
        .map(STrack::kalman_velocity)  
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
  
fn diag(rect: Rect) -> f64 {  
    ((rect.width.pow(2) + rect.height.pow(2)) as f64).sqrt()  
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
