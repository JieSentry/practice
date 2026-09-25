use log::debug;  
use opencv::{  
    core::{  
        self, Mat, MatTrait, MatTraitConst, Point, Point2d, Rect, Scalar, Size,  
        BORDER_CONSTANT, CV_8UC1, CV_32FC1, CV_32S,  
    },  
    imgproc::{  
        self, CC_STAT_AREA, COLOR_BGR2HSV_FULL, COLOR_BGRA2BGR, COLOR_BGRA2GRAY,  
        INTER_LINEAR, MORPH_CLOSE, MORPH_RECT, THRESH_BINARY,  
    },  
};  
  
use crate::detect::Detector;  
  
// ── 可调参数(必须用真实帧调) ──────────────────────────  
/// diff 后判定为"信号"的最小灰度差(0..255)。越大越严格。  
const DIFF_THRESHOLD: f64 = 8.0;  
/// 幸存连通域面积上下限(像素),滤掉噪点和过大伪影。  
const MIN_BLOB_AREA: i32 = 6;  
const MAX_BLOB_AREA: i32 = 4000;  
/// 速度 EMA 平滑系数。调大→跟得快但抖;调小→平稳但滞后。  
const VELOCITY_ALPHA: f64 = 0.7;  
/// 输出前瞻强度,补帧差半帧滞后。滞后大就调 1.5~2.0。  
const LEAD_GAIN: f64 = 1.0;  
/// 门控半径:实测质心离预测锚点的残差超过它则限幅拉回(不直接丢弃,避免跟丢)。  
const GATE_RADIUS: f64 = 60.0;  
/// 形态学核尺寸,越大合并拖尾/碎块越强。  
const MORPH_KERNEL: i32 = 5;  
/// 丢帧外推倍率。  
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
    /// 鼠标位置(绿色光标质心),仅作首帧兜底锚点。  
    last_cursor: Option<Point2d>,  
    /// 被追踪图形自身的位置(选连通域的锚点来源)。  
    last_target: Option<Point2d>,  
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
  
        // 4. 选连通域的锚点 = 目标预测位置(last_target + velocity),首帧退回鼠标  
        let anchor = self  
            .last_target  
            .map(|t| t + self.velocity)  
            .or(self.last_cursor);  
  
        // 5. 需要上一帧才能做漂移减除  
        let target = match self.prev_gray.take() {  
            Some(prev) if prev.size().ok() == cur_gray.size().ok() => {  
                extract_signal_centroid(&prev, &cur_gray, anchor)  
            }  
            _ => None,  
        };  
        self.prev_gray = Some(cur_gray);  
  
        // 6. 门控(限幅拉回,而非直接丢弃,避免跟丢)  
        let gated_target = match (target, anchor) {  
            (Some(t), Some(a)) => {  
                let d = t - a;  
                let dist = d.norm();  
                if dist > GATE_RADIUS {  
                    Some(a + d * (GATE_RADIUS / dist))  
                } else {  
                    Some(t)  
                }  
            }  
            (t, _) => t,  
        };  
  
        // 7. 有目标 -> 更新速度并前瞻返回;无目标 -> 外推  
        let cursor = match gated_target {  
            Some(t) => {  
                if let Some(last) = self.last_target {  
                    let raw_v = t - last;  
                    self.velocity =  
                        self.velocity * (1.0 - VELOCITY_ALPHA) + raw_v * VELOCITY_ALPHA;  
                }  
                self.last_target = Some(t);  
                self.miss_count = 0;  
                // 输出前瞻,补帧差半帧滞后  
                t + self.velocity * LEAD_GAIN  
            }  
            None => {  
                let last = self.last_target.or(self.last_cursor)?;  
                self.miss_count += 1;  
                if self.miss_count > MAX_MISS {  
                    return None;  
                }  
                let next = last + self.velocity * DEAD_RECKON_GAIN;  
                self.last_target = Some(next);  
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
                "shape target={:?} cursor={:?} v={:?} miss={}",  
                self.last_target, self.last_cursor, self.velocity, self.miss_count  
            );  
        }  
  
        Some(absolute)  
    }  
}  
  
/// BGRA ROI -> (去绿灰度图 CV_8UC1, 绿色光标质心[region 局部坐标])。  
fn to_gray_and_cursor(bgra: &Mat) -> Option<(Mat, Option<Point2d>)> {  
    let mut gray = Mat::default();  
    imgproc::cvt_color_def(bgra, &mut gray, COLOR_BGRA2GRAY).ok()?;  
  
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
        None  
    };  
  
    // diff 用的灰度图把绿色像素抹掉,避免污染  
    gray.set_to(&Scalar::all(0.0), &green_mask).ok()?;  
    Some((gray, cursor))  
}  
  
/// phaseCorrelate 估漂移 -> 对齐 prev -> absdiff -> threshold -> 闭运算合并 -> 连通域取质心。  
fn extract_signal_centroid(prev: &Mat, cur: &Mat, anchor: Option<Point2d>) -> Option<Point2d> {  
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
        prev, &mut aligned, &m, size, INTER_LINEAR, BORDER_CONSTANT, Scalar::all(0.0),  
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
  
    // 把拖尾/碎块闭运算合并成一整块,让质心回到几何中心  
    let kernel = imgproc::get_structuring_element_def(  
        MORPH_RECT,  
        Size::new(MORPH_KERNEL, MORPH_KERNEL),  
    )  
    .ok()?;  
    let mut closed = Mat::default();  
    imgproc::morphology_ex_def(&mask, &mut closed, MORPH_CLOSE, &kernel).ok()?;  
  
    best_centroid(&closed, anchor)  
}  
  
/// 连通域:门控半径内选面积最大块(目标本体),质心天然居中;首帧无锚点则取全局最大。  
fn best_centroid(mask: &Mat, anchor: Option<Point2d>) -> Option<Point2d> {  
    let mut labels = Mat::default();  
    let mut stats = Mat::default();  
    let mut centroids = Mat::default();  
    let n = imgproc::connected_components_with_stats(  
        mask, &mut labels, &mut stats, &mut centroids, 8, CV_32S,  
    )  
    .ok()?;  
  
    let mut best: Option<Point2d> = None;  
    let mut best_area = 0i32;  
    for i in 1..n {  
        let area = *stats.at_2d::<i32>(i, CC_STAT_AREA).ok()?;  
        if !(MIN_BLOB_AREA..=MAX_BLOB_AREA).contains(&area) {  
            continue;  
        }  
        let cx = *centroids.at_2d::<f64>(i, 0).ok()?;  
        let cy = *centroids.at_2d::<f64>(i, 1).ok()?;  
        let c = Point2d::new(cx, cy);  
  
        // 有锚点:先按门控半径过滤远处无关块  
        if let Some(a) = anchor {  
            if (c - a).norm() > GATE_RADIUS * 1.5 {  
                continue;  
            }  
        }  
        // 候选里选面积最大的块(目标本体)  
        if area > best_area {  
            best_area = area;  
            best = Some(c);  
        }  
    }  
    best  
}
