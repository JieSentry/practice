use log::debug;  
use opencv::core::{Point, Point2d, Rect};  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack, TrackState},  
};  
  
/// 透明形状测谎求解器 —— 完全对齐 Python `ShapeSolver`(solver.py)。  
///  
/// 集成 ByteTracker + 5 项用户优化:  
/// 1. 运动约束(search_radius)  2. 重叠惩罚(IoU)  3. 稳定 track 保留  
/// 4. 动态阈值  5. 背景方向 EMA 平滑  
#[derive(Debug)]  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
  
    // 跟踪状态  
    current_track_id: Option<u64>,  
    last_cursor: Option<Point2d>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
  
    // 候选切换逻辑  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
  
    // ===== 用户优化参数(对齐 solver.py __init__) =====  
    // 优化1: 运动约束 —— 搜索半径 = factor * (w + h) / 2  
    search_radius_factor: f64,  
    // 优化2: 重叠惩罚  
    overlap_iou_thresh: f64,  
    overlap_switch_penalty: f64,  
    // 优化4: 动态阈值  
    dynamic_thresh_enabled: bool,  
    det_count_low: usize,  
    det_count_high: usize,  
    // 优化5: 背景方向 EMA 平滑系数  
    bg_direction_alpha: f64,  
  
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}  
  
