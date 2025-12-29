use std::{collections::HashMap, ops::Div};

use log::debug;
use opencv::core::{Point, Point_, Point2d, Rect};

use crate::{
    bridge::MouseKind,
    ecs::{Resources, transition, transition_if, try_ok_transition},
    player::{
        Player, PlayerAction, PlayerContext, PlayerEntity, next_action,
        timeout::{Lifecycle, Timeout, next_timeout_lifecycle},
        transition_from_action,
    },
    tracker::{ByteTracker, Detection, STrack},
};

type SpatialCell = (i32, i32);

struct SpatialGrid<'a> {
    grid: HashMap<SpatialCell, Vec<&'a STrack>>,
    cell_size: i32,
}

impl<'a> SpatialGrid<'a> {
    fn new(tracks: &'a [STrack]) -> Self {
        let mut grid = HashMap::new();
        let cell_size = median_bbox_diagonal(tracks) as i32;
        for track in tracks {
            let center = mid_point(track.rect());
            let cell = (center.x / cell_size, center.y / cell_size);
            grid.entry(cell).or_insert_with(Vec::new).push(track);
        }

        Self { grid, cell_size }
    }

    fn nearby_tracks(&self, point: Point) -> impl Iterator<Item = &'a STrack> {
        let (cx, cy) = self.cell_of(point);

        (-1..=1).flat_map(move |dx| {
            (-1..=1).flat_map(move |dy| {
                self.grid
                    .get(&(cx + dx, cy + dy))
                    .into_iter()
                    .flatten()
                    .copied()
            })
        })
    }

    fn cell_of(&self, point: Point) -> SpatialCell {
        (point.x / self.cell_size, point.y / self.cell_size)
    }
}

fn median_bbox_diagonal(tracks: &[STrack]) -> f64 {
    let mut diags: Vec<f64> = tracks
        .iter()
        .map(|track| {
            let bbox = track.rect();
            let point = Point2d::new(bbox.width as f64, bbox.height as f64);
            point.norm()
        })
        .collect();

    if diags.is_empty() {
        return 100.0;
    }

    diags.sort_by(|a, b| a.partial_cmp(b).unwrap());
    diags[diags.len() / 2]
}

