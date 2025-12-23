use std::fmt::Debug;

#[cfg(test)]
use mockall::automock;
use platforms::{Window, capture::query_capture_name_window_pairs};

use crate::{CaptureMode, bridge::Capture};

/// A service to handle capture-related incoming requests.
#[cfg_attr(test, automock)]
pub trait CaptureService: Debug {
    /// Updates `capture` to use `mode`.
    fn apply_mode(&self, capture: &mut dyn Capture, mode: CaptureMode);

    /// Gets a list of [`Window`] names to be used for selection.
    ///
    /// The index of a name corresponds to a [`Window`].
    fn window_names(&self) -> Vec<String>;

    /// Updates the list available of [`Window`]s from platform.
    fn update_windows(&mut self);

    /// Gets the current selected [`Window`] index.
    fn selected_window_index(&self) -> Option<usize>;

    /// Gets the current selected [`Window`].
    ///
    /// If none is selected, the default [`Window`] is returned.
    fn selected_window(&self) -> Window;

    /// Updates the selected [`Window`] specified by `index`.
    fn update_selected_window(&mut self, index: Option<usize>);

    /// Updates `capture` to use the currently selected [`Window`].
    fn apply_selected_window(&self, capture: &mut dyn Capture);
}

#[derive(Debug)]
struct DefaultCaptureService {
    default_window: Window,
    name_window_pairs: Vec<(String, Window)>,
    selected_window_index: Option<usize>,
}

impl DefaultCaptureService {
    pub fn new() -> Self {
        // MapleStoryClass <- GMS
        // MapleStoryClassSG <- MSEA
        // MapleStoryClassTW <- TMS
        if cfg!(windows) {
            let window = Window::new("MapleStoryClass");

            return Self {
                default_window: window,
                name_window_pairs: query_capture_name_window_pairs().expect("supported platform"),
                selected_window_index: None,
            };
        }

        panic!("unsupported platform")
    }
}

impl CaptureService for DefaultCaptureService {
    fn apply_mode(&self, capture: &mut dyn Capture, mode: CaptureMode) {
        if capture.mode() != mode {
            capture.set_mode(mode);
        }
    }

    fn window_names(&self) -> Vec<String> {
        self.name_window_pairs
            .iter()
            .map(|(name, _)| name)
            .cloned()
            .collect::<Vec<_>>()
    }

    fn update_windows(&mut self) {
        self.name_window_pairs = query_capture_name_window_pairs().expect("supported platform");
    }

    fn selected_window_index(&self) -> Option<usize> {
        self.selected_window_index
    }

    fn selected_window(&self) -> Window {
        self.selected_window_index
            .and_then(|index| {
                self.name_window_pairs
                    .get(index)
                    .map(|(_, handle)| handle)
                    .copied()
            })
            .unwrap_or(self.default_window)
    }

    fn update_selected_window(&mut self, index: Option<usize>) {
        self.selected_window_index = index;
    }

    fn apply_selected_window(&self, capture: &mut dyn Capture) {
        capture.set_window(self.selected_window());
    }
}

#[cfg(test)]
mod tests {
    // TODO:
}
