use std::ops::Div;  
  
use log::debug;  
use opencv::core::{Mat, MatTraitConst, Point, Point2d, Point2f, Point_, Rect, Vector};  
use opencv::imgproc::good_features_to_track_def;  
use opencv::video::calc_optical_flow_pyr_lk_def;  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack},  
};  
  
// ── 光流调参常量 ─────────────────────────────────  
/// 每个目标框最多取多少特征点。  
const MAX_FLOW_POINTS: i32 = 40;  
/// 存活特征点少于该值时，用当前目标框重新补点。  
const MIN_FLOW_POINTS: usize = 6;  
  
#[derive(Debug)]  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
    current_track_id: Option<u64>,  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    // 光流：上一帧灰度（区域内坐标系）与跟踪中的特征点（区域内坐标）  
    prev_gray: Option<Mat>,  
    flow_points: Vec<Point2f>,  
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
            prev_gray: None,  
            flow_points: Vec::new(),  
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
  
        // 当前帧灰度 ROI（区域内坐标系）  
        let cur_gray = detector  
            .grayscale()  
            .roi(region)  
            .ok()  
            .map(|m| m.clone_pointee());  
  
        // 1) 用光流把上一帧特征点推进到这一帧，只保留跟踪成功的点  
        if let (Some(prev), Some(cur)) = (self.prev_gray.as_ref(), cur_gray.as_ref()) {  
            self.flow_points = track_features(prev, cur, &self.flow_points);  
        }  
  
        // 2) 常规打分选目标（作为辅助 / 补点框 / 兜底）  
        let best_info = self  
            .update_and_find_best_track(&tracks, region)  
            .map(|t| (t.track_id(), t.rect(), predicted_center(t), t.kalman_velocity()));  
  
        // 3) 特征点不足时，用当前目标框重新补点  
        if self.flow_points.len() < MIN_FLOW_POINTS {  
            if let (Some(cur), Some((_, rect, _, _))) = (cur_gray.as_ref(), best_info.as_ref()) {  
                let pts = collect_features(cur, *rect, region);  
                if pts.len() >= self.flow_points.len() {  
                    self.flow_points = pts;  
                }  
            }  
        }  
  
        // 4) 计算光标：优先用光流质心  
        let next_cursor = if let Some(fc) = centroid(&self.flow_points) {  
            let c = Point::new(fc.x.round() as i32, fc.y.round() as i32);  
            // 光流质心始终贴着真目标 → 用它重认领离它最近的 track  
            if let Some(t) = find_track_closest_to(c, &tracks) {  
                self.current_track_id = Some(t.track_id());  
                self.last_velocity = Some(t.kalman_velocity());  
            }  
            Some(c)  
        } else if let Some((id, _, pc, vel)) = best_info {  
            // 没有光流点：退回 track 预测中心  
            self.current_track_id = Some(id);  
            self.last_velocity = Some(vel);  
            Some(pc)  
        } else {  
            // 完全没信息：纯速度外推  
            let last = self.last_cursor?;  
            let v = self.last_velocity? * 1.5;  
            Some(last + Point::new(v.x.round() as i32, v.y.round() as i32))  
        };  
  
        let next_cursor = next_cursor?;  
        let absolute = region.tl() + next_cursor;  
        if !region.contains(absolute) {  
            return None;  
        }  
  
        self.last_cursor = Some(next_cursor);  
        self.prev_gray = cur_gray;  
  
        #[cfg(debug_assertions)]  
        if self.is_debugging {  
            debug_transparent_shapes(  
                detector,  
                &tracks,  
                region,  
                next_cursor,  
                self.bg_direction,  
                &self.flow_points,  
            );  
        }  
  
        Some(absolute)  
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
        let current_track_id = self.current_track_id?;  
        let last_cursor = self.last_cursor?;  
        let bg_direction = self.bg_direction;  
  
        let scored_tracks: Vec<_> = tracks  
            .iter()  
            .filter(|track| track.track_id() == current_track_id || track.tracklet_len() >= 3)  
            .filter_map(|track| {  
                let is_current = track.track_id() == current_track_id;  
                let score =  
                    track_background_score(track, last_cursor, bg_direction, region, is_current)?;  
                Some((track, score, is_current))  
            })  
            .collect();  
  
        let best_track_info = scored_tracks  
            .iter()  
            .max_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap());  
        let (best_track, best_score, is_best_current) = match best_track_info {  
            Some(info) => info,  
            None => return tracks.iter().find(|t| t.track_id() == current_track_id),  
        };  
  
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
  
        let should_switch =  
            self.candidate_track_count >= 2 && best_score - current_score > 0.1;  
  
        if should_switch {  
            debug!(target: "backend/player", "Switch from {:?} to {}", self.current_track_id, best_track.track_id());  
            self.current_track_id = Some(best_track.track_id());  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(best_track);  
        }  
  
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
    flow_points: &[Point2f],  
) {  
    use crate::debug::debug_shape_tracks;  
  
    debug_shape_tracks(  
        &detector.mat().roi(region).unwrap(),  
        tracks.to_vec(),  
        last_cursor,  
        bg_direction,  
        flow_points,  
    );  
}  
  
// ── 光流辅助函数 ─────────────────────────────────  
  
/// 在目标框范围内取特征点，返回区域内坐标。  
fn collect_features(gray: &Mat, box_rect: Rect, region: Rect) -> Vec<Point2f> {  
    let bound = Rect::new(0, 0, region.width, region.height);  
    let r = box_rect & bound;  
    if r.width < 3 || r.height < 3 {  
        return Vec::new();  
    }  
  
    let Ok(sub) = gray.roi(r) else {  
        return Vec::new();  
    };  
  
    let mut corners = Vector::<Point2f>::new();  
    if good_features_to_track_def(&sub, &mut corners, MAX_FLOW_POINTS, 0.01, 5.0).is_err() {  
        return Vec::new();  
    }  
  
    corners  
        .iter()  
        .map(|p| Point2f::new(p.x + r.x as f32, p.y + r.y as f32))  
        .collect()  
}  
  
/// 用 LK 光流把上一帧的点推进到当前帧，只保留跟踪成功的点。  
fn track_features(prev: &Mat, cur: &Mat, pts: &[Point2f]) -> Vec<Point2f> {  
    if pts.is_empty() {  
        return Vec::new();  
    }  
  
    let prev_pts: Vector<Point2f> = pts.iter().copied().collect();  
    let mut next_pts = Vector::<Point2f>::new();  
    let mut status = Vector::<u8>::new();  
    let mut err = Vector::<f32>::new();  
  
    if calc_optical_flow_pyr_lk_def(  
        prev,  
        cur,  
        &prev_pts,  
        &mut next_pts,  
        &mut status,  
        &mut err,  
    )  
    .is_err()  
    {  
        return Vec::new();  
    }  
  
    next_pts  
        .iter()  
        .zip(status.iter())  
        .filter(|(_, s)| *s == 1)  
        .map(|(p, _)| p)  
        .collect()  
}  
  
fn centroid(pts: &[Point2f]) -> Option<Point2f> {  
    if pts.is_empty() {  
        return None;  
    }  
  
    let mut sx = 0.0f32;  
    let mut sy = 0.0f32;  
    for p in pts {  
        sx += p.x;  
        sy += p.y;  
    }  
    let n = pts.len() as f32;  
    Some(Point2f::new(sx / n, sy / n))  
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
) -> Option<f64> {  
    let angle = track_background_degree(track, bg_direction)?;  
  
    if angle <= 45.0 {  
        return None;  
    }  
  
    let angle_score = angle / 180.0;  
  
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
  
    if is_current_track {  
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
