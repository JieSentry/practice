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
    // 移除 candidate_track_id  
    // 移除 candidate_track_count  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}
  
impl Default for TransparentShapeSolver {  
    fn default() -> Self {  
        Self {  
            tracker: ByteTracker::new(FPS as u64, 0.25, 0.1, 0.25, IouGating::None),  
            current_track_id: None,  
            last_cursor: None,  
            last_velocity: None,  
            bg_direction: Point2d::default(),  
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
    if self.current_track_id != Some(track.track_id()) {  
        debug!(target: "backend/player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());  
    }  
    self.current_track_id = Some(track.track_id());  
    self.last_cursor = Some(next_cursor);  
    self.last_velocity = Some(track.kalman_velocity());  
    // ... debug code unchanged ...  
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
  
    // Clamp to region bounds instead of returning None  
    let clamped_cursor = Point::new(  
        next_cursor.x.clamp(0, region.width - 1),  
        next_cursor.y.clamp(0, region.height - 1),  
    );  
    let absolute_next_cursor = region.tl() + clamped_cursor;  
  
    self.last_cursor = Some(clamped_cursor);  
  
    #[cfg(debug_assertions)]  
    if self.is_debugging {  
        debug_transparent_shapes(  
            detector,  
            &tracks,  
            region,  
            clamped_cursor,  
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
) -> Option<&'a STrack> {  
    let last_cursor = self.last_cursor?;  
    let last_velocity = self.last_velocity.unwrap_or_default();  
    let bg_direction = self.bg_direction;  
  
    // 1. Solver 自己的位置预测（独立于 ByteTracker track ID）  
    let predicted_pos = last_cursor + Point::new(  
        last_velocity.x.round() as i32,  
        last_velocity.y.round() as i32,  
    );  
  
    // 2. 对所有 track 计算组合评分  
    let current_track_id = self.current_track_id;  
    let scored_tracks: Vec<_> = tracks  
        .iter()  
        .filter(|track| {  
            // 当前 track 或已跟踪 >= 2 帧的 track 都参与评分  
            Some(track.track_id()) == current_track_id || track.tracklet_len() >= 2  
        })  
        .filter_map(|track| {  
            let score = combined_score(  
                track,  
                predicted_pos,  
                bg_direction,  
                region,  
            )?;  
            Some((track, score))  
        })  
        .collect();  
  
    // 3. 找最高分 track  
    let (best_track, best_score) = scored_tracks  
        .iter()  
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())  
        .map(|(t, s)| (*t, *s))?;  
  
    // 4. 轻量 hysteresis：如果当前 track 的分数 >= 最高分的 85%，保持当前 track  
    //    这防止帧间抖动，但不会像旧机制那样阻止纠正（旧机制需要连续 2 帧 + 分差 > 0.1）  
if let Some(current_id) = current_track_id  
    && let Some((current_track, current_score)) = scored_tracks  
        .iter()  
        .find(|(t, _)| t.track_id() == current_id)  
    && *current_score >= best_score * 0.85  
{  
    return Some(current_track);  
} 
  
    // 5. 切换到最高分 track  
    if Some(best_track.track_id()) != current_track_id {  
        debug!(target: "backend/player", "shape re-identified: {:?} -> {} (score: {:.3})",  
            current_track_id, best_track.track_id(), best_score);  
    }  
    Some(best_track)  
} 
  
    // ← 移除了整个 update_low_angle_count 方法  
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
  
fn combined_score(  
    track: &STrack,  
    predicted_pos: Point,  
    bg_direction: Point2d,  
    region: Rect,  
) -> Option<f64> {  
    // 角度评分（与背景方向的夹角）  
    let angle = track_background_degree(track, bg_direction)?;  
    if angle <= 45.0 {  
        return None; // 与背景同向，排除  
    }  
    let angle_score = angle / 180.0;  
  
    // 位置接近度评分（与 Solver 预测位置的距离）  
    let track_center = mid_point(track.rect());  
    let diff = track_center - predicted_pos;  
    let dist_squared = (diff.x.pow(2) + diff.y.pow(2)) as f64;  
    let sigma = 0.3 * diag(region);  
    let proximity_score = (-dist_squared / (2.0 * sigma.powi(2))).exp();  
  
    // 组合评分：角度是主要信号，位置接近度作为调制  
    // proximity 范围 [0, 1]，映射到 [0.3, 1.0]，确保高角度但远距离的 track 仍有机会  
    let score = angle_score * (0.3 + 0.7 * proximity_score);  
  
    if score <= 0.15 {  
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
