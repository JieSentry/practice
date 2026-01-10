use opencv::core::Point;
use opencv::core::Point2d;
use opencv::core::Rect;
use opencv::core::Scalar;
use opencv::core::Size;
use opencv::core::ToInputArray;
use opencv::core::{MatTraitConst, Vector};
use opencv::highgui::destroy_window;
use opencv::highgui::{imshow, wait_key};
use opencv::imgproc::arrowed_line;
use opencv::imgproc::draw_contours_def;
use opencv::imgproc::polylines;
use opencv::imgproc::rectangle;
use opencv::imgproc::{FONT_HERSHEY_SIMPLEX, put_text_def};
use opencv::imgproc::{LINE_8, circle_def};
use rand::distr::{Alphanumeric, SampleString};

use crate::bridge::KeyKind;
use crate::detect::ArrowsComplete;
use crate::tracker::STrack;
use crate::utils::{self, DatasetDir};

pub fn debug_spinning_arrows(
    mat: &impl MatTraitConst,
    arrow_curve: &Vector<Point>,
    arrow_contours: &Vector<Vector<Point>>,
    arrow_region: Rect,
    last_arrow_head: Point,
    cur_arrow_head: Point,
    region_centroid: Point,
) {
    let mut mat = mat.try_clone().unwrap();
    let curve = arrow_curve
        .clone()
        .into_iter()
        .map(|point| point + arrow_region.tl())
        .collect::<Vector<Point>>();
    let contours = arrow_contours
        .clone()
        .into_iter()
        .map(|points| {
            points
                .into_iter()
                .map(|pt| pt + arrow_region.tl())
                .collect::<Vector<Point>>()
        })
        .collect::<Vector<Vector<Point>>>();

    let _ = draw_contours_def(&mut mat, &contours, 0, Scalar::new(255.0, 0.0, 0.0, 0.0));
    let _ = circle_def(
        &mut mat,
        last_arrow_head + region_centroid,
        3,
        Scalar::new(0.0, 255.0, 0.0, 0.0),
    );
    let _ = circle_def(
        &mut mat,
        cur_arrow_head + region_centroid,
        3,
        Scalar::new(255.0, 0.0, 0.0, 0.0),
    );
    let _ = circle_def(
        &mut mat,
        region_centroid,
        3,
        Scalar::new(0.0, 0.0, 255.0, 0.0),
    );
    let _ = polylines(
        &mut mat,
        &curve,
        true,
        Scalar::new(0., 255., 0., 0.),
        1,
        LINE_8,
        0,
    );

    debug_mat("Spin Arrow", &mat, 0, &[]);
}

#[allow(unused)]
pub fn debug_tracks(
    mat: &impl MatTraitConst,
    tracks: Vec<STrack>,
    cursor: Point,
    bg_direction: Point2d,
) {
    fn signed_angle_deg(a: Point2d, b: Point2d) -> f64 {
        let dot = a.dot(b);
        let det = a.cross(b);
        det.atan2(dot).to_degrees()
    }

    fn mid_point(rect: Rect) -> Point {
        rect.tl() + Point::new(rect.width / 2, rect.height / 2)
    }

    let arrows = tracks
        .iter()
        .filter_map(|track| {
            if track.tracklet_len() <= 1 {
                return None;
            }

            let center = mid_point(track.rect()).to::<f64>().unwrap();
            let (vx, vy) = track.kalman_velocity();
            let v = Point2d::new(vx as f64, vy as f64);

            if v.norm() < 1e-3 {
                return None;
            }

            let angle = signed_angle_deg(v, bg_direction);

            let end = center + v * 5.0;

            Some((center.to::<i32>().unwrap(), end.to::<i32>().unwrap(), angle))
        })
        .collect::<Vec<_>>();

    let bboxes = tracks
        .into_iter()
        .map(|track| (track.kalman_rect(), format!("Track {}", track.track_id())))
        .collect::<Vec<_>>();

    let mut mat = mat.try_clone().unwrap();
    let arrow_start = Point::new(mat.cols() / 2, mat.rows() / 2);
    let arrow_end = Point::new(
        (arrow_start.x as f64 + bg_direction.x * 60.0) as i32,
        (arrow_start.y as f64 + bg_direction.y * 60.0) as i32,
    );

    let _ = circle_def(&mut mat, cursor, 3, Scalar::new(0.0, 0.0, 255.0, 0.0));
    let _ = arrowed_line(
        &mut mat,
        arrow_start,
        arrow_end,
        Scalar::new(255.0, 0.0, 0.0, 0.0),
        2,
        LINE_8,
        0,
        0.25,
    );

    for (arrow_start, arrow_end, angle) in arrows {
        let abs_angle = angle.abs();

        // green = aligned, yellow = diagonal, red = opposite
        let color = if abs_angle <= 45.0 {
            Scalar::new(0.0, 255.0, 0.0, 0.0)
        } else if abs_angle <= 90.0 {
            Scalar::new(0.0, 255.0, 255.0, 0.0)
        } else {
            Scalar::new(0.0, 0.0, 255.0, 0.0)
        };

        let _ = arrowed_line(&mut mat, arrow_start, arrow_end, color, 2, LINE_8, 0, 0.25);

        let label = format!("{:+.0}", angle);
        let _ = put_text_def(
            &mut mat,
            &label,
            arrow_end + Point::new(3, -3),
            FONT_HERSHEY_SIMPLEX,
            0.45,
            color,
        );
    }

    for (bbox, text) in bboxes {
        let _ = rectangle(
            &mut mat,
            bbox,
            Scalar::new(255.0, 0.0, 0.0, 0.0),
            1,
            LINE_8,
            0,
        );
        let _ = put_text_def(
            &mut mat,
            &text,
            bbox.tl() - Point::new(0, 10),
            FONT_HERSHEY_SIMPLEX,
            0.9,
            Scalar::new(0.0, 255.0, 0.0, 0.0),
        );
    }

    imshow("Tracks", &mat).unwrap();
    wait_key(1).unwrap();
}

