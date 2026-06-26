use std::ops::Div;  
  
use log::debug;  
use opencv::core::{  
    Mat, MatTraitConst, Point, Point2d, Point2f, Point_, Rect, Vector,  
};  
use opencv::imgproc::good_features_to_track_def;  
use opencv::video::calc_optical_flow_pyr_lk_def;  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack},  
};  
  
// ── 光流调参常量 ───────────────────────────────────────────  
/// 每次最多检测的特征点数量。  
const MAX_FEATURES: i32 = 80;  
/// 光流存活点低于此数视为"跟丢"，回退到运动惯性。  
const MIN_SURVIVING_FEATURES: usize = 6;  
/// 存活点低于此数时补点（在仍清晰时补充新角点）。  
const REPLENISH_THRESHOLD: usize = 24;  
/// good_features_to_track 的质量阈值。  
const FEATURE_QUALITY: f64 = 0.01;  
/// good_features_to_track 的最小点间距（像素）。  
const FEATURE_MIN_DISTANCE: f64 = 5.0;  
  
#[derive(Debug)]  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
    current_track_id: Option<u64>,  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    // ── 光流状态（坐标均为 region 局部坐标）──  
    prev_gray: Option<Mat>,  
    flow_points: Vec<Point2f>,  
    target_box: Option<Rect>,  
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
            target_box: None,  
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
  
        // 当前帧灰度的 region 子图（光流的输入图像）  
        let cur_gray = extract_gray_roi(detector, region);  
  
        self.update_initial_track_if_needed(region, &tracks, cur_gray.as_ref());  
        self.update_background_direction(&tracks);  
  
        // ── 1. 光流：从上一帧灰度追踪特征点到当前帧 ──  
        let flow_centroid = self.run_optical_flow(cur_gray.as_ref());  
  
        // ── 2. 在仍清晰时补充特征点（避免逐渐变透明后点流失殆尽）──  
        if let (Some(cur), Some(tb)) = (cur_gray.as_ref(), self.target_box) {  
            if self.flow_points.len() < REPLENISH_THRESHOLD {  
                let mut fresh = detect_features(cur, tb);  
                self.flow_points.append(&mut fresh);  
            }  
        }  
  
        // 当前帧灰度存为下一帧的 prev  
        self.prev_gray = cur_gray;  
  
        // ── 3. 光流质心可用：它就是目标位置，无视 trackID 漂移 ──  
        if let Some(centroid) = flow_centroid {  
            // 用质心就近重新锁定一个 track（仅用于取速度/调试，不依赖其 ID 稳定）  
            if let Some(track) = find_track_closest_to(centroid, &tracks) {  
                self.current_track_id = Some(track.track_id());  
                self.last_velocity = Some(track.kalman_velocity());  
            }  
            self.last_cursor = Some(centroid);  
  
            #[cfg(debug_assertions)]  
            if self.is_debugging {  
                debug_transparent_shapes(detector, &tracks, region, centroid, self.bg_direction);  
            }  
  
            let absolute = region.tl() + centroid;  
            if !region.contains(absolute) {  
                return None;  
            }  
            return Some(absolute);  
        }  
  
        // ── 4. 光流跟丢：回退到原有 track 评分逻辑，并重建光流模板 ──  
        match self.update_and_find_best_track(&tracks, region) {  
            Some(track) => {  
                let next_cursor = predicted_center(track);  
                if self.current_track_id != Some(track.track_id()) {  
                    debug!(target: "backend/player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());  
                }  
                self.current_track_id = Some(track.track_id());  
                self.last_cursor = Some(next_cursor);  
                self.last_velocity = Some(track.kalman_velocity());  
  
                // 重新以该 track 的框为目标，重选特征点让光流恢复  
                self.reseed_features(track.rect());  
  
                #[cfg(debug_assertions)]  
                if self.is_debugging {  
                    debug_transparent_shapes(detector, &tracks, region, next_cursor, self.bg_direction);  
                }  
  
                Some(region.tl() + next_cursor)  
            }  
            None => {  
                // ── 5. 完全融合等极端帧：纯速度外推硬撑 ──  
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
                    debug_transparent_shapes(detector, &tracks, region, next_cursor, self.bg_direction);  
                }  
  
                Some(absolute_next_cursor)  
            }  
        }  
    }  
  
    /// 从 prev_gray 到 cur_gray 追踪特征点；存活点足够则返回其质心，并平移 target_box。  
    fn run_optical_flow(&mut self, cur_gray: Option<&Mat>) -> Option<Point> {  
        let prev = self.prev_gray.as_ref()?;  
        let cur = cur_gray?;  
        if self.flow_points.is_empty() {  
            return None;  
        }  
  
        let old_centroid = centroid(&self.flow_points);  
        let survivors = track_flow(prev, cur, &self.flow_points);  
        if survivors.len() < MIN_SURVIVING_FEATURES {  
            // 跟丢：清空让上层重建  
            self.flow_points.clear();  
            return None;  
        }  
  
        let new_centroid = centroid(&survivors);  
  
        // 用质心位移平移目标框，保持"焊在目标上的框"随之移动  
        if let (Some(oc), Some(nc), Some(tb)) = (old_centroid, new_centroid, self.target_box) {  
            let dx = nc.x - oc.x;  
            let dy = nc.y - oc.y;  
            self.target_box = Some(Rect::new(tb.x + dx, tb.y + dy, tb.width, tb.height));  
        }  
  
        self.flow_points = survivors;  
        new_centroid  
    }  
  
    /// 以给定框（region 局部坐标）为目标，重置光流模板。  
    fn reseed_features(&mut self, box_local: Rect) {  
        self.target_box = Some(box_local);  
        if let Some(cur) = self.prev_gray.as_ref() {  
            self.flow_points = detect_features(cur, box_local);  
        } else {  
            self.flow_points.clear();  
        }  
    }  
  
    fn update_initial_track_if_needed(  
        &mut self,  
        region: Rect,  
        tracks: &[STrack],  
        cur_gray: Option<&Mat>,  
    ) {  
        if self.current_track_id.is_none() {  
            let region_mid = mid_point(Rect::new(0, 0, region.width, region.height));  
            if let Some(track) = find_track_closest_to(region_mid, tracks) {  
                self.current_track_id = Some(track.track_id());  
                self.last_cursor = Some(mid_point(track.rect()));  
                self.last_velocity = Some(track.kalman_velocity());  
  
                // 初始锁定：记录目标框并选取特征点  
                let box_local = track.rect();  
                self.target_box = Some(box_local);  
                if let Some(cur) = cur_gray {  
                    self.flow_points = detect_features(cur, box_local);  
                }  
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
  
// ── 光流辅助函数 ───────────────────────────────────────────  
  
/// 取整帧灰度图在 region 内的子图克隆（光流逐帧输入）。  
fn extract_gray_roi(detector: &dyn Detector, region: Rect) -> Option<Mat> {  
    let gray = detector.grayscale();  
    let roi = gray.roi(region).ok()?;  
    roi.try_clone().ok()  
}  
  
/// 在目标框（region 局部坐标）内检测角点，返回 region 局部坐标的特征点。  
fn detect_features(gray: &Mat, box_local: Rect) -> Vec<Point2f> {  
    let bounds = Rect::new(0, 0, gray.cols(), gray.rows());  
    let b = box_local & bounds;  
    if b.width < 4 || b.height < 4 {  
        return Vec::new();  
    }  
    let Ok(sub) = gray.roi(b) else {  
        return Vec::new();  
    };  
  
    let mut corners: Vector<Point2f> = Vector::new();  
    if good_features_to_track_def(  
        &sub,  
        &mut corners,  
        MAX_FEATURES,  
        FEATURE_QUALITY,  
        FEATURE_MIN_DISTANCE,  
    )  
    .is_err()  
    {  
        return Vec::new();  
    }  
  
    corners  
        .into_iter()  
        .map(|p| Point2f::new(p.x + b.x as f32, p.y + b.y as f32))  
        .collect()  
}  
  
/// Lucas-Kanade 光流：返回成功跟踪的点（region 局部坐标）。  
fn track_flow(prev: &Mat, next: &Mat, pts: &[Point2f]) -> Vec<Point2f> {  
    if pts.is_empty() {  
        return Vec::new();  
    }  
  
    let prev_pts: Vector<Point2f> = pts.iter().copied().collect();  
    let mut next_pts: Vector<Point2f> = Vector::new();  
    let mut status: Vector<u8> = Vector::new();  
    let mut err: Vector<f32> = Vector::new();  
  
    if calc_optical_flow_pyr_lk_def(  
        prev,  
        next,  
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
        .into_iter()  
        .zip(status)  
        .filter_map(|(p, ok)| if ok != 0 { Some(p) } else { None })  
        .collect()  
}  
  
fn centroid(pts: &[Point2f]) -> Option<Point> {  
    if pts.is_empty() {  
        return None;  
    }  
    let n = pts.len() as f32;  
    let sum = pts  
        .iter()  
        .fold(Point2f::new(0.0, 0.0), |a, p| Point2f::new(a.x + p.x, a.y + p.y));  
    Some(Point::new((sum.x / n).round() as i32, (sum.y / n).round() as i32))  
}  
  
// ── 原有辅助函数 ───────────────────────────────────────────  
  
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
        &[],   // feature_points：还没接光流数据时先传空切片  
        None,  // target_box：还没接光流数据时先传 None  
    );
  
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
