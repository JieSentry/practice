use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::time::Duration;
use std::time::Instant;

use crate::models::{CycleRunStopMode, Settings};

#[derive(Debug, Clone, Copy)]
pub struct OperationConfiguration {
    pub mode: CycleRunStopMode,
    pub run_duration_millis: u64,
    pub stop_duration_millis: u64,
}

impl From<&Settings> for OperationConfiguration {
    fn from(settings: &Settings) -> Self {
        let mode = settings.cycle_run_stop;
        let run_duration_millis = settings.cycle_run_duration_millis;
        let stop_duration_millis = settings.cycle_stop_duration_millis;

        Self {
            mode,
            run_duration_millis,
            stop_duration_millis,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OperationState {
    HaltUntil { instant: Instant },
    TemporaryHalting { resume: Duration },
    Halting,
    Running,
    RunUntil { instant: Instant },
}

impl OperationState {
    #[inline]
    pub fn halt_until(config: OperationConfiguration) -> OperationState {
        OperationState::HaltUntil {
            instant: Instant::now() + Duration::from_millis(config.stop_duration_millis),
        }
    }

    #[inline]
    pub fn run_until(config: OperationConfiguration) -> OperationState {
        OperationState::RunUntil {
            instant: Instant::now() + Duration::from_millis(config.run_duration_millis),
        }
    }
}

/// Current operating state of the bot.
#[derive(Debug, Clone, Copy)]
pub struct Operation {
    pub config: OperationConfiguration,
    pub state: OperationState,
}

impl Operation {
    #[inline]
    pub fn halting(&self) -> bool {
        matches!(
            self.state,
            OperationState::Halting
                | OperationState::HaltUntil { .. }
                | OperationState::TemporaryHalting { .. }
        )
    }

    pub fn update_tick(&mut self) {
        let now = Instant::now();
        let config = self.config;
        let current_state = self.state;
        let next_state = match current_state {
            OperationState::HaltUntil { instant } => {
                if now < instant {
                    current_state
                } else {
                    OperationState::run_until(config)
                }
            }
            OperationState::RunUntil { instant } => {
                if now < instant {
                    current_state
                } else if matches!(config.mode, CycleRunStopMode::Once) {
                    OperationState::Halting
                } else {
                    OperationState::halt_until(config)
                }
            }
            OperationState::Halting
            | OperationState::TemporaryHalting { .. }
            | OperationState::Running => current_state,
        };

        self.state = next_state;
    }
}

impl Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.state {
            OperationState::HaltUntil { instant, .. } => {
                write!(f, "Halting for {}", duration_from_instant(instant))
            }
            OperationState::TemporaryHalting { resume, .. } => write!(
                f,
                "Halting temporarily with {} remaining",
                duration_from(resume)
            ),
            OperationState::Halting => write!(f, "Halting"),
            OperationState::Running => write!(f, "Running"),
            OperationState::RunUntil { instant, .. } => {
                write!(f, "Running for {}", duration_from_instant(instant))
            }
        }
    }
}

#[inline]
fn duration_from_instant(instant: Instant) -> String {
    duration_from(instant.saturating_duration_since(Instant::now()))
}

#[inline]
fn duration_from(duration: Duration) -> String {
    let seconds = duration.as_secs() % 60;
    let minutes = (duration.as_secs() / 60) % 60;
    let hours = (duration.as_secs() / 60) / 60;

    format!("{hours:0>2}:{minutes:0>2}:{seconds:0>2}")
}
