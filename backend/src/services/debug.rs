use std::sync::{Arc, LazyLock};

use include_dir::{Dir, include_dir};
use log::debug;
use opencv::{
    core::{Mat, MatTraitConst, ModifyInplace, Vector},
    imgcodecs::{IMREAD_COLOR, imdecode},
    imgproc::{COLOR_BGR2BGRA, cvt_color_def},
    videoio::{VideoWriter, VideoWriterTrait},
};
use rand::distr::SampleString;
use rand_distr::Alphanumeric;
use tokio::sync::broadcast::{self, Receiver, Sender};

use crate::{
    DebugState,
    detect::{ArrowsCalibrating, ArrowsState, DefaultDetector, Detector},
    ecs::Resources,
    mat::OwnedMat,
    models::Localization,
    run::FPS,
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

    pub fn sandbox_test_transparent_shape(&self) {
        todo!()
    }
}
