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
    candidate_track_id: Option<u64>,
    candidate_track_count: u32,
    last_cursor: Option<Point>,
    last_velocity: Option<Point2d>,
    // 【移除】bg_direction 不再需要，因为不知道目标运动方向
    // 【新增】记录当前目标被连续检测到的帧数，越长越可信
    current_track_streak: u32,
    #[cfg(debug_assertions)]
    is_debugging: bool,
}

impl Default for TransparentShapeSolver {
    fn default() -> Self {
        Self {
            // 【修改1】ByteTracker参数：专门针对多假阳性场景优化
            // 提高 track_thresh 过滤低质量检测；开启 IoU 防止 ID 跳变
            tracker: ByteTracker::new(FPS as u64, 0.35, 0.5, 0.5, IouGating::IoU),
            current_track_id: None,
            candidate_track_id: None,
            candidate_track_count: 0,
            last_cursor: None,
            last_velocity: None,
            current_track_streak: 0,
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

        match self.update_and_find_best_track(&tracks, region) {
            Some(track) => {
                let next_cursor = predicted_center(track);
                if self.current_track_id != Some(track.track_id()) {
                    debug!(target: "backend/player", "shape id switches from {:?} to {}", self.current_track_id, track.track_id());
                    self.current_track_streak = 0; // 切换目标时重置计数
                } else {
                    self.current_track_streak += 1; // 没切换就增加计数
                }
                
                self.current_track_id = Some(track.track_id());
                self.last_cursor = Some(next_cursor);
                self.last_velocity = Some(track.kalman_velocity());

                #[cfg(debug_assertions)]
                if self.is_debugging {
                    // 注意：debug函数如果需要bg_direction可以传默认值，这里不再维护
                    debug_transparent_shapes(
                        detector,
                        &tracks,
                        region,
                        next_cursor,
                        Point2d::default(),
                    );
                }

                Some(region.tl() + next_cursor)
            }
            None => {
                let last_cursor = self.last_cursor?;
                let last_velocity = self.last_velocity.expect("set if last_cursor set") * 0.8;
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
                self.current_track_streak = 0; // 丢失目标时重置计数

                #[cfg(debug_assertions)]
                if self.is_debugging {
                    debug_transparent_shapes(
                        detector,
                        &tracks,
                        region,
                        next_cursor,
                        Point2d::default(),
                    );
                }

                Some(absolute_next_cursor)
            }
        }
    }

    // 【修改2】初始目标选择：只看检测分和轨迹长度，不看角度
    fn update_initial_track_if_needed(&mut self, region: Rect, tracks: &[STrack]) {
        if self.current_track_id.is_none() {
            let best_track = tracks
                .iter()
                .filter(|t| t.tracklet_len() >= 1)
                .max_by(|a, b| {
                    // 优先比检测分，检测分一样比轨迹长度
                    a.score().partial_cmp(&b.score()).unwrap()
                        .then_with(|| a.tracklet_len().cmp(&b.tracklet_len()))
                });

            if let Some(track) = best_track {
                self.current_track_id = Some(track.track_id());
                self.last_cursor = Some(mid_point(track.rect()));
                self.last_velocity = Some(track.kalman_velocity());
                self.current_track_streak = 1;
            }
        }
    }

    // 【核心修改3】彻底重写：锁死当前目标，除非它消失
    fn update_and_find_best_track<'a>(
        &mut self,
        tracks: &'a [STrack],
        region: Rect,
    ) -> Option<&'a STrack> {
        let current_track_id = self.current_track_id?;
        let last_cursor = self.last_cursor?;

        // 1. 先找当前正在跟踪的目标还在不在
        let current_track = tracks.iter().find(|t| t.track_id() == current_track_id);

        // 【核心逻辑】如果当前目标还在，直接返回它！不跟任何其他候选比分数！
        // 只要它还能被检测到，就绝不换目标
        if let Some(track) = current_track {
            self.candidate_track_id = None;
            self.candidate_track_count = 0;
            return Some(track);
        }

        // 2. 如果当前目标真的消失了，才开始选新目标
        debug!(target: "backend/player", "Current track lost, finding new candidate...");

        let scored_tracks: Vec<_> = tracks
            .iter()
            .filter(|track| track.tracklet_len() >= 1)
            .filter_map(|track| {
                let score = track_stability_score(track, last_cursor, region);
                Some((track, score))
            })
            .collect();

        let best_track_info = scored_tracks.iter().max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap());
        
        let (best_track, best_score) = match best_track_info {
            Some(info) => info,
            None => return None,
        };

        // 处理新候选的确认逻辑
        let is_same_candidate = self.candidate_track_id == Some(best_track.track_id());
        if is_same_candidate {
            self.candidate_track_count += 1;
        } else {
            self.candidate_track_id = Some(best_track.track_id());
            self.candidate_track_count = 1;
        }

        // 新候选必须连续2帧都是最高分，才确认切换
        if self.candidate_track_count >= 2 && best_score > 0.3 {
            debug!(target: "backend/player", "Locked onto new track ID: {}", best_track.track_id());
            return Some(best_track);
        }

        None
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
    _bg_direction: Point2d, // 占位，保持函数签名兼容
) {
    use opencv::core::MatTraitConst;

    use crate::debug::debug_shape_tracks;

    debug_shape_tracks(
        &detector.mat().roi(region).unwrap(),
        tracks.to_vec(),
        last_cursor,
        Point2d::default(),
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

// 【新增】纯稳定性评分函数，不涉及任何角度/背景逻辑
fn track_stability_score(
    track: &STrack,
    last_cursor: Point,
    region: Rect,
) -> f64 {
    // 1. 检测置信度分：越高越好
    let det_score = track.score();

    // 2. 轨迹长度分：活得越久越可信
    let len_score = (track.tracklet_len() as f64).min(20.0) / 20.0;

    // 3. 距离分：离上一帧光标越近越可能是目标
    let cursor_dir = mid_point(track.rect()) - last_cursor;
    let dist_squared = (cursor_dir.x.pow(2) + cursor_dir.y.pow(2)) as f64;
    let sigma = 0.3 * diag(region);
    let proximity_score = (-dist_squared / (2.0 * sigma.powi(2))).exp();

    // 综合加权
    det_score * 0.4 + len_score * 0.35 + proximity_score * 0.25
}

// 【移除】所有和背景方向、角度相关的辅助函数
// 保留 diag 和 unit 供其他地方调用

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
