use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, LazyLock},
    thread::sleep,
    time::{Duration, Instant},
};

use include_dir::{Dir, include_dir};
use log::debug;
use opencv::{
    core::{Mat, MatTraitConst, ModifyInplace, Rect, Vector},
    highgui::destroy_all_windows,
    imgcodecs::{IMREAD_COLOR, imdecode},
    imgproc::{COLOR_BGR2BGRA, cvt_color_def},
    videoio::{
        CAP_PROP_FPS, VideoCapture, VideoCaptureTrait, VideoCaptureTraitConst, VideoWriter,
        VideoWriterTrait,
    },
};
use platforms::Window;
use rand::distr::SampleString;
use rand_distr::Alphanumeric;
use tokio::{
    sync::{
        broadcast::{self, Receiver, Sender},
        mpsc::{self},
    },
    task::spawn_blocking,
};

use crate::{
    CycleRunStopMode, DebugState, Settings,
    bridge::{DefaultInput, InputMethod},
    detect::{ArrowsCalibrating, ArrowsState, DefaultDetector, Detector},
    ecs::{Debug, Resources},
    mat::OwnedMat,
    models::Localization,
    notification::DiscordNotification,
    operation::{Operation, OperationConfiguration, OperationState},
    rng::Rng,
    run::FPS,
    solvers::TransparentShapeSolver,
    tracker::ByteTracker,
    utils::DatasetDir,
};

#[derive(Debug)]
pub struct DebugService {
    state: Sender<DebugState>,
    writer: Option<VideoWriter>,
}

impl Default for DebugService {
    fn default() -> Self {
        Self {
            state: broadcast::channel(1).0,
            writer: None,
        }
    }
}

impl DebugService {
    pub fn poll(&mut self, resources: &Resources) {
        if let Some(writer) = self.writer.as_mut()
            && let Some(detector) = resources.detector.as_ref()
        {
            writer.write(&detector.mat()).unwrap();
        }

        if self.state.is_empty() {
            let _ = self.state.send(DebugState {
                is_recording: self.writer.is_some(),
                is_rune_auto_saving: resources.debug.auto_save_rune(),
            });
        }
    }

    pub fn subscribe_state(&self) -> Receiver<DebugState> {
        self.state.subscribe()
    }

    pub fn set_auto_save_rune(&self, resources: &Resources, auto_save: bool) {
        resources.debug.set_auto_save_rune(auto_save);
    }

    pub fn record_video(&mut self, resources: &Resources, start: bool) {
        if !start {
            self.writer = None;
            return;
        }

        if resources.detector.is_none() {
            return;
        }

        let detector = resources.detector();
        let frame_size = detector.mat().size().unwrap();

        let id = Alphanumeric.sample_string(&mut rand::rng(), 8);
        let file = DatasetDir::Recordings.to_folder().join(format!("{id}.mp4"));
        let fourcc = VideoWriter::fourcc('H', 'V', 'C', '1').unwrap();

        let mut writer =
            VideoWriter::new(file.to_str().unwrap(), fourcc, FPS as f64, frame_size, true).unwrap();
        writer.write(&detector.mat()).unwrap();

        self.writer = Some(writer);
    }

    pub fn sandbox_test_spin_rune(&self) {
        static SPIN_TEST_DIR: Dir<'static> = include_dir!("$SPIN_TEST_DIR");
        static SPIN_TEST_IMAGES: LazyLock<Vec<Mat>> = LazyLock::new(|| {
            let mut files = SPIN_TEST_DIR.files().collect::<Vec<_>>();
            files.sort_by_key(|file| file.path().to_str().unwrap());
            files
                .into_iter()
                .map(|file| {
                    let vec = Vector::from_slice(file.contents());
                    let mut mat = imdecode(&vec, IMREAD_COLOR).unwrap();
                    unsafe {
                        mat.modify_inplace(|mat, mat_mut| {
                            cvt_color_def(mat, mat_mut, COLOR_BGR2BGRA).unwrap();
                        });
                    }
                    mat
                })
                .collect()
        });

        let localization = Arc::new(Localization::default());
        let mut calibrating = ArrowsCalibrating::default();
        calibrating.enable_spin_test();

        for mat in &*SPIN_TEST_IMAGES {
            match DefaultDetector::new(OwnedMat::from(mat.clone()), localization.clone())
                .detect_rune_arrows(calibrating)
            {
                Ok(ArrowsState::Complete(arrows)) => {
                    debug!(target: "test", "spin test completed {arrows:?}");
                    break;
                }
                Ok(ArrowsState::Calibrating(new_calibrating)) => {
                    calibrating = new_calibrating;
                }
                Err(err) => {
                    debug!(target: "test", "spin test error {err}");
                    break;
                }
            }
        }
    }

