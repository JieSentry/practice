// backend/src/solvers/shape.rs —— 漂移减除 + 绿色光标闭环追踪  
use log::debug;  
use opencv::{  
    core::{  
        self, Mat, MatTrait, MatTraitConst, Point, Point2d, Rect, Scalar, CV_8UC1, CV_32FC1, CV_32S,  
        BORDER_CONSTANT,  
    },  
    imgproc::{  
        self, CC_STAT_AREA, COLOR_BGR2HSV_FULL, COLOR_BGRA2BGR, COLOR_BGRA2GRAY, INTER_LINEAR,  
        THRESH_BINARY,  
    },  
};  
  
use crate::detect::Detector;  
  
// ── 可调参数(必须用真实帧调) ──────────────────────────  
/// diff 后判定为“信号”的最小灰度差(0..255)。越大越严格。  
const DIFF_THRESHOLD: f64 = 8.0;  
/// 幸存连通域面积上下限(像素),滤掉噪点和过大伪影。  
const MIN_BLOB_AREA: i32 = 6;  
const MAX_BLOB_AREA: i32 = 4000;  
/// 速度 EMA 平滑系数。  
const VELOCITY_ALPHA: f64 = 0.5;  
/// 丢帧外推倍率(沿用旧实现的 1.5)。  
const DEAD_RECKON_GAIN: f64 = 1.5;  
/// 连续丢帧超过此值则放弃(返回 None)。  
const MAX_MISS: u32 = 20;  
/// 绿色光标 HSV 阈值(COLOR_BGR2HSV_FULL: H/S/V 均 0..255)。★占位,需实测★  
const GREEN_LO: [f64; 3] = [60.0, 60.0, 60.0];  
const GREEN_HI: [f64; 3] = [110.0, 255.0, 255.0];  
// ─────────────────────────────────────────────────────────  
  
#[derive(Debug, Default)]  
pub struct TransparentShapeSolver {  
    /// 上一帧去绿灰度图(region 尺寸, CV_8UC1)。  
    prev_gray: Option<Mat>,  
    /// 上一次光标位置(region 局部坐标)。  
    last_cursor: Option<Point2d>,  
    /// 平滑后的速度。  
    velocity: Point2d,  
    /// 连续丢帧计数。  
    miss_count: u32,  
    #[cfg(debug_assertions)]  
    is_debugging: bool,  
}  
  
impl TransparentShapeSolver {  
    #[cfg(debug_assertions)]  
    pub fn debug() -> Self {  
        let mut default = Self::default();  
        default.is_debugging = true;  
        default  
    }  
  
