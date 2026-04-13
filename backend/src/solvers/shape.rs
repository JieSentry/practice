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
    pre_overlap_velocity: Option<Point2d>,
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
            .field("last_cursor", &self.last_cursor)    
            .field("last_velocity", &self.last_velocity)    
            .field("bg_direction", &self.bg_direction)    
            .field("merge_frames", &self.merge_frames)    
            .field("last_track_rect", &self.last_track_rect)    
            .field("pre_overlap_velocity", &self.pre_overlap_velocity)    
            .field("prev_gray", &self.prev_gray.as_ref().map(|_| "Mat(...)"))    
            .finish()    
    }    
}

impl Default for TransparentShapeSolver {    
    fn default() -> Self {    
        Self {    
            tracker: ByteTracker::new(FPS as u64, 0.25, 0.1, 0.25, IouGating::Position),  
            last_cursor: None,    
            last_velocity: None,    
            bg_direction: Point2d::default(),    
            prev_gray: None,    
            merge_frames: 0,    
            last_track_rect: None,    
            pre_overlap_velocity: None,    
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
    // 1. 捕获灰度帧  
    let curr_gray = { /* 同原来 */ };  
  
    // 2. YOLO + ByteTracker（保留，用于 Kalman 平滑和 tracklet_len 过滤）  
    let shapes = detector.detect_transparent_shapes(region);  
    let tracks = self.tracker.update(/* 同原来 */);  
  
    self.update_background_direction(&tracks);  
  
    // 3. 如果没有 last_cursor，做初始选择（同原来的 update_initial_track_if_needed 逻辑）  
    if self.last_cursor.is_none() {  
        let reference = mid_point(Rect::new(0, 0, region.width, region.height));  
        let bg = self.bg_direction;  
        let best = tracks.iter()  
            .filter(|t| t.tracklet_len() >= 3)  
            .filter(|t| track_background_degree(t, bg).map(|a| a > 45.0).unwrap_or(false))  
            .min_by(|a, b| {  
                let da = (mid_point(a.rect()) - reference).norm();  
                let db = (mid_point(b.rect()) - reference).norm();  
                da.partial_cmp(&db).unwrap()  
            })  
            .or_else(|| find_track_closest_to(reference, &tracks));  
  
        if let Some(track) = best {  
            self.last_cursor = Some(mid_point(track.rect()));  
            self.last_velocity = Some(track.kalman_velocity());  
            self.last_track_rect = Some(track.rect());  
        }  
        self.prev_gray = Some(curr_gray);  
        return self.last_cursor.map(|c| region.tl() + c);  
    }  
  
    let last_cursor = self.last_cursor.unwrap();  
    let last_velocity = self.last_velocity.unwrap_or_default();  
  
    // 4. 预测目标位置  
    let predicted = Point::new(  
        last_cursor.x + last_velocity.x.round() as i32,  
        last_cursor.y + last_velocity.y.round() as i32,  
    );  
  
    // 5. 在所有 track 中找最佳匹配：  
    //    - 必须 tracklet_len >= 3  
    //    - 速度方向与背景方向夹角 > 45°  
    //    - 距离 predicted 在合理范围内（对角线 * 1.5）  
    //    按距离 predicted 排序，选最近的  
    let search_threshold = self.last_track_rect  
        .map(|r| diag(r) * 1.5)  
        .unwrap_or(100.0);  
  
    let best_track = tracks.iter()  
        .filter(|t| t.tracklet_len() >= 3)  
        .filter(|t| {  
            track_background_degree(t, self.bg_direction)  
                .map(|angle| angle > 45.0)  
                .unwrap_or(false)  
        })  
        .filter(|t| {  
            let dist = (mid_point(t.rect()) - predicted).norm();  
            dist <= search_threshold  
        })  
        .min_by(|a, b| {  
            let da = (mid_point(a.rect()) - predicted).norm();  
            let db = (mid_point(b.rect()) - predicted).norm();  
            da.partial_cmp(&db).unwrap()  
        });  
  
    // 6. 如果找不到匹配的 track  
    let Some(best) = best_track else {  
        // 没有合适的 track：用速度外推  
        self.merge_frames += 1;  
        if self.merge_frames > FPS / 2 {  
            // 超时：重置  
            self.last_cursor = None;  
            self.last_velocity = None;  
            self.last_track_rect = None;  
            self.pre_overlap_velocity = None;  
            self.merge_frames = 0;  
            self.prev_gray = Some(curr_gray);  
            return None;  
        }  
        // 尝试光流，失败则用速度外推  
        let displacement = self.prev_gray.as_ref()  
            .and_then(|prev| compute_target_displacement(prev, &curr_gray, last_cursor, self.last_track_rect));  
        let next_cursor = if let Some(disp) = displacement {  
            let nc = last_cursor + Point::new(disp.x.round() as i32, disp.y.round() as i32);  
            Point::new(nc.x.clamp(0, region.width - 1), nc.y.clamp(0, region.height - 1))  
        } else {  
            let nc = last_cursor + Point::new(last_velocity.x.round() as i32, last_velocity.y.round() as i32);  
            Point::new(nc.x.clamp(0, region.width - 1), nc.y.clamp(0, region.height - 1))  
        };  
        self.last_cursor = Some(next_cursor);  
        // 不更新 last_velocity 和 last_track_rect  
        self.prev_gray = Some(curr_gray);  
        return Some(region.tl() + next_cursor);  
    };  
  
    // 7. 找到了最佳 track，检查它是否与其他 track 重叠  
    let best_rect = best.rect();  
    let is_overlapping = tracks.iter().any(|t| {  
        // 排除自身（用位置判断，因为不用 track_id）  
        let t_rect = t.rect();  
        t_rect != best_rect && (best_rect & t_rect).area() > 0  
    });  
  
    if is_overlapping {  
        // ===== 重叠模式 =====  
        // 保存进入重叠前的速度（只在第一帧保存）  
        if self.merge_frames == 0 {  
            self.pre_overlap_velocity = self.last_velocity;  
        }  
        self.merge_frames += 1;  
  
        // 用光流或速度外推，不信任 track 位置  
        let displacement = self.prev_gray.as_ref()  
            .and_then(|prev| compute_target_displacement(prev, &curr_gray, last_cursor, self.last_track_rect));  
        let vel = self.pre_overlap_velocity.unwrap_or(last_velocity);  
        let next_cursor = if let Some(disp) = displacement {  
            let nc = last_cursor + Point::new(disp.x.round() as i32, disp.y.round() as i32);  
            Point::new(nc.x.clamp(0, region.width - 1), nc.y.clamp(0, region.height - 1))  
        } else {  
            let nc = last_cursor + Point::new(vel.x.round() as i32, vel.y.round() as i32);  
            Point::new(nc.x.clamp(0, region.width - 1), nc.y.clamp(0, region.height - 1))  
        };  
        self.last_cursor = Some(next_cursor);  
        // 不更新 last_velocity 和 last_track_rect（保留重叠前的值）  
        self.prev_gray = Some(curr_gray);  
        return Some(region.tl() + next_cursor);  
    }  
  
    // ===== 正常模式：track 没有重叠 =====  
    self.merge_frames = 0;  
    self.pre_overlap_velocity = None;  
  
    let next_cursor = predicted_center(best);  
    self.last_cursor = Some(next_cursor);  
    self.last_velocity = Some(best.kalman_velocity());  
    self.last_track_rect = Some(best.rect());  
    self.prev_gray = Some(curr_gray);  
  
    Some(region.tl() + next_cursor)  
}
  
    fn update_background_direction(&mut self, tracks: &[STrack]) {  
        if let Some(direction) = estimate_background_direction(self.last_cursor, tracks)  
            .and_then(|direction| unit(self.bg_direction * 0.5 + direction * 0.5))  
        {  
            self.bg_direction = direction;  
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
if *status.at::<u8>(i as i32).ok()? != 1 {
        continue;  
    }  
let next = *next_pts.at::<Point2f>(i as i32).ok()?;
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