impl Default for TransparentShapeSolver {  
    fn default() -> Self {  
        Self {  
            // 对齐 Python: ByteTracker(30, 0.25, 0.1, 0.25, IouGating.None_)  
            tracker: ByteTracker::new(FPS as u64, 0.25, 0.1, 0.25, IouGating::None),  
            current_track_id: None,  
            last_cursor: None,  
            last_velocity: None,  
            bg_direction: Point2d::default(),  
            candidate_track_id: None,  
            candidate_track_count: 0,  
            search_radius_factor: 2.0,  
            overlap_iou_thresh: 0.1,  
            overlap_switch_penalty: 0.1,  
            dynamic_thresh_enabled: true,  
            det_count_low: 5,  
            det_count_high: 15,  
            bg_direction_alpha: 0.3,  
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
  
    /// 对齐 Python `ShapeSolver.solve(image)`。  
    pub fn solve(&mut self, detector: &dyn Detector, region: Rect) -> Option<Point> {  
        let region_w = region.width as f64;  
        let region_h = region.height as f64;  
  
        // 检测形状(region 相对坐标)  
        let shapes = detector.detect_transparent_shapes(region);  
  
        // 优化4: 动态阈值调整  
        if self.dynamic_thresh_enabled {  
            self.adjust_thresholds(shapes.len());  
        }  
  
        // 转换为 Detection 传入 ByteTracker  
        let det_tracks: Vec<Detection> = shapes  
            .into_iter()  
            .map(|(bbox, score)| Detection::new(bbox, score))  
            .collect();  
  
        // 更新追踪器  
        let tracks = self.tracker.update(det_tracks);  
  
        // 检查 current track 是否还存在(被 ByteTracker 删除后需要重置)  
        if let Some(current_id) = self.current_track_id {  
            let exists = self  
                .tracker  
                .tracked()  
                .iter()  
                .chain(self.tracker.lost().iter())  
                .any(|t| t.track_id() == current_id);  
            if !exists {  
                self.current_track_id = None;  
                self.last_cursor = None;  
                self.last_velocity = None;  
                self.candidate_track_id = None;  
                self.candidate_track_count = 0;  
            }  
        }  
  
        // 初始化跟踪(如果没有当前 track)  
        if self.current_track_id.is_none() && !tracks.is_empty() {  
            // 优先使用 last_cursor 作为参考点,否则用 ROI 中心  
            let ref_point = self  
                .last_cursor  
                .unwrap_or_else(|| Point2d::new(region_w / 2.0, region_h / 2.0));  
            let closest = tracks  
                .iter()  
                .min_by(|a, b| {  
                    let da = (track_center(a) - ref_point).norm();  
                    let db = (track_center(b) - ref_point).norm();  
                    da.partial_cmp(&db).unwrap()  
                })  
                .unwrap();  
            self.current_track_id = Some(closest.track_id());  
            self.last_cursor = Some(track_center(closest));  
            self.last_velocity = Some(closest.kalman_velocity());  
            debug!(target: "backend/player", "INIT track_id={}", closest.track_id());  
        }  
  
        // 更新背景方向  
        self.update_background_direction(&tracks);  
  
        // 找到最佳追踪目标  
        if let Some(best_track) = self.find_best_track(&tracks, region_w, region_h) {  
            let next_cursor = predicted_center(&best_track);  
            self.current_track_id = Some(best_track.track_id());  
            self.last_cursor = Some(next_cursor);  
            self.last_velocity = Some(best_track.kalman_velocity());  
  
            #[cfg(debug_assertions)]  
            if self.is_debugging {  
                debug_transparent_shapes(detector, &tracks, region, next_cursor, self.bg_direction);  
            }  
  
            return Some(to_point(region.tl(), next_cursor));  
        }  
  
// 优化2: 检查 current_track_id 是否在 lost 池中,用其 Kalman 预测位置  
        if let Some(current_id) = self.current_track_id  
            && let Some((next_cursor, vel)) = self  
                .tracker  
                .lost()  
                .iter()  
                .find(|t| t.track_id() == current_id)  
                .map(|t| {  
                    let k = t.kalman_tlwh_pub();  
                    (  
                        Point2d::new((k[0] + k[2] / 2.0) as f64, (k[1] + k[3] / 2.0) as f64),  
                        t.kalman_velocity(),  
                    )  
                })  
        {  
            self.last_cursor = Some(next_cursor);  
            self.last_velocity = Some(vel);  
            return Some(to_point(region.tl(), next_cursor));  
        } 
  
        // 兜底:last_cursor + last_velocity * 1.5 线性外推(与 Komari 一致)  
        if let (Some(last_cursor), Some(last_velocity)) = (self.last_cursor, self.last_velocity) {  
            let next_cursor = last_cursor + last_velocity * 1.5;  
            if next_cursor.x >= 0.0  
                && next_cursor.x < region_w  
                && next_cursor.y >= 0.0  
                && next_cursor.y < region_h  
            {  
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
  
                return Some(to_point(region.tl(), next_cursor));  
            }  
        }  
  
        None  
    }  
  
    // ===== 优化4: 动态阈值调整(对齐 _adjust_thresholds) =====  
    fn adjust_thresholds(&mut self, det_count: usize) {  
        let low = self.det_count_low as f32;  
        let high = self.det_count_high as f32;  
        let count = det_count as f32;  
  
        if count <= low {  
            self.tracker.set_high_match_score_threshold(0.20);  
            self.tracker.set_low_match_score_threshold(0.05);  
        } else if count >= high {  
            self.tracker.set_high_match_score_threshold(0.30);  
            self.tracker.set_low_match_score_threshold(0.15);  
        } else {  
            let t = (count - low) / (high - low);  
            self.tracker.set_high_match_score_threshold(0.20 + t * 0.10);  
            self.tracker.set_low_match_score_threshold(0.05 + t * 0.10);  
        }  
    }  
  
    // ===== 背景方向估计 + 优化5 EMA 平滑(对齐 _update_background_direction) =====  
    fn update_background_direction(&mut self, tracks: &[STrack]) {  
        if tracks.len() < 3 {  
            return;  
        }  
  
        let mut velocities: Vec<Point2d> = vec![];  
        for track in tracks {  
            if track.tracklet_len() < 5 {  
                continue;  
            }  
  
            let t = track.tlwh();  
            let (x, y, w, h) = (t[0] as f64, t[1] as f64, t[2] as f64, t[3] as f64);  
            let center = Point2d::new(x + w / 2.0, y + h / 2.0);  
  
            if let Some(last_cursor) = self.last_cursor {  
                // 排除包含光标的轨迹  
                if x <= last_cursor.x  
                    && last_cursor.x <= x + w  
                    && y <= last_cursor.y  
                    && last_cursor.y <= y + h  
                {  
                    continue;  
                }  
                // 距离过近则跳过  
                let dist = (center - last_cursor).norm();  
                let diag = (w * w + h * h).sqrt();  
                if dist < diag {  
                    continue;  
                }  
            }  
  
            velocities.push(track.kalman_velocity());  
        }  
  
        if velocities.len() >= 3 {  
            let velocity_sum = velocities  
                .into_iter()  
                .fold(Point2d::default(), |acc, v| acc + v);  
            let norm = velocity_sum.norm();  
            if norm > 1e-3 {  
                let new_direction = velocity_sum / norm;  
                // EMA 平滑  
                let blended = self.bg_direction * (1.0 - self.bg_direction_alpha)  
                    + new_direction * self.bg_direction_alpha;  
                // 重新归一化  
                self.bg_direction = blended / (blended.norm() + 1e-6);  
            }  
        }  
    }  
  
    // ===== 核心:找到最佳追踪目标(对齐 _find_best_track) =====  
    fn find_best_track(  
        &mut self,  
        tracks: &[STrack],  
        region_w: f64,  
        region_h: f64,  
    ) -> Option<STrack> {  
        let current_id = self.current_track_id?;  
        if tracks.is_empty() {  
            return None;  
        }  
  
        let current_track = tracks.iter().find(|t| t.track_id() == current_id).cloned();  
        let current_in_tracks = current_track.is_some();  
  
// ===== 核心策略:稳定 track 直接保留(增加融合→分离防护) =====  
        if let Some(ref ct) = current_track  
            && ct.state() == TrackState::Tracked  
            && ct.tracklet_len() >= 10  
            && ct.score() >= 0.50  
        {  
            // 融合→分离防护:若稳定 track 出现异常跳变,说明底层 ByteTracker  
            // 在两图形分离时把 ID 错配到了另一物理图形上。此时不再信任该 ID,  
            // 改选距离上一帧光标最近的 track(分离后仍留在原处的正确图形)。  
            if !self.is_motion_consistent(ct)  
                && let Some(lc) = self.last_cursor  
                && let Some(closest) = tracks.iter().min_by(|a, b| {  
                    (track_center(a) - lc)  
                        .norm()  
                        .partial_cmp(&(track_center(b) - lc).norm())  
                        .unwrap()  
                })  
                && closest.track_id() != ct.track_id()  
            {  
                debug!(  
                    target: "backend/player",  
                    "REASSIGN(merge-split) {} -> {}",  
                    ct.track_id(),  
                    closest.track_id()  
                );  
                self.candidate_track_id = None;  
                self.candidate_track_count = 0;  
                return Some(closest.clone());  
            }  
  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return Some(ct.clone());  
        }
  
        // 计算 predicted_pos  
        let predicted_pos: Option<Point2d> = if let Some(ref ct) = current_track {  
            Some(predicted_center(ct))  
        } else if let (Some(lc), Some(lv)) = (self.last_cursor, self.last_velocity) {  
            Some(lc + lv)  
        } else {  
            None  
        };  
  
        // 计算每个轨迹的分数  
        let mut scored_tracks: Vec<(STrack, f64)> = vec![];  
        for track in tracks {  
            let is_current = track.track_id() == current_id;  
  
            // tracklet_len 过滤:current 在 tracks 中时,新候选需 >= 1  
            if !is_current && track.tracklet_len() < 1 && current_in_tracks {  
                continue;  
            }  
  
            // 运动约束  
            if !is_current  
                && let Some(pp) = predicted_pos  
            {  
                let dist = (track_center(track) - pp).norm();  
                let search_radius = if let Some(ref ct) = current_track {  
                    let t = ct.tlwh();  
                    self.search_radius_factor * (t[2] as f64 + t[3] as f64) / 2.0  
                } else {  
                    self.search_radius_factor * 50.0  
                };  
                if dist > search_radius {  
                    continue;  
                }  
            } 
  
            let mut score = self  
                .track_background_score(track, region_w, region_h, is_current)  
                .unwrap_or(track.score() as f64 * 0.2);  
  
            // 重叠惩罚  
            if !is_current  
                && let Some(ref ct) = current_track  
                && iou(track, ct) > self.overlap_iou_thresh  
            {  
                score *= self.overlap_switch_penalty;  
            } 
  
            scored_tracks.push((track.clone(), score));  
        }  
  
        if scored_tracks.is_empty() {  
            return current_track;  
        }  
  
        // 选择最高分  
        let (best_track, best_score) = scored_tracks  
            .iter()  
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())  
            .map(|(t, s)| (t.clone(), *s))  
            .unwrap();  
  
        // 如果 best 就是 current,直接返回  
        if best_track.track_id() == current_id {  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            return current_track;  
        }  
  
        // 获取 current track 的分数  
        let current_score: Option<f64> = scored_tracks  
            .iter()  
            .find(|(t, _)| t.track_id() == current_id)  
            .map(|(_, s)| *s);  
  
        // 切换门槛  
        let mut switch_threshold_multiplier = 1.0f64;  
        let mut required_confirm_frames = 3u32;  
        if let Some(ref ct) = current_track {  
            if ct.tracklet_len() >= 50 {  
                switch_threshold_multiplier = 3.0;  
                required_confirm_frames = 6;  
            } else if ct.tracklet_len() >= 20 {  
                switch_threshold_multiplier = 2.0;  
                required_confirm_frames = 5;  
            } else if ct.tracklet_len() >= 10 {  
                switch_threshold_multiplier = 1.5;  
                required_confirm_frames = 4;  
            }  
  
            if ct.score() >= 0.85 {  
                switch_threshold_multiplier *= 1.5;  
                required_confirm_frames += 1;  
            }  
        }  
  
        // current track 有分数且候选没有显著优势,不切换  
        if let Some(cs) = current_score  
            && best_score <= cs * switch_threshold_multiplier  
        {  
            return current_track;  
        } 
  
        // 候选确认计数  
        if self.candidate_track_id == Some(best_track.track_id()) {  
            self.candidate_track_count += 1;  
        } else {  
            self.candidate_track_id = Some(best_track.track_id());  
            self.candidate_track_count = 0;  
        }  
  
        // 确认切换  
        if self.candidate_track_count >= required_confirm_frames {  
            self.candidate_track_id = None;  
            self.candidate_track_count = 0;  
            debug!(target: "backend/player", "SWITCH to id={}", best_track.track_id());  
            return Some(best_track);  
        }  
  
        // current_track 不在 tracks 中时,直接返回 best_track(旧 track 已不可见)  
        if current_track.is_none() {  
            return Some(best_track);  
        }  
  
        current_track  
    }  
  
    // ===== 背景运动分数(对齐 _track_background_score) =====  
    fn track_background_score(  
        &self,  
        track: &STrack,  
        region_w: f64,  
        region_h: f64,  
        is_current: bool,  
    ) -> Option<f64> {  
        // bg_direction 无效时退化为距离先验评分  
        if self.bg_direction.norm() < 0.1 {  
            return self.distance_prior_score(track, region_w, region_h, is_current);  
        }  
  
        let angle = self.track_background_degree(track);  
  
        // 角度门槛:current 更宽松(30° vs 45°)  
        let angle_threshold = if is_current { 30.0 } else { 45.0 };  
        if angle <= angle_threshold {  
            return None;  
        }  
  
        let score = angle / 180.0;  
  
        // 距离惩罚  
        let distance_penalty = if angle >= 60.0 {  
            1.0  
        } else if let Some(last_cursor) = self.last_cursor {  
            let cursor_dir = track_center(track) - last_cursor;  
            let cursor_squared = cursor_dir.x * cursor_dir.x + cursor_dir.y * cursor_dir.y;  
            let sigma = 0.25 * (region_w * region_w + region_h * region_h).sqrt();  
            (-cursor_squared / (2.0 * sigma * sigma)).exp()  
        } else {  
            1.0  
        };  
  
        // current 的距离惩罚门槛也更宽松  
        let penalty_threshold = if is_current { 0.15 } else { 0.3 };  
        if distance_penalty <= penalty_threshold {  
            return None;  
        }  
  
        Some(score * distance_penalty)  
    }  
  
    // ===== bg_direction 无效时的距离先验评分(对齐 _distance_prior_score) =====  
    fn distance_prior_score(  
        &self,  
        track: &STrack,  
        region_w: f64,  
        region_h: f64,  
        is_current: bool,  
    ) -> Option<f64> {  
        let last_cursor = self.last_cursor?;  
  
        let dist = (track_center(track) - last_cursor).norm();  
        let sigma = 0.25 * (region_w * region_w + region_h * region_h).sqrt();  
        let distance_score = (-dist * dist / (2.0 * sigma * sigma)).exp();  
  
        let threshold = if is_current { 0.10 } else { 0.25 };  
        if distance_score <= threshold {  
            return None;  
        }  
  
        Some(distance_score)  
    }  
  
    // ===== 速度与背景方向夹角(对齐 _track_background_degree) =====  
    fn track_background_degree(&self, track: &STrack) -> f64 {  
        let v = track.kalman_velocity();  
        let norm = v.norm();  
        if norm < 1e-3 {  
            return 0.0;  
        }  
  
        let direction = v / norm;  
        let dot = direction.dot(self.bg_direction);  
        // 2D 叉积  
        let det = direction.x * self.bg_direction.y - direction.y * self.bg_direction.x;  
  
        det.atan2(dot).to_degrees().abs()  
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
  
// ===== 自由函数 =====  
  
/// 轨迹中心(基于检测框 tlwh),对齐 Python track_center。  
fn track_center(track: &STrack) -> Point2d {  
    let t = track.tlwh();  
    Point2d::new(  
        (t[0] + t[2] / 2.0) as f64,  
        (t[1] + t[3] / 2.0) as f64,  
    )  
}  
  
/// 预测轨迹下一位置(对齐 _predicted_center):Kalman 中心 + 速度。  
fn predicted_center(track: &STrack) -> Point2d {  
    let v = track.kalman_velocity();  
    let k = track.kalman_tlwh_pub();  
    let cx = (k[0] + k[2] / 2.0) as f64;  
    let cy = (k[1] + k[3] / 2.0) as f64;  
    Point2d::new(cx + v.x, cy + v.y)  
}  
  
/// 两个 track 检测框的 IoU(对齐 _iou)。  
fn iou(a: &STrack, b: &STrack) -> f64 {  
    let ta = a.tlwh();  
    let tb = b.tlwh();  
  
    let (ax1, ay1, aw, ah) = (ta[0] as f64, ta[1] as f64, ta[2] as f64, ta[3] as f64);  
    let (ax2, ay2) = (ax1 + aw, ay1 + ah);  
    let (bx1, by1, bw, bh) = (tb[0] as f64, tb[1] as f64, tb[2] as f64, tb[3] as f64);  
    let (bx2, by2) = (bx1 + bw, by1 + bh);  
  
    let inter_w = (ax2.min(bx2) - ax1.max(bx1)).max(0.0);  
    let inter_h = (ay2.min(by2) - ay1.max(by1)).max(0.0);  
    let inter_area = inter_w * inter_h;  
  
    inter_area / (aw * ah + bw * bh - inter_area + 1e-6)  
}  
  
/// Point2d(region 相对浮点)转为原图整数坐标点。  
fn to_point(offset: Point, p: Point2d) -> Point {  
    offset + Point::new(p.x.round() as i32, p.y.round() as i32)  
}  
  
#[cfg(debug_assertions)]  
fn debug_transparent_shapes(  
    detector: &dyn Detector,  
    tracks: &[STrack],  
    region: Rect,  
    cursor: Point2d,  
    bg_direction: Point2d,  
) {  
    use opencv::core::MatTraitConst;  
  
    use crate::debug::debug_shape_tracks;  
  
    debug_shape_tracks(  
        &detector.mat().roi(region).unwrap(),  
        tracks.to_vec(),  
        Point::new(cursor.x.round() as i32, cursor.y.round() as i32),  
        bg_direction,  
    );  
}