pub fn debug_mat(
    name: &str,
    mat: &impl MatTraitConst,
    wait_ms: i32,
    bboxes: &[(Rect, &str)],
) -> i32 {
    let mut mat = mat.try_clone().unwrap();
    for (bbox, text) in bboxes {
        let _ = rectangle(
            &mut mat,
            *bbox,
            Scalar::new(255.0, 0.0, 0.0, 0.0),
            1,
            LINE_8,
            0,
        );
        let _ = put_text_def(
            &mut mat,
            text,
            bbox.tl() - Point::new(0, 10),
            FONT_HERSHEY_SIMPLEX,
            0.9,
            Scalar::new(0.0, 255.0, 0.0, 0.0),
        );
    }
    imshow(name, &mat).unwrap();
    let result = wait_key(wait_ms).unwrap();
    if result == 81 {
        destroy_window(name).unwrap();
    }
    result
}

pub fn save_rune_for_training<T: MatTraitConst + ToInputArray>(mat: &T, result: ArrowsComplete) {
    let has_spin_arrow = result.spins.iter().any(|spin| *spin);
    let mut name = Alphanumeric.sample_string(&mut rand::rng(), 8);
    if has_spin_arrow {
        name = format!("{name}_spin");
    }
    let size = mat.size().unwrap();

    let labels = if has_spin_arrow {
        result
            .bboxes
            .into_iter()
            .enumerate()
            .filter(|(index, _)| result.spins[*index])
            .map(|(_, bbox)| to_yolo_format(0, size, bbox))
            .collect::<Vec<String>>()
            .join("\n")
    } else {
        result
            .bboxes
            .into_iter()
            .zip(result.keys)
            .map(|(bbox, arrow)| {
                let label = match arrow {
                    KeyKind::Up => 0,
                    KeyKind::Down => 1,
                    KeyKind::Left => 2,
                    KeyKind::Right => 3,
                    _ => unreachable!(),
                };
                to_yolo_format(label, size, bbox)
            })
            .collect::<Vec<String>>()
            .join("\n")
    };

    utils::save_image_to(mat, DatasetDir::Rune, format!("{name}.png"));
    utils::save_file_to(labels, DatasetDir::Rune, format!("{name}.txt"));
}

fn to_yolo_format(label: u32, size: Size, bbox: Rect) -> String {
    let x_center = bbox.x + bbox.width / 2;
    let y_center = bbox.y + bbox.height / 2;
    let x_center = x_center as f32 / size.width as f32;
    let y_center = y_center as f32 / size.height as f32;
    let width = bbox.width as f32 / size.width as f32;
    let height = bbox.height as f32 / size.height as f32;
    format!("{label} {x_center} {y_center} {width} {height}")
}
