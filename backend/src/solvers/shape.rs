use std::fmt;  
use std::ops::Div;  
  
use log::debug;  
use opencv::core::{Mat, MatTraitConst, Point, Point2d, Point2f, Point_, Rect, Size, TermCriteria, TermCriteria_Type};  
use opencv::video::calc_optical_flow_pyr_lk;  
  
use crate::{  
    detect::Detector,  
    run::FPS,  
    tracker::{ByteTracker, Detection, IouGating, STrack},  
};
  
pub struct TransparentShapeSolver {  
    tracker: ByteTracker,  
    current_track_id: Option<u64>,  
    candidate_track_id: Option<u64>,  
    candidate_track_count: u32,  
    last_cursor: Option<Point>,  
    last_velocity: Option<Point2d>,  
    bg_direction: Point2d,  
    // 光流相关字段  
    prev_gray: Option<Mat>,         // 上一帧灰度 ROI  
    merge_frames: u32,              // 融合模式持续帧数  
    last_track_rect: Option<Rect>,  // 上一帧目标的 bounding box  
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}

impl fmt::Debug for TransparentShapeSolver {  
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {  
        f.debug_struct("TransparentShapeSolver")  
            .field("current_track_id", &self.current_track_id)  
            .field("candidate_track_id", &self.candidate_track_id)  
            .field("candidate_track_count", &self.candidate_track_count)  
            .field("last_cursor", &self.last_cursor)  
            .field("last_velocity", &self.last_velocity)  
            .field("bg_direction", &self.bg_direction)  
            .field("merge_frames", &self.merge_frames)  
            .field("last_track_rect", &self.last_track_rect)  
            .field("prev_gray", &self.prev_gray.as_ref().map(|_| "Mat(...)"))  
            .finish()  
    }  
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
            merge_frames: 0,  
            last_track_rect: None,  
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
    // 1. 捕获当前帧灰度 ROI（用于光流）  
    let curr_gray = {  
        let roi = detector.grayscale().roi(region).ok()?;  
        let mut gray = Mat::default();  
        roi.copy_to(&mut gray).ok()?;  
        gray  
    };  
  
    // 2. 运行 YOLO + ByteTracker（与原来完全一样）  
    let shapes = detector.detect_transparent_shapes(region);  
    let tracks = self.tracker.update(  
        shapes  
            .into_iter()  
            .map(|(bbox, score)| Detection::new(bbox, score))  
            .collect(),  
    );  
  
    self.update_initial_track_if_needed(region, &tracks);  
    self.update_background_direction(&tracks);  
  
    // 3. 检查当前 track 是否存活  
    let current_track_alive = self  
        .current_track_id  
        .is_some_and(|id| tracks.iter().any(|t| t.track_id() == id));  
  
