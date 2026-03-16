#[cfg(test)]
use std::rc::Rc;
use std::sync::Arc;

use crate::services::Event;
#[cfg(test)]
use crate::{Settings, bridge::MockInput, detect::MockDetector};
use crate::{
    bridge::Input, buff::BuffEntities, detect::Detector, minimap::MinimapEntity,
    notification::Notification, operation::Operation, player::PlayerEntity, rng::Rng,
    skill::SkillEntities,
};
#[cfg(debug_assertions)]
use crate::{debug::save_rune_for_training, solvers::SolvedArrow};

#[derive(Debug, Default)]
#[cfg(debug_assertions)]
pub struct Debug {
    auto_save: bool,
    last_rune_detector: Option<Arc<dyn Detector>>,
    last_rune_result: Option<[SolvedArrow; 4]>,
}

#[cfg(debug_assertions)]
impl Debug {
    pub fn auto_save_rune(&self) -> bool {
        self.auto_save
    }

    pub fn set_auto_save_rune(&mut self, auto_save: bool) {
        self.auto_save = auto_save
    }

    pub fn save_last_rune_result(&self) {
        if !self.auto_save {
            return;
        }

        if let Some((detector, result)) =
            self.last_rune_detector.as_ref().zip(self.last_rune_result)
        {
            save_rune_for_training(&detector.mat(), result);
        }
    }

    pub fn set_last_rune_result(&mut self, detector: Arc<dyn Detector>, result: [SolvedArrow; 4]) {
        self.last_rune_detector = Some(detector);
        self.last_rune_result = Some(result);
    }
}

/// A struct containing shared resources.
///
/// TODO: Reduce field visibilities.
#[derive(Debug)]
pub struct Resources {
    /// A resource to hold debugging information.
    #[cfg(debug_assertions)]
    pub debug: Debug,
    /// A resource to send inputs.
    pub input: Box<dyn Input>,
    /// A resource for generating random values.
    pub rng: Rng,
    /// A resource for sending notifications through web hook.
    pub notification: Notification,
    /// A resource to detect game information.
    ///
    /// This is [`None`] when no frame as ever been captured.
    pub detector: Option<Arc<dyn Detector>>,
    /// A resource indicating current operation state.
    pub operation: Operation,
    /// A resource indicating current tick.
    pub tick: u64,
}

impl Resources {
    #[cfg(test)]
    pub fn new(input: Option<MockInput>, detector: Option<MockDetector>) -> Self {
        use std::cell::RefCell;

        use crate::operation::{OperationConfiguration, OperationState};

        Self {
            #[cfg(debug_assertions)]
            debug: Debug::default(),
            input: Box::new(input.unwrap_or_default()),
            rng: Rng::new(rand::random(), rand::random()),
            notification: Notification::new(Rc::new(RefCell::new(Settings::default()))),
            detector: detector.map(|detector| Arc::new(detector) as Arc<dyn Detector>),
            operation: Operation {
                config: OperationConfiguration {
                    run_timer: false,
                    run_timer_millis: 0,
                },
                state: OperationState::Running,
            },
            tick: 0,
        }
    }

    /// Retrieves a reference to a [`Detector`] for the latest captured frame.
    ///
    /// # Panics
    ///
    /// Panics if no frame has ever been captured.
    #[inline]
    pub fn detector(&self) -> &dyn Detector {
        self.detector
            .as_ref()
            .expect("detector is not available because no frame has ever been captured")
            .as_ref()
    }

    /// Same as [`Self::detector`] but cloned.
    #[inline]
    pub fn detector_cloned(&self) -> Arc<dyn Detector> {
        self.detector
            .as_ref()
            .cloned()
            .expect("detector is not available because no frame has ever been captured")
    }
}

/// Different game-related events.
#[derive(Debug, Clone, Copy)]
pub enum WorldEvent {
    RunTimerEnded,
    PlayerDied,
    MinimapChanged,
    CaptureFailed,
    LieDetectorShapeAppeared,
    LieDetectorViolettaAppeared,
    EliteBossAppeared,
}

impl Event for WorldEvent {}

/// A container for entities.
#[derive(Debug)]
pub struct World {
    pub minimap: MinimapEntity,
    pub player: PlayerEntity,
    pub skills: SkillEntities,
    pub buffs: BuffEntities,
}
