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
    predicted_position: Option<Point>,      // 预测的下一帧位置  
    last_shape_count: u32,                  // 上一帧的图形数量  
    merge_recovery_frames: u32,             // 融合恢复帧计数器
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}  
  
impl Default for TransparentShapeSolver {  
    fn default() -> Self {  
        Self {  
            tracker: ByteTracker::new(FPS as u64, 0.25, 0.1, 0.25, IouGating::None),  
            current_track_id: None,  
            candidate_track_id: None,  
            candidate_track_count: 0,  
            last_cursor: None,  
            last_velocity: None,  
            bg_direction: Point2d::default(),  
            // 新增字段初始化  
            predicted_position: None,  
            last_shape_count: 0,  
            merge_recovery_frames: 0,  
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
    let current_shape_count = shapes.len() as u32;  
      
    let tracks = self.tracker.update(  
        shapes  
            .into_iter()  
            .map(|(bbox, score)| Detection::new(bbox, score))  
            .collect(),  
    );  
  
    self.update_initial_track_if_needed(region, &tracks);  
    self.update_background_direction(&tracks);  
      
    // 检测融合/分开事件  
    let is_merge = self.last_shape_count > 1 && current_shape_count == 1;  
    let is_split = self.last_shape_count == 1 && current_shape_count > 1;  
      
    if is_merge {  
        self.merge_recovery_frames = 10; // 融合后10帧内禁止切换  
    }  
      
    if self.merge_recovery_frames > 0 {  
        self.merge_recovery_frames -= 1;  
    }  
      
    self.last_shape_count = current_shape_count;  
  
    match self.update_and_find_best_track(&tracks, region, is_merge, is_split) { 
            Some(track) => {  
                let next_cursor = predicted_center(track);  
                self.predicted_position = Some(next_cursor);
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
    let last_velocity = self.last_velocity.expect("set if last_cursor set") * 1;  
    let next_cursor = last_cursor  
        + Point::new(  
            last_velocity.x.round() as i32,  
            last_velocity.y.round() as i32,  
        );  
    self.predicted_position = Some(next_cursor); // 添加这行  
      
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
  
fn update_and_find_best_track<'a>(  
    &mut self,  
    tracks: &'a [STrack],  
    region: Rect,  
    is_merge: bool,  
    is_split: bool,  
) -> Option<&'a STrack> {  
    let last_cursor = self.last_cursor?;  
    let bg_direction = self.bg_direction;  
  
    // 如果当前track ID存在且不在融合恢复期，使用原有逻辑  
    if let Some(current_track_id) = self.current_track_id {  
        if self.merge_recovery_frames == 0 {  
            // 检查当前track是否仍然存在  
            if tracks.iter().any(|t| t.track_id() == current_track_id) {  
                // 使用原有的评分和切换逻辑  
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
                        )?;  
                        Some((track, score, is_current))  
                    })  
                    .collect();  
  
                let best_track_info = scored_tracks  
                    .iter()  
                    .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap());  
                  
                if let Some((best_track, best_score, is_best_current)) = best_track_info {  
                    if *is_best_current {  
                        self.candidate_track_id = None;  
                        self.candidate_track_count = 0;  
                        return Some(best_track);  
                    }  
  
                    let is_same_candidate = self.candidate_track_id == Some(best_track.track_id());  
                    if is_same_candidate {  
                        self.candidate_track_count += 1;  
                    } else {  
                        self.candidate_track_id = Some(best_track.track_id());  
                        self.candidate_track_count = 0;  
                    }  
  
                    let current_score = scored_tracks  
                        .iter()  
                        .find(|(_, _, is_cur)| *is_cur)  
                        .map(|(_, s, _)| *s)  
                        .unwrap_or(0.0);  
  
                    let should_switch = self.candidate_track_count >= 2 && best_score - current_score > 0.1;  
  
                    if should_switch {  
                        debug!(target: "backend/player", "Switch from {:?} to {}", self.current_track_id, best_track.track_id());  
                        self.current_track_id = Some(best_track.track_id());  
                        self.candidate_track_id = None;  
                        self.candidate_track_count = 0;  
                        return Some(best_track);  
                    }  
                }  
                  
                return tracks.iter().find(|t| t.track_id() == current_track_id);  
            }  
        }  
    }  
  
    // 当前track不存在或在融合恢复期，基于位置连续性选择  
    if let Some(predicted_pos) = self.predicted_position {  
        if let Some(closest_track) = find_track_closest_to(predicted_pos, tracks) {  
            // 检查距离是否合理  
            let track_mid = mid_point(closest_track.rect());  
            let distance = (predicted_pos - track_mid).norm();  
            let max_distance = diag(region) * 0.3; // 最大允许距离为区域对角线的30%  
              
            if distance <= max_distance {  
                // 检查运动方向一致性  
                if let Some(last_velocity) = self.last_velocity {  
                    let track_velocity = closest_track.kalman_velocity();  
                    let velocity_diff = (last_velocity - track_velocity).norm();  
                    let max_velocity_diff = last_velocity.norm() * 0.5;  
                      
                    if velocity_diff <= max_velocity_diff || max_velocity_diff < 1e-3 {  
                        self.current_track_id = Some(closest_track.track_id());  
                        self.candidate_track_id = None;  
                        self.candidate_track_count = 0;  
                        return Some(closest_track);  
                    }  
                } else {  
                    // 没有速度信息，直接使用最近距离  
                    self.current_track_id = Some(closest_track.track_id());  
                    self.candidate_track_id = None;  
                    self.candidate_track_count = 0;  
                    return Some(closest_track);  
                }  
            }  
        }  
    }  
  
    // 如果所有方法都失败，返回None让调用者使用Kalman预测  
    None  
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
    // ← 移除了 current_low_angle_frames 参数  
) -> Option<f64> {  
    let angle = track_background_degree(track, bg_direction)?;  
  
    // ← 统一使用 45° 阈值，移除了动态阈值逻辑  
    if angle <= 45.0 {  
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
