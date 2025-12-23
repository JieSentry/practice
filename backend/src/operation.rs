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

/// Current operating state of the bot.
#[derive(Debug, Clone, Copy)]
pub enum Operation {
    HaltUntil {
        instant: Instant,
        config: OperationConfiguration,
    },
    TemporaryHalting {
        resume: Duration,
        config: OperationConfiguration,
    },
    Halting,
    Running,
    RunUntil {
        instant: Instant,
        config: OperationConfiguration,
    },
}

impl Operation {
    #[inline]
    pub fn halting(&self) -> bool {
        matches!(
            self,
            Operation::Halting | Operation::HaltUntil { .. } | Operation::TemporaryHalting { .. }
        )
    }

    #[inline]
    pub fn halt_until(config: OperationConfiguration) -> Operation {
        Operation::HaltUntil {
            instant: Instant::now() + Duration::from_millis(config.stop_duration_millis),
            config,
        }
    }

    #[inline]
    pub fn run_until(config: OperationConfiguration) -> Operation {
        Operation::RunUntil {
            instant: Instant::now() + Duration::from_millis(config.run_duration_millis),
            config,
        }
    }

    pub fn update_tick(self) -> Operation {
        let now = Instant::now();
        match self {
            Operation::HaltUntil { instant, config } => {
                if now < instant {
                    self
                } else {
                    Operation::run_until(config)
                }
            }
            Operation::RunUntil { instant, config } => {
                if now < instant {
                    self
                } else if matches!(config.mode, CycleRunStopMode::Once) {
                    Operation::Halting
                } else {
                    Operation::halt_until(config)
                }
            }
            Operation::Halting | Operation::TemporaryHalting { .. } | Operation::Running => self,
        }
    }
}

impl Display for Operation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Operation::HaltUntil { instant, .. } => {
                write!(f, "Halting for {}", duration_from_instant(instant))
            }
            Operation::TemporaryHalting { resume, .. } => write!(
                f,
                "Halting temporarily with {} remaining",
                duration_from(resume)
            ),
            Operation::Halting => write!(f, "Halting"),
            Operation::Running => write!(f, "Running"),
            Operation::RunUntil { instant, .. } => {
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
