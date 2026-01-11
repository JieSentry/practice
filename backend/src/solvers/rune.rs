use opencv::core::Rect;

#[cfg(debug_assertions)]
use crate::debug::{debug_mat, debug_spinning_arrows};
use crate::{
    bridge::KeyKind,
    detect::{Detector, SpinArrow},
};

const MAX_ARROWS: usize = 4;
const MAX_SPIN_ARROWS: usize = 2;
const MAX_CALIBRATE_COUNT: u32 = 3;

#[derive(Debug)]
pub enum SolvingState {
    Calibrating,
    Solving,
    Complete([SolvedArrow; MAX_ARROWS]),
    Error,
}

#[derive(Debug, Copy, Clone)]
pub struct SolvedArrow {
    pub key: KeyKind,
    pub bbox: Rect,
    #[cfg(debug_assertions)]
    pub is_spin: bool,
}

#[derive(Default, Copy, Clone, Debug)]
pub struct RuneSolver {
    spin_arrows: Option<[SpinArrow; MAX_SPIN_ARROWS]>,
    spin_arrows_calibrated_count: u32,
    spin_arrows_calibrated: bool,
    #[cfg(debug_assertions)]
    debugging: bool,
}

impl RuneSolver {
    #[cfg(debug_assertions)]
    pub fn debug() -> Self {
        Self {
            #[cfg(debug_assertions)]
            debugging: true,
            ..Default::default()
        }
    }

    pub fn solve(&mut self, detector: &dyn Detector) -> SolvingState {
        if !self.spin_arrows_calibrated {
            self.calibrate_for_spin_arrows(detector);

            return if self.spin_arrows_calibrated {
                SolvingState::Solving
            } else {
                SolvingState::Calibrating
            };
        }

        // After calibration is complete and there are spin arrows, prioritize its detection
        if self.detect_spin_arrows_if_calibrated(detector) {
            return SolvingState::Solving;
        }

        let ignore = self
            .spin_arrows
            .as_ref()
            .into_iter()
            .flatten()
            .map(|arrow| arrow.region)
            .collect();
        let mut arrows = detector
            .detect_rune_arrows(ignore)
            .into_iter()
            .map(|arrow| SolvedArrow {
                key: arrow.key,
                bbox: arrow.region,
                #[cfg(debug_assertions)]
                is_spin: false,
            })
            .collect::<Vec<_>>();

        if let Some(spin_arrows) = self.spin_arrows.take() {
            for arrow in spin_arrows {
                arrows.push(SolvedArrow {
                    key: arrow.final_arrow.unwrap(),
                    bbox: arrow.region,
                    #[cfg(debug_assertions)]
                    is_spin: true,
                });
            }
            arrows.sort_by_key(|arrow| arrow.bbox.x);
        }

        if arrows.len() == MAX_ARROWS {
            #[cfg(debug_assertions)]
            if self.debugging {
                debug_mat(
                    "Result",
                    &detector.mat(),
                    0,
                    arrows
                        .iter()
                        .map(|arrow| (arrow.bbox, arrow.key.to_string()))
                        .collect::<Vec<_>>(),
                );
            }

            SolvingState::Complete(arrows.try_into().unwrap())
        } else {
            SolvingState::Error
        }
    }

    fn detect_spin_arrows_if_calibrated(&mut self, detector: &dyn Detector) -> bool {
        let Some(spin_arrows) = self.spin_arrows.as_mut() else {
            return false;
        };

        if spin_arrows.iter().all(|arrow| arrow.final_arrow.is_some()) {
            return false;
        }

        for spin_arrow in spin_arrows
            .iter_mut()
            .filter(|arrow| arrow.final_arrow.is_none())
        {
            *spin_arrow = detector.detect_rune_spin_arrow(*spin_arrow);

            #[cfg(debug_assertions)]
            if self.debugging {
                debug_spinning_arrows(&detector.mat(), *spin_arrow);
            }
        }

        true
    }

    fn calibrate_for_spin_arrows(&mut self, detector: &dyn Detector) {
        assert!(!self.spin_arrows_calibrated);

        self.spin_arrows_calibrated_count += 1;
        self.spin_arrows_calibrated = self.spin_arrows_calibrated_count >= MAX_CALIBRATE_COUNT;

        let spin_arrows = detector.detect_rune_initial_spin_arrows();
        if spin_arrows.is_empty() {
            self.spin_arrows_calibrated = true;
            return;
        }

        if spin_arrows.len() != MAX_SPIN_ARROWS {
            return;
        }

        self.spin_arrows_calibrated = true;
        self.spin_arrows = Some(spin_arrows.try_into().unwrap());

        #[cfg(debug_assertions)]
        if self.debugging {
            debug_mat(
                "Spin Arrow Regions",
                &detector.mat(),
                0,
                self.spin_arrows
                    .iter()
                    .flatten()
                    .map(|arrow| (arrow.region, "Region".to_string()))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