    if current_track_alive {  
        // ===== 正常模式：与原始代码完全一致 =====  
        self.merge_frames = 0;  
  
        let result = match self.update_and_find_best_track(&tracks, region) {  
            Some(track) => {  
                let next_cursor = predicted_center(track);  
                if self.current_track_id != Some(track.track_id()) {  
                    debug!(target: "backend/player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());  
                }  
                self.current_track_id = Some(track.track_id());  
                self.last_cursor = Some(next_cursor);  
                self.last_velocity = Some(track.kalman_velocity());  
                self.last_track_rect = Some(track.rect());  
  
                #[cfg(debug_assertions)]  
                if self.is_debugging {  
                    debug_transparent_shapes(  
                        detector, &tracks, region, next_cursor, self.bg_direction,  
                    );  
                }  
  
                Some(region.tl() + next_cursor)  
            }  
            None => {  
                let last_cursor = self.last_cursor?;  
                let last_velocity =  
                    self.last_velocity.expect("set if last_cursor set") * 1.5;  
                let next_cursor = last_cursor  
                    + Point::new(  
                        last_velocity.x.round() as i32,  
                        last_velocity.y.round() as i32,  
                    );  
                let absolute_next_cursor = region.tl() + next_cursor;  
                if !region.contains(absolute_next_cursor) {  
                    self.prev_gray = Some(curr_gray);  
                    return None;  
                }  
  
                self.last_cursor = Some(next_cursor);  
  
                #[cfg(debug_assertions)]  
                if self.is_debugging {  
                    debug_transparent_shapes(  
                        detector, &tracks, region, next_cursor, self.bg_direction,  
                    );  
                }  
  
                Some(absolute_next_cursor)  
            }  
        };  
  
        self.prev_gray = Some(curr_gray);  
        return result;  
    }  
  
    // ===== 融合模式：当前 track 丢失，用光流跟踪 =====  
    self.merge_frames += 1;  
  
    // 超时：融合超过 0.5 秒，重置让 update_initial_track_if_needed 重新选择  
if self.merge_frames > FPS / 2 { 
        debug!(target: "backend/player", "merge timeout after {} frames, resetting", self.merge_frames);  
        self.current_track_id = None;  
        self.last_cursor = None;  
        self.last_velocity = None;  
        self.candidate_track_id = None;  
        self.candidate_track_count = 0;  
        self.merge_frames = 0;  
        self.last_track_rect = None;  
        self.prev_gray = Some(curr_gray);  
        return None;  
    }  
  
    let last_cursor = match self.last_cursor {  
        Some(c) => c,  
        None => {  
            self.prev_gray = Some(curr_gray);  
            return None;  
        }  
    };  
  
    // 尝试用光流计算目标位移  
    let displacement = self  
        .prev_gray  
        .as_ref()  
        .and_then(|prev| {  
            compute_target_displacement(prev, &curr_gray, last_cursor, self.last_track_rect)  
        });  
  
    let next_cursor = if let Some(disp) = displacement {  
        let nc = last_cursor  
            + Point::new(disp.x.round() as i32, disp.y.round() as i32);  
        Point::new(  
            nc.x.clamp(0, region.width - 1),  
            nc.y.clamp(0, region.height - 1),  
        )  
    } else {  
        // 光流失败：用 last_velocity 温和外推（不乘 1.5）  
        let v = self.last_velocity.unwrap_or_default();  
        let nc = last_cursor + Point::new(v.x.round() as i32, v.y.round() as i32);  
        Point::new(  
            nc.x.clamp(0, region.width - 1),  
            nc.y.clamp(0, region.height - 1),  
        )  
    };  
  
    debug!(target: "backend/player",  
        "merge mode frame {}: cursor {:?} -> {:?}, displacement {:?}",  
        self.merge_frames, last_cursor, next_cursor, displacement);  
  
    self.last_cursor = Some(next_cursor);  
    // 关键：不更新 last_velocity，保留融合前的速度  
    // 也不更新 last_track_rect，保留融合前的 bbox 大小用于光流采样  
    self.prev_gray = Some(curr_gray);  
  
    #[cfg(debug_assertions)]  
    if self.is_debugging {  
        debug_transparent_shapes(  
            detector, &tracks, region, next_cursor, self.bg_direction,  
        );  
    }  
  
    Some(region.tl() + next_cursor)  
}
  
fn update_initial_track_if_needed(&mut self, region: Rect, tracks: &[STrack]) {  
    if self.current_track_id.is_none() {  
        // 优先用 last_cursor（光流维持的位置），否则用 region 中心  
        let reference_point = self.last_cursor.unwrap_or_else(|| {  
            mid_point(Rect::new(0, 0, region.width, region.height))  
        });  
        if let Some(track) = find_track_closest_to(reference_point, tracks) {  
            self.current_track_id = Some(track.track_id());  
            self.last_cursor = Some(mid_point(track.rect()));  
            self.last_velocity = Some(track.kalman_velocity());  
            self.last_track_rect = Some(track.rect());  
            self.merge_frames = 0;  
            debug!(target: "backend/player", "re-identified target as track {}", track.track_id());  
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

/// 使用稀疏光流计算目标在两帧之间的位移。  
///  
/// 在 `last_cursor` 附近（`last_rect` 范围内）生成网格点，  
/// 用 Lucas-Kanade 光流跟踪到当前帧，通过中位数分离背景运动，  
/// 返回目标的位移向量。  
fn compute_target_displacement(  
    prev_gray: &Mat,  
    curr_gray: &Mat,  
    last_cursor: Point,  
    last_rect: Option<Rect>,  
) -> Option<Point2d> {  
    let img_size = prev_gray.size().ok()?;  
  
    // 确定采样区域：使用上一帧的 bbox，或 last_cursor 周围 60x60  
    let sample_rect = last_rect.unwrap_or_else(|| {  
        Rect::new(last_cursor.x - 30, last_cursor.y - 30, 60, 60)  
    });  
  
    // Clamp 到图像边界  
    let x0 = sample_rect.x.max(0);  
    let y0 = sample_rect.y.max(0);  
    let x1 = (sample_rect.x + sample_rect.width).min(img_size.width);  
    let y1 = (sample_rect.y + sample_rect.height).min(img_size.height);  
  
    if x1 <= x0 || y1 <= y0 {  
        return None;  
    }  
  
    // 生成网格点（间距 5 像素）  
    let step = 5;  
    let mut prev_points = Vec::new();  
    let mut y = y0;  
    while y < y1 {  
        let mut x = x0;  
        while x < x1 {  
            prev_points.push(Point2f::new(x as f32, y as f32));  
            x += step;  
        }  
        y += step;  
    }  
  
    if prev_points.len() < 4 {  
        return None;  
    }  
  
    // 转换为 Mat（Nx1 的 2 通道 Mat）  
    let n = prev_points.len();  
    let prev_pts = Mat::from_slice(&prev_points).ok()?;  
    let mut next_pts = Mat::default();  
    let mut status = Mat::default();  
    let mut err = Mat::default();  
  
    let criteria = TermCriteria::new(  
        TermCriteria_Type::COUNT as i32 + TermCriteria_Type::EPS as i32,  
        30,  
        0.01,  
    )  
    .ok()?;  
  
    calc_optical_flow_pyr_lk(  
        prev_gray,  
        curr_gray,  
        &prev_pts,  
        &mut next_pts,  
        &mut status,  
        &mut err,  
        Size::new(21, 21), // 窗口大小  
        3,                  // 金字塔层数  
        criteria,  
        0,    // flags  
        1e-4, // minEigThreshold  
    )  
    .ok()?;  
  
    // 收集有效位移  
    let mut displacements = Vec::with_capacity(n);  
for (i, prev_pt) in prev_points.iter().enumerate().take(n) {  
    if *status.at::<u8>(i as i32)? != 1 {  
        continue;  
    }  
    let next = *next_pts.at::<Point2f>(i as i32)?;  
    let dx = (next.x - prev_pt.x) as f64;  
    let dy = (next.y - prev_pt.y) as f64;  
    displacements.push(Point2d::new(dx, dy));  
}
  
    if displacements.len() < 4 {  
        return None;  
    }  
  
    // 计算中位数位移 = 背景运动估计  
    // （采样区域在融合区域，多数像素属于背景形状 + 实际背景纹理）  
    let mut dx_vals: Vec<f64> = displacements.iter().map(|d| d.x).collect();  
    let mut dy_vals: Vec<f64> = displacements.iter().map(|d| d.y).collect();  
    dx_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());  
    dy_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());  
    let median_dx = dx_vals[dx_vals.len() / 2];  
    let median_dy = dy_vals[dy_vals.len() / 2];  
  
    // 找到位移与中位数显著不同的点 = 目标像素  
    let threshold = 1.5; // 像素，残差超过此值认为是目标运动  
    let target_disps: Vec<&Point2d> = displacements  
        .iter()  
        .filter(|d| {  
            let rx = d.x - median_dx;  
            let ry = d.y - median_dy;  
            (rx * rx + ry * ry).sqrt() > threshold  
        })  
        .collect();  
  
    if target_disps.is_empty() {  
        // 没有异常运动：目标可能完全被遮挡或静止  
        // 返回中位数位移（跟随融合体，保持在附近）  
        return Some(Point2d::new(median_dx, median_dy));  
    }  
  
    // 目标像素的平均位移  
    let sum = target_disps  
        .iter()  
        .fold(Point2d::default(), |acc, d| acc + **d);  
    Some(sum / target_disps.len() as f64)  
}
