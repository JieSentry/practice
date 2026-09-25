// backend/src/solvers/shape.rs —— 漂移减除 + 绿色光标闭环追踪  
use log::debug;  
use opencv::{  
    core::{  
        self, Mat, MatTrait, MatTraitConst, Point, Point2d, Rect, Scalar, Size, CV_8UC1, CV_32F,  
        CV_32FC1, BORDER_CONSTANT,  
    },  
    imgproc::{  
        self, COLOR_BGR2HSV_FULL, COLOR_BGRA2BGR, COLOR_BGRA2GRAY, INTER_LINEAR, THRESH_BINARY,  
    },  
};
  
use crate::detect::Detector;  
  
// ── 可调参数(必须用真实帧调) ──────────────────────────  
/// diff 后判定为“信号”的最小灰度差(0..255)。越大越严格。  
const DIFF_THRESHOLD: f64 = 8.0;  
/// 速度 EMA 平滑系数。  
const VELOCITY_ALPHA: f64 = 0.5;  
/// 丢帧外推倍率(降到 1.0,避免换向时过冲飞出)。  
const DEAD_RECKON_GAIN: f64 = 1.0;  
/// 连续丢帧超过此值则放弃(返回 None)。  
const MAX_MISS: u32 = 20;  
/// 密度窗口边长(px),覆盖最大图形(125~175)。噪点干扰时可降到 ~150。★需实测★  
const BOX_SIZE: i32 = 175;  
/// 密度峰值窗口内像素和(单位:像素个数)最小阈值,低于则判本帧无有效目标。★需实测★  
const MIN_WINDOW_SUM: f32 = 200.0;  
/// 单帧离群门控阈值(px):候选质心相对预测位置的最大允许跳变距离。★需实测★  
const MAX_JUMP_PX: f64 = 60.0;  
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
                extract_signal_centroid(&prev, &cur_gray)  
            }  
            _ => None,  
        };  
        self.prev_gray = Some(cur_gray);  
  
        // 5. 有目标 -> 单帧离群门控 + 更新速度;无目标/坏帧 -> 惯性外推  
        let cursor = 'pick: {  
            if let Some(t) = target {  
                // 单帧离群门控:图形匀速移动、不会瞬移。  
                // 候选质心相对预测位置(last + velocity)跳变过大 -> 判坏帧,走外推,  
                // 不污染 velocity / last_cursor,避免鼠标锁到错误目标。  
                let is_outlier = match self.last_cursor {  
                    Some(last) => (t - (last + self.velocity)).norm() > MAX_JUMP_PX,  
                    None => false, // 首帧无历史,直接接受  
                };  
                if !is_outlier {  
                    if let Some(last) = self.last_cursor {  
                        let raw_v = t - last;  
                        self.velocity =  
                            self.velocity * (1.0 - VELOCITY_ALPHA) + raw_v * VELOCITY_ALPHA;  
                    }  
                    self.last_cursor = Some(t);  
                    self.miss_count = 0;  
                    break 'pick t;  
                }  
            }  
            // 无目标 或 门控判定的坏帧:用惯性外推顶过去  
            let last = self.last_cursor?;  
            self.miss_count += 1;  
            if self.miss_count > MAX_MISS {  
                return None;  
            }  
            let next = last + self.velocity * DEAD_RECKON_GAIN;  
            self.last_cursor = Some(next);  
            next  
        };  
  
        // 6. clamp 到 region 内:越界不再返回 None 中断,而是贴到最近边界,  
        //    保证鼠标不飞出测谎仪窗口、也不中断移动。  
        let lx = (cursor.x.round() as i32).clamp(0, region.width - 1);  
        let ly = (cursor.y.round() as i32).clamp(0, region.height - 1);  
        let local = Point::new(lx, ly);  
        let absolute = region.tl() + local;
  
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
fn extract_signal_centroid(prev: &Mat, cur: &Mat) -> Option<Point2d> {
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
  
    // 背景被抵消,只剩运动方式不同的目标(碎片化 diff)  
    let mut diff = Mat::default();  
    core::absdiff(&aligned, cur, &mut diff).ok()?;  
  
    let mut mask = Mat::default();  
    imgproc::threshold(&diff, &mut mask, DIFF_THRESHOLD, 255.0, THRESH_BINARY).ok()?;  
    if mask.typ() != CV_8UC1 {  
        let mut m8 = Mat::default();  
        mask.convert_to(&mut m8, CV_8UC1, 1.0, 0.0).ok()?;  
        mask = m8;  
    }  
  
    peak_window_centroid(&mask)  
}  
  
/// 密度窗口:用 BOX_SIZE 方框滑过整张碎片 mask,找碎片最密集的窗口,  
/// 再对该窗口内的碎片求质心作为目标点(不用窗口位置,避免平顶抖动)。  
fn peak_window_centroid(mask: &Mat) -> Option<Point2d> {  
    let size = mask.size().ok()?;  
  
    // mask(0/255) -> f32,并归一到 0/1,使窗口和 = 窗口内像素个数  
    let mut mask_f = Mat::default();  
    mask.convert_to(&mut mask_f, CV_32F, 1.0 / 255.0, 0.0).ok()?;  
  
    // 盒式滤波(normalize=false): 每像素 = 以其为中心的窗口内像素和  
    let mut resp = Mat::default();  
    imgproc::box_filter(  
        &mask_f,  
        &mut resp,  
        CV_32F,  
        Size::new(BOX_SIZE, BOX_SIZE),  
        Point::new(-1, -1),  
        false,  
        BORDER_CONSTANT,  
    )  
    .ok()?;  
  
    // 找密度峰值窗口的中心位置  
    let mut min_val = 0.0f64;  
    let mut max_val = 0.0f64;  
    let mut min_loc = Point::default();  
    let mut max_loc = Point::default();  
    core::min_max_loc(  
        &resp,  
        Some(&mut min_val),  
        Some(&mut max_val),  
        Some(&mut min_loc),  
        Some(&mut max_loc),  
        &core::no_array(),  
    )  
    .ok()?; 
  
    // 窗口内碎片过少 -> 本帧无有效目标(交给上层走外推)  
    if (max_val as f32) < MIN_WINDOW_SUM {  
        return None;  
    }  
  
    // 以峰值为中心构造 ROI,并 clamp 到 mask 边界内  
    let half = BOX_SIZE / 2;  
    let x0 = (max_loc.x - half).clamp(0, size.width - 1);  
    let y0 = (max_loc.y - half).clamp(0, size.height - 1);  
    let x1 = (max_loc.x + half + 1).clamp(x0 + 1, size.width);  
    let y1 = (max_loc.y + half + 1).clamp(y0 + 1, size.height);  
    let roi_rect = Rect::new(x0, y0, x1 - x0, y1 - y0);  
  
    // 只在峰值窗口内对碎片求质心(质心由碎片真实分布决定,天然稳定居中)  
    let roi = mask.roi(roi_rect).ok()?;  
    let mm = imgproc::moments_def(&roi).ok()?;  
    if mm.m00 <= 1.0 {  
        return None;  
    }  
    let cx = mm.m10 / mm.m00 + roi_rect.x as f64;  
    let cy = mm.m01 / mm.m00 + roi_rect.y as f64;  
    Some(Point2d::new(cx, cy))  
}