    pub fn solve(&mut self, detector: &dyn Detector, region: Rect) -> Option<Point> {  
        // 1. 取当前帧 BGRA ROI  
        let bgra = detector.mat().roi(region).ok()?.clone_pointee();  
  
        // 2. 去绿灰度 + 绿色光标质心(= 鼠标当前位置)  
        let (cur_gray, measured_cursor) = to_gray_and_cursor(&bgra)?;  
  
        // 3. 用实测绿色光标位置闭环校正 last_cursor  
        if let Some(c) = measured_cursor {  
            self.last_cursor = Some(c);  
        }  
  
        // 4. 需要上一帧才能做漂移减除  
        let target = match self.prev_gray.take() {  
            Some(prev) if prev.size().ok() == cur_gray.size().ok() => {  
                extract_signal_centroid(&prev, &cur_gray, self.last_cursor)  
            }  
            _ => None,  
        };  
        self.prev_gray = Some(cur_gray);  
  
        // 5. 有目标 -> 更新速度并返回;无目标 -> 外推  
        let cursor = match target {  
            Some(t) => {  
                if let Some(last) = self.last_cursor {  
                    let raw_v = t - last;  
                    self.velocity =  
                        self.velocity * (1.0 - VELOCITY_ALPHA) + raw_v * VELOCITY_ALPHA;  
                }  
                self.last_cursor = Some(t);  
                self.miss_count = 0;  
                t  
            }  
            None => {  
                let last = self.last_cursor?;  
                self.miss_count += 1;  
                if self.miss_count > MAX_MISS {  
                    return None;  
                }  
                let next = last + self.velocity * DEAD_RECKON_GAIN;  
                self.last_cursor = Some(next);  
                next  
            }  
        };  
  
        let local = Point::new(cursor.x.round() as i32, cursor.y.round() as i32);  
        let absolute = region.tl() + local;  
        if !region.contains(absolute) {  
            return None;  
        }  
  
        #[cfg(debug_assertions)]  
        if self.is_debugging {  
            debug!(  
                target: "backend/player",  
                "shape cursor(local)={local:?} v={:?} miss={}",  
                self.velocity, self.miss_count  
            );  
        }  
  
        Some(absolute)  
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
  
/// BGRA ROI -> (去绿灰度图 CV_8UC1, 绿色光标质心[region 局部坐标])。  
fn to_gray_and_cursor(bgra: &Mat) -> Option<(Mat, Option<Point2d>)> {  
    // 灰度  
    let mut gray = Mat::default();  
    imgproc::cvt_color_def(bgra, &mut gray, COLOR_BGRA2GRAY).ok()?;  
  
    // BGRA -> BGR -> HSV,取绿色 mask  
    let mut bgr = Mat::default();  
    imgproc::cvt_color_def(bgra, &mut bgr, COLOR_BGRA2BGR).ok()?;  
    let mut hsv = Mat::default();  
    imgproc::cvt_color_def(&bgr, &mut hsv, COLOR_BGR2HSV_FULL).ok()?;  
  
    let mut green_mask = Mat::default();  
    core::in_range(  
        &hsv,  
        &Scalar::new(GREEN_LO[0], GREEN_LO[1], GREEN_LO[2], 0.0),  
        &Scalar::new(GREEN_HI[0], GREEN_HI[1], GREEN_HI[2], 0.0),  
        &mut green_mask,  
    )  
    .ok()?;  
  
    // 绿色光标质心 = 鼠标当前位置  
    let m = imgproc::moments_def(&green_mask).ok()?;  
    let cursor = if m.m00 > 1.0 {  
        Some(Point2d::new(m.m10 / m.m00, m.m01 / m.m00))  
    } else {  
        None // 绿色光标不在 region 内 / 被遮挡  
    };  
  
    // diff 用的灰度图把绿色像素抹掉,避免污染  
    gray.set_to(&Scalar::all(0.0), &green_mask).ok()?;  
    Some((gray, cursor))  
}  
  
/// phaseCorrelate 估漂移 -> 对齐 prev -> absdiff -> threshold -> 连通域取质心。  
fn extract_signal_centroid(prev: &Mat, cur: &Mat, last: Option<Point2d>) -> Option<Point2d> {  
    // f32 版本用于 phaseCorrelate  
    let mut prev_f = Mat::default();  
    let mut cur_f = Mat::default();  
    prev.convert_to(&mut prev_f, CV_32FC1, 1.0, 0.0).ok()?;  
    cur.convert_to(&mut cur_f, CV_32FC1, 1.0, 0.0).ok()?;  
  
    // 背景漂移向量(cur 相对 prev 的位移)  
    let shift = imgproc::phase_correlate_def(&prev_f, &cur_f).ok()?;  
  
    // 用 shift 把 prev 平移对齐到 cur  
    let m = Mat::from_slice_2d(&[[1.0f64, 0.0, shift.x], [0.0, 1.0, shift.y]]).ok()?;  
    let size = cur.size().ok()?;  
    let mut aligned = Mat::default();  
    imgproc::warp_affine(  
        prev,  
        &mut aligned,  
        &m,  
        size,  
        INTER_LINEAR,  
        BORDER_CONSTANT,  
        Scalar::all(0.0),  
    )  
    .ok()?;  
  
    // 背景被抵消,只剩运动方式不同的目标  
    let mut diff = Mat::default();  
    core::absdiff(&aligned, cur, &mut diff).ok()?;  
  
    let mut mask = Mat::default();  
    imgproc::threshold(&diff, &mut mask, DIFF_THRESHOLD, 255.0, THRESH_BINARY).ok()?;  
    if mask.typ() != CV_8UC1 {  
        let mut m8 = Mat::default();  
        mask.convert_to(&mut m8, CV_8UC1, 1.0, 0.0).ok()?;  
        mask = m8;  
    }  
  
    best_centroid(&mask, last)  
}  
  
/// 连通域:选面积达标、且离上次光标最近的质心;首帧取最大面积。  
fn best_centroid(mask: &Mat, last: Option<Point2d>) -> Option<Point2d> {  
    let mut labels = Mat::default();  
    let mut stats = Mat::default();  
    let mut centroids = Mat::default();  
    let n = imgproc::connected_components_with_stats(  
        mask,  
        &mut labels,  
        &mut stats,  
        &mut centroids,  
        8,  
        CV_32S,  
    )  
    .ok()?;  
  
    let mut best: Option<Point2d> = None;  
    let mut best_metric = f64::MAX;  
    for i in 1..n {  
        let area = *stats.at_2d::<i32>(i, CC_STAT_AREA).ok()?;  
        if !(MIN_BLOB_AREA..=MAX_BLOB_AREA).contains(&area) {  
    continue;  
} 
        let cx = *centroids.at_2d::<f64>(i, 0).ok()?;  
        let cy = *centroids.at_2d::<f64>(i, 1).ok()?;  
        let c = Point2d::new(cx, cy);  
  
        // 有历史光标:取距离最小;否则:取面积最大(用负面积当 metric)  
        let metric = match last {  
            Some(l) => (c - l).norm(),  
            None => -(area as f64),  
        };  
        if metric < best_metric {  
            best_metric = metric;  
            best = Some(c);  
        }  
    }  
    best  
}