/// Representing the current state of transparent shape (e.g. lie detector) solving.
#[derive(Debug, Clone, Copy, Default)]
pub enum State {
    #[default]
    Waiting,
    Solving(Timeout),
    Completed,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SolvingShape {
    state: State,
    lie_detector_region: Option<Rect>,
    current_track_id: Option<u64>,
    candidate_track_id: Option<u64>,
    candidate_track_count: u32,
    last_cursor: Option<Point>,
    bg_direction: Point2d,
    bg_velocity: Point2d,
}

/// Updates the [`Player::SolvingShape`] contextual state.
///
/// Note: This state does not use any [`Task`], so all detections are blocking. But this should be
/// acceptable for this state.
pub fn update_solving_shape_state(resources: &Resources, player: &mut PlayerEntity) {
    let Player::SolvingShape(mut solving_shape) = player.state else {
        panic!("state is not solving shape");
    };

    match solving_shape.state {
        State::Waiting => update_waiting(resources, &mut player.context, &mut solving_shape),
        State::Solving(_) => update_solving(
            resources,
            player.context.shape_tracker(),
            &mut solving_shape,
        ),
        State::Completed => unreachable!(),
    }

    let player_next_state = if matches!(solving_shape.state, State::Completed) {
        Player::Idle
    } else {
        Player::SolvingShape(solving_shape)
    };

    match next_action(&player.context) {
        Some(PlayerAction::SolveShape) => transition_from_action!(
            player,
            player_next_state,
            matches!(player_next_state, Player::Idle)
        ),
        Some(_) => unreachable!(),
        None => transition!(player, Player::Idle), // Force cancel if not from action
    }
}

fn update_waiting(
    resources: &Resources,
    player_context: &mut PlayerContext,
    solving_shape: &mut SolvingShape,
) {
    const CHECK_INTERVAL: u64 = 30;

    let State::Waiting = solving_shape.state else {
        panic!("solving shape state is not waiting")
    };

    if !resources.tick.is_multiple_of(CHECK_INTERVAL) {
        return;
    }
    if resources.detector().detect_lie_detector_preparing() {
        return;
    }

    let title = try_ok_transition!(
        solving_shape,
        State::Completed,
        resources.detector().detect_lie_detector()
    );

    transition!(solving_shape, State::Solving(Timeout::default()), {
        let tl = title.tl();
        let br = title.br() + Point::new(660, 530);
        let region = Rect::from_points(tl, br);
        player_context.reset_shape_tracker();
        solving_shape.lie_detector_region = Some(region);
        debug!(target: "player", "lie detector transparent shape region: {region:?}");
    });
}

fn update_solving(
    resources: &Resources,
    tracker: &mut ByteTracker,
    solving_shape: &mut SolvingShape,
) {
    const CHECK_INTERVAL: u64 = 30;

    let State::Solving(timeout) = solving_shape.state else {
        panic!("solving shape state is not solving")
    };

    if resources.tick.is_multiple_of(CHECK_INTERVAL) {
        transition_if!(
            solving_shape,
            State::Completed,
            resources.detector().detect_lie_detector().is_err()
        );
    }

    match next_timeout_lifecycle(timeout, 545) {
        Lifecycle::Ended => transition!(solving_shape, State::Completed),
        Lifecycle::Started(timeout) | Lifecycle::Updated(timeout) => {
            transition!(solving_shape, State::Solving(timeout), {
                perform_solving(resources, tracker, solving_shape);
            })
        }
    }
}

fn perform_solving(
    resources: &Resources,
    tracker: &mut ByteTracker,
    solving_shape: &mut SolvingShape,
) {
    let region = solving_shape.lie_detector_region.expect("has region");
    let shapes = resources.detector().detect_transparent_shapes(region);
    let tracks = tracker.update(shapes.into_iter().map(Detection::new).collect());

    if solving_shape.current_track_id.is_none() {
        let region_mid = mid_point(Rect::new(0, 0, region.width, region.height));
        if let Some(track) = find_track_closest_to(region_mid, &tracks) {
            solving_shape.current_track_id = Some(track.track_id());
            solving_shape.last_cursor = Some(mid_point(track.rect()));
        }
    }

    if let Some((direction, velocity)) = estimate_background_direction_velocity(&tracks) {
        solving_shape.bg_direction = direction;
        solving_shape.bg_velocity = velocity;
    }

    match select_best_track(solving_shape, &tracks) {
        Some(track) => {
            let next_cursor = predicted_center(track);
            let absolute_next_cursor = next_cursor + region.tl();
            if solving_shape.current_track_id != Some(track.track_id()) {
                debug!(target: "player", "shape id switches from {:?} to {}", solving_shape.current_track_id, track.track_id());
            }
            resources.input.send_mouse(
                absolute_next_cursor.x,
                absolute_next_cursor.y,
                MouseKind::Move,
            );
            solving_shape.current_track_id = Some(track.track_id());
            solving_shape.last_cursor = Some(next_cursor);

            #[cfg(debug_assertions)]
            #[cfg(feature = "debug_transparent_shape")]
            debug_transparent_shapes(resources, solving_shape, &tracks);
        }
        None => {
            let Some(last_cursor) = solving_shape.last_cursor else {
                return;
            };
            let scaled = solving_shape.bg_velocity * 0.7;
            let next_cursor =
                last_cursor + Point::new(-scaled.x.round() as i32, -scaled.y.round() as i32);
            let absolute_next_cursor = next_cursor + region.tl();
            resources.input.send_mouse(
                absolute_next_cursor.x,
                absolute_next_cursor.y,
                MouseKind::Move,
            );
            solving_shape.last_cursor = Some(next_cursor);

            #[cfg(debug_assertions)]
            #[cfg(feature = "debug_transparent_shape")]
            debug_transparent_shapes(resources, solving_shape, &tracks);
        }
    }
}

#[cfg(debug_assertions)]
#[cfg(feature = "debug_transparent_shape")]
fn debug_transparent_shapes(
    resources: &Resources,
    solving_shape: &SolvingShape,
    tracks: &[STrack],
) {
    use opencv::core::MatTraitConst;

    use crate::debug::debug_tracks;

    debug_tracks(
        &resources
            .detector()
            .mat()
            .roi(solving_shape.lie_detector_region.unwrap())
            .unwrap(),
        tracks.to_vec(),
        solving_shape.last_cursor.unwrap(),
        solving_shape.bg_direction,
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

fn select_best_track<'a>(
    solving_shape: &mut SolvingShape,
    tracks: &'a [STrack],
) -> Option<&'a STrack> {
    let current_track_id = solving_shape.current_track_id?;
    let bg_direction = solving_shape.bg_direction;
    let match_track = tracks
        .iter()
        .filter(|track| track.tracklet_len() >= 3 && track.track_id() != current_track_id)
        .filter_map(|track| {
            let dot = track_background_dot(track, bg_direction)?;
            if dot >= 0.2 {
                return None;
            }

            Some((track, dot))
        })
        .min_by(|(_, a_dot), (_, b_dot)| a_dot.partial_cmp(b_dot).unwrap())
        .map(|(track, _)| track)
        .or_else(|| find_common_approaching_track(tracks, current_track_id, bg_direction));
    if let Some(track) = match_track {
        if solving_shape.candidate_track_id == Some(track.track_id()) {
            solving_shape.candidate_track_count += 1;
        } else {
            solving_shape.candidate_track_id = Some(track.track_id());
            solving_shape.candidate_track_count = 0;
        }

        if solving_shape.candidate_track_count >= 1 {
            solving_shape.candidate_track_id = None;
            solving_shape.candidate_track_count = 0;
            return Some(track);
        }
    }

    tracks
        .iter()
        .find(|track| track.track_id() == current_track_id)
}

fn mid_point(rect: Rect) -> Point {
    rect.tl() + Point::new(rect.width / 2, rect.height / 2)
}

fn predicted_center(track: &STrack) -> Point {
    let (vx, vy) = track.kalman_velocity();
    let point = mid_point(track.kalman_rect());

    Point::new(
        (point.x as f32 + vx).round() as i32,
        (point.y as f32 + vy).round() as i32,
    )
}

fn track_background_dot(track: &STrack, bg_direction: Point2d) -> Option<f64> {
    let history = track.rect_history();
    let len = history.len();
    if len < 2 {
        return None;
    }

    let window = 4.min(len - 1);
    let start = len - 1 - window;
    let mut displacement = Point2d::new(0.0, 0.0);
    for i in (start + 1)..len {
        let prev = mid_point(history[i - 1]);
        let curr = mid_point(history[i]);
        displacement += (curr - prev).to::<f64>().unwrap();
    }

    Some(unit(displacement)?.dot(bg_direction))
}

fn estimate_background_direction_velocity(tracks: &[STrack]) -> Option<(Point2d, Point2d)> {
    let filtered = tracks
        .iter()
        .map(track_velocity)
        .filter(|velocity| velocity.norm() >= 2.0)
        .collect::<Vec<Point2d>>();
    if filtered.len() < 3 {
        return None;
    }

    let len = filtered.len() as f64;
    let velocity_sum = filtered
        .into_iter()
        .fold(Point2d::default(), |acc, v| acc + v);
    let velocity_unit = unit(velocity_sum)?;

    Some((velocity_unit, velocity_sum / len))
}

fn track_velocity(track: &STrack) -> Point2d {
    let (vx, vy) = track.kalman_velocity();
    Point2d::new(vx as f64, vy as f64)
}

fn find_common_approaching_track(
    tracks: &[STrack],
    current_track_id: u64,
    bg_direction: Point2d,
) -> Option<&STrack> {
    let grid = SpatialGrid::new(tracks);
    let mut votes = HashMap::<u64, u32>::new();
    for a in tracks.iter().filter(|t| t.track_id() != current_track_id) {
        let a_center = mid_point(a.rect());

        for b in grid.nearby_tracks(a_center) {
            if a.track_id() == b.track_id() {
                continue;
            }
            if b.track_id() == current_track_id {
                continue;
            }

            if !are_tracks_closing_distance(a, b) {
                continue;
            }
            if !are_tracks_direction_against_background(a, b, bg_direction) {
                continue;
            }

            *votes.entry(a.track_id()).or_insert(0) += 1;
        }
    }

    votes
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .inspect(|(id, count)| {
            debug!(target: "player", "solve shape common approaching track {id} {count}");
        })
        .map(|(id, _)| id)
        .map(|id| {
            tracks
                .iter()
                .find(|track| track.track_id() == id)
                .expect("has track")
        })
}

fn are_tracks_direction_against_background(a: &STrack, b: &STrack, bg_direction: Point2d) -> bool {
    let va = track_velocity(a);
    let Some(va_unit) = unit(va) else {
        return false;
    };

    let ab = (mid_point(b.rect()) - mid_point(a.rect()))
        .to::<f64>()
        .unwrap();
    let Some(ab_unit) = unit(ab) else {
        return false;
    };

    if va_unit.dot(ab_unit) <= 0.0 {
        return false;
    }
    if ab_unit.dot(bg_direction) >= -0.5 {
        return false;
    }

    true
}

fn are_tracks_closing_distance(a: &STrack, b: &STrack) -> bool {
    let a_history = a.rect_history();
    let b_history = b.rect_history();
    if a_history.len() < 2 || b_history.len() < 2 {
        return false;
    }

    let a_prev = mid_point(a_history[a_history.len() - 2]);
    let a_curr = mid_point(*a_history.last().unwrap());
    let b_prev = mid_point(b_history[b_history.len() - 2]);
    let b_curr = mid_point(*b_history.last().unwrap());

    let prev_dist = (a_prev - b_prev).norm();
    let curr_dist = (a_curr - b_curr).norm();

    prev_dist - curr_dist >= 5.0
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