    pub fn sandbox_test_transparent_shape(&mut self) {
        static VIDEO_BYTES: &[u8] = include_bytes!(env!("TRANSPARENT_SHAPE_TEST_VIDEO"));

        let file = DatasetDir::Root
            .to_folder()
            .join("transparent_shape_test.mp4");
        if !file.exists() {
            let _ = fs::write(&file, VIDEO_BYTES);
        }

        spawn_blocking(move || {
            let mut frame_rx = frame_receiver_from_video(file);
            let mut solver = TransparentShapeSolver::debug();
            let mut tracker = ByteTracker::new(FPS);
            let mut resources = create_sandbox_test_resources();
            let localization = Arc::new(Localization::default());

            loop_with_fps(FPS, || {
                if frame_rx.is_closed() {
                    return false;
                }
                if let Ok(frame) = frame_rx.try_recv() {
                    let region = Rect::new(0, 0, frame.cols(), frame.rows());
                    let detector =
                        DefaultDetector::new(OwnedMat::from(frame), localization.clone());

                    resources.detector = Some(Arc::new(detector));
                    solver.solve(&resources, &mut tracker, region);
                }

                true
            });
            destroy_all_windows().unwrap();
        });
    }
}

fn frame_receiver_from_video(file: PathBuf) -> mpsc::Receiver<Mat> {
    let (frame_tx, frame_rx) = mpsc::channel::<Mat>(1);
    let mut capture = VideoCapture::from_file_def(file.to_str().unwrap()).unwrap();
    let fps = capture.get(CAP_PROP_FPS).unwrap();
    spawn_blocking(move || {
        loop_with_fps(fps as u32, || {
            let mut frame = Mat::default();
            if !capture.read(&mut frame).unwrap_or(false) {
                return false;
            }

            unsafe {
                frame.modify_inplace(|mat, mat_mut| {
                    cvt_color_def(mat, mat_mut, COLOR_BGR2BGRA).unwrap();
                });
            }
            let _ = frame_tx.try_send(frame);
            true
        });
    });

    frame_rx
}

fn loop_with_fps(fps: u32, mut on_tick: impl FnMut() -> bool) {
    let nanos_per_frame = (1_000_000_000 / fps) as u128;
    loop {
        let start = Instant::now();

        if !on_tick() {
            return;
        }

        let now = Instant::now();
        let elapsed_duration = now.duration_since(start);
        let elapsed_nanos = elapsed_duration.as_nanos();
        if elapsed_nanos <= nanos_per_frame {
            sleep(Duration::new(0, (nanos_per_frame - elapsed_nanos) as u32));
        }
    }
}

fn create_sandbox_test_resources() -> Resources {
    let rng = Rng::new(rand::random(), rand::random());
    let input = Box::new(DefaultInput::new(
        Window::new("Debug"),
        InputMethod::FocusedDefault,
        rng.clone(),
    ));
    let operation = Operation {
        config: OperationConfiguration {
            mode: CycleRunStopMode::None,
            run_duration_millis: 0,
            stop_duration_millis: 0,
        },
        state: OperationState::Running,
    };
    let notification = DiscordNotification::new(Rc::new(RefCell::new(Settings::default())));

    Resources {
        debug: Debug::default(),
        input,
        rng,
        notification,
        detector: None,
        operation,
        tick: 0,
    }
}
