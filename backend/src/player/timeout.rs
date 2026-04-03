use opencv::core::Point;

use super::Moving;

/// The axis to which the change in position should be detected.
#[derive(Debug)]
pub enum ChangeAxis {
    /// Detects a change in x direction.
    Horizontal,
    /// Detects a change in y direction.
    Vertical,
    /// Detects a change in both directions.
    Both,
}

/// The lifecycle of a [`Timeout`].
#[derive(Debug)]
pub enum Lifecycle {
    Started(Timeout),
    Ended,
    Updated(Timeout),
}

/// The lifecycle of a [`Timeout`] in conjunction with [`Moving`].
#[derive(Debug)]
pub enum MovingLifecycle {
    Started(Moving),
    Ended(Moving),
    Updated(Moving),
}

/// A struct that stores the current tick before timing out.
///
/// Most contextual states can be timed out as there is no guaranteed
/// an action will be performed or a state can be transitioned. So timeout is used to retry
/// such action/state and to avoid looping in a single state forever. Or
/// for some contextual states to perform an action only after timing out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timeout {
    /// The current timeout tick.
    ///
    /// If the timeout has started, in the context of movement, this can be reset to 1 .
    pub current: u32,
    /// The total number of passed ticks.
    ///
    /// Useful when [`Self::current`] can be reset. And currently only used for delaying
    /// up-jumping and stopping down key early in falling.
    pub total: u32,
    /// Indicates whether the timeout has started.
    pub started: bool,
}

impl Timeout {
    pub fn started(mut self, started: bool) -> Timeout {
        self.started = started;
        self
    }
}

/// Gets the next [`Timeout`] lifecycle.
///
/// This is basic building block for contextual states that can
/// be timed out.
#[inline]
pub fn next_timeout_lifecycle(timeout: Timeout, max_timeout: u32) -> Lifecycle {
    debug_assert!(max_timeout > 0, "max_timeout must be positive");
    debug_assert!(
        timeout.started || timeout == Timeout::default(),
        "started timeout in non-default state"
    );
    debug_assert!(
        timeout.current <= max_timeout,
        "current timeout tick larger than max_timeout"
    );

    match timeout {
        Timeout { started: false, .. } => Lifecycle::Started(Timeout {
            started: true,
            ..timeout
        }),
        Timeout { current, .. } if current >= max_timeout => Lifecycle::Ended,
        timeout => Lifecycle::Updated(Timeout {
            current: timeout.current + 1,
            total: timeout.total + 1,
            ..timeout
        }),
    }
}

/// Gets the next [`Moving`] lifecyle.
///
/// This function helps resetting the [`Timeout`] when the player's position changed
/// based on [`ChangeAxis`].
#[inline]
pub fn next_moving_lifecycle_with_axis(
    mut moving: Moving,
    cur_pos: Point,
    max_timeout: u32,
    axis: ChangeAxis,
) -> MovingLifecycle {
    if moving.timeout.current < max_timeout {
        let prev_pos = moving.pos;
        let moved = match axis {
            ChangeAxis::Horizontal => cur_pos.x != prev_pos.x,
            ChangeAxis::Vertical => cur_pos.y != prev_pos.y,
            ChangeAxis::Both => cur_pos.x != prev_pos.x || cur_pos.y != prev_pos.y,
        };

        if moved {
            moving.timeout.current = 0;
        }
    }
    moving.pos = cur_pos;

    match next_timeout_lifecycle(moving.timeout, max_timeout) {
        Lifecycle::Started(timeout) => MovingLifecycle::Started(moving.timeout(timeout)),
        Lifecycle::Ended => MovingLifecycle::Ended(moving),
        Lifecycle::Updated(timeout) => MovingLifecycle::Updated(moving.timeout(timeout)),
    }
}

#[cfg(test)]
mod tests {
    use opencv::core::Point;

    use super::*;

    fn make_timeout(current: u32, total: u32, started: bool) -> Timeout {
        Timeout {
            current,
            total,
            started,
        }
    }

    fn make_moving(pos: Point, timeout: Timeout) -> Moving {
        Moving {
            pos,
            timeout,
            dest: pos,
            exact: false,
            completed: false,
            intermediates: None,
        }
    }

    #[test]
    fn timeout_lifecycle_started() {
        let timeout = Timeout::default();
        let lifecycle = next_timeout_lifecycle(timeout, 5);
        matches!(lifecycle, Lifecycle::Started(_));
    }

    #[test]
    fn timeout_lifecycle_updated() {
        let timeout = make_timeout(2, 2, true);
        let lifecycle = next_timeout_lifecycle(timeout, 5);
        match lifecycle {
            Lifecycle::Updated(t) => {
                assert_eq!(t.current, 3);
                assert_eq!(t.total, 3);
            }
            _ => panic!("Expected Updated variant"),
        }
    }

    #[test]
    fn timeout_lifecycle_ended() {
        let timeout = make_timeout(5, 10, true);
        let lifecycle = next_timeout_lifecycle(timeout, 5);
        matches!(lifecycle, Lifecycle::Ended);
    }

    #[test]
    fn moving_lifecycle_reset_on_move() {
        let timeout = make_timeout(3, 3, true);
        let prev_pos = Point::new(0, 0);
        let cur_pos = Point::new(1, 0);
        let moving = make_moving(prev_pos, timeout);

        let lifecycle = next_moving_lifecycle_with_axis(moving, cur_pos, 5, ChangeAxis::Horizontal);
        match lifecycle {
            MovingLifecycle::Updated(m) => {
                assert_eq!(m.timeout.current, 1);
                assert_eq!(m.pos, cur_pos);
            }
            _ => panic!("Expected Started variant"),
        }
    }

    #[test]
    fn moving_lifecycle_no_move_updates_timeout() {
        let timeout = make_timeout(2, 2, true);
        let pos = Point::new(0, 0);
        let moving = make_moving(pos, timeout);

        let lifecycle = next_moving_lifecycle_with_axis(moving, pos, 5, ChangeAxis::Both);
        match lifecycle {
            MovingLifecycle::Updated(m) => {
                assert_eq!(m.timeout.current, 3);
                assert_eq!(m.timeout.total, 3);
            }
            _ => panic!("Expected Updated variant"),
        }
    }

    #[test]
    fn moving_lifecycle_timeout_expires() {
        let timeout = make_timeout(5, 5, true);
        let pos = Point::new(0, 0);
        let moving = make_moving(pos, timeout);

        let lifecycle = next_moving_lifecycle_with_axis(moving, pos, 5, ChangeAxis::Both);
        match lifecycle {
            MovingLifecycle::Ended(m) => {
                assert_eq!(m.timeout.current, 5);
                assert_eq!(m.pos, pos);
            }
            _ => panic!("Expected Ended variant"),
        }
    }
}
