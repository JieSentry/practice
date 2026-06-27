use std::ops::Div;  
  
use log::debug;  
use opencv::core::{Point, Point_, Point2d, Rect};  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack},  
};  
  
// ── 调参常量 ──────────────────────────────────────────────  
/// 当前目标框与另一个 track 框重叠即视为「正在融合」，融合期冻结速度。  
const FUSION_OVERLAP_AREA: i32 = 1;  
/// 惯性滑行的最大帧数，超过则放弃（返回 None），避免无限盲滑。  
const MAX_SLIDE_FRAMES: u32 = 30;  
  
#[derive(Debug)]  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
    current_track_id: Option<u64>,  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    /// 连续惯性滑行的帧数。  
    slide_frames: u32,  
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
            slide_frames: 0,  
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
  
                // 融合期冻结速度：若目标框与其它 track 框重叠，认为正在融合，  
                // 此时 kalman_velocity 已被融合质心污染，不用它覆盖干净速度快照。  
                if !is_fusing(track, &tracks) {  
                    self.last_velocity = Some(track.kalman_velocity());  
                }  
  
                // 成功锁定到真实 track，清零滑行计数。  
                self.slide_frames = 0;  
  
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
                // 完全融合 / 无逆背景候选：用冻结的干净速度滑行过渡。  
                let last_cursor = self.last_cursor?;  
                let last_velocity = self.last_velocity.expect("set if last_cursor set") * 1.0;  
  
                // 滑行过久则放弃，避免连环融合时一直盲滑。  
                self.slide_frames += 1;  
                if self.slide_frames > MAX_SLIDE_FRAMES {  
                    return None;  
                }  
  
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
  
    fn update_and_find_best_track<'a>(  
        &mut self,  
        tracks: &'a [STrack],  
        region: Rect,  
    ) -> Option<&'a STrack> {  
        let current_track_id = self.current_track_id?;  
        let last_cursor = self.last_cursor?;  
        let bg_direction = self.bg_direction;  
  
        // 计算所有「逆背景运动」候选的分数（被 30° 过滤的会被丢弃）。  
        let scored_tracks: Vec<(&STrack, f64, bool)> = tracks  
            .iter()  
            .filter(|track| track.track_id() == current_track_id || track.tracklet_len() >= 3)  
            .filter_map(|track| {  
                let is_current = track.track_id() == current_track_id;  
                let score =  
                    track_background_score(track, last_cursor, bg_direction, region, is_current)?;  
                Some((track, score, is_current))  
            })  
            .collect();  
  
        // 找出最高分候选。  
        let best_track_info = scored_tracks  
            .iter()  
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());  
        let (best_track, best_score, is_best_current) = match best_track_info {  
            Some(info) => (info.0, info.1, info.2),  
            // 画面里没有任何逆背景候选（完全融合那几帧）→ 交给惯性滑行。  
            None => return None,  
        };  
  
        // 当前目标是否仍在逆背景候选集中。  
        let current_in_scored = scored_tracks.iter().any(|t| t.2);  
  
        // 当前目标已掉出候选集（被背景带走 / 融合 / 丢失）→ 立即重锁定：  
        // 在所有逆背景候选里选离预测光标位置最近的那个，采纳其新 ID。  
        if !current_in_scored {  
            let predicted = predicted_cursor(last_cursor, self.last_velocity);  
            let relocked = reclaim_track(predicted, &scored_tracks).unwrap_or(best_track);  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(relocked);  
        }  
  
        // 最佳仍是当前目标，重置候选并返回。  
        if is_best_current {  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(best_track);  
        }  
  
        // 更新候选计数（迟滞切换，抗抖动）。  
        let is_same_candidate = self.candidate_track_id == Some(best_track.track_id());  
        if is_same_candidate {  
            self.candidate_track_count += 1;  
        } else {  
            self.candidate_track_id = Some(best_track.track_id());  
            self.candidate_track_count = 0;  
        }  
  
        let current_score = scored_tracks  
            .iter()  
            .find(|t| t.2)  
            .map(|t| t.1)  
            .unwrap_or(0.0);  
  
        let should_switch = self.candidate_track_count >= 2 && best_score - current_score > 0.1;  
  
        if should_switch {  
            debug!(target: "backend/player", "Switch from {:?} to {}", self.current_track_id, best_track.track_id());  
            self.current_track_id = Some(best_track.track_id());  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(best_track);  
        }  
  
        // 当前目标仍通过过滤，返回当前目标。  
        scored_tracks.iter().find(|t| t.2).map(|t| t.0)  
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
  
/// 目标框与任意其它 track 框重叠即视为正在融合。  
fn is_fusing(target: &STrack, tracks: &[STrack]) -> bool {  
    let target_rect = target.kalman_rect();  
    tracks.iter().any(|t| {  
        t.track_id() != target.track_id()  
            && (t.kalman_rect() & target_rect).area() >= FUSION_OVERLAP_AREA  
    })  
}  
  
/// 预测光标位置 = 上一帧光标 + 冻结速度。  
fn predicted_cursor(last_cursor: Point, last_velocity: Option<Point2d>) -> Point {  
    match last_velocity {  
        Some(v) => last_cursor + Point::new(v.x.round() as i32, v.y.round() as i32),  
        None => last_cursor,  
    }  
}  
  
/// 在逆背景候选里选离预测位置最近的 track 重新认领。  
fn reclaim_track<'a>(predicted: Point, scored: &[(&'a STrack, f64, bool)]) -> Option<&'a STrack> {  
    scored  
        .iter()  
        .min_by_key(|t| (mid_point(t.0.kalman_rect()) - predicted).norm() as i32)  
        .map(|t| t.0)  
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
  
    // ← 逆背景运动判别阈值：30°（原 45°）  
    if angle <= 30.0 {  
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
