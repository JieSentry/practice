use std::{
    cell::{Ref, RefCell},
    fmt::Debug,
    rc::Rc,
};

#[cfg(test)]
use mockall::automock;
use platforms::{Window, capture::query_capture_name_window_pairs, input::InputKind};

use crate::{
    CaptureMode, InputMethod as DatabaseInputMethod, Settings,
    bridge::{Capture, Input, InputMethod, InputReceiver},
};

/// A service to handle [`Settings`]-related incoming requests.
#[cfg_attr(test, automock)]
pub trait SettingsService: Debug {
    /// Gets the current [`Settings`] in use.
    fn settings(&self) -> Ref<'_, Settings>;

    /// Updates the currently in use [`Settings`] with new `settings`.
    fn update_settings(&mut self, settings: Settings);

    /// Updates `operation`, `input`, `input_receiver` and `capture` to use the
    /// current [`Settings`].
    fn apply_settings(
        &self,
        input: &mut dyn Input,
        input_receiver: &mut dyn InputReceiver,
        capture: &mut dyn Capture,
    );

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

    /// Updates `input`, `input_receiver` and `capture` to use the currently selected [`Window`].
    fn apply_selected_window(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        capture: &mut dyn Capture,
    );
}

#[derive(Debug)]
pub struct DefaultSettingsService {
    settings: Rc<RefCell<Settings>>,
    capture_default_window: Window,
    capture_name_window_pairs: Vec<(String, Window)>,
    capture_selected_window_index: Option<usize>,
}

impl DefaultSettingsService {
    pub fn new(settings: Rc<RefCell<Settings>>) -> Self {
        // MapleStoryClass <- GMS
        // MapleStoryClassSG <- MSEA
        // MapleStoryClassTW <- TMS
        if cfg!(windows) {
            let window = Window::new("MapleStoryClass");

            return Self {
                settings,
                capture_default_window: window,
                capture_name_window_pairs: query_capture_name_window_pairs()
                    .expect("supported platform"),
                capture_selected_window_index: None,
            };
        }

        panic!("unsupported platform")
    }

    fn update_capture(&self, capture: &mut dyn Capture, forced: bool) {
        let settings = self.settings();
        if forced || capture.mode() != settings.capture_mode {
            capture.set_mode(settings.capture_mode);
            capture.set_window(self.selected_window());
        }
    }

    fn update_inputs(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        capture: &dyn Capture,
    ) {
        let settings = self.settings();
        let (window, kind) = if matches!(capture.mode(), CaptureMode::BitBltArea) {
            (capture.window(), InputKind::Foreground)
        } else {
            (self.selected_window(), InputKind::Focused)
        };

        input_rx.set_window_and_input_kind(window, kind);
        match settings.input_method {
            DatabaseInputMethod::Default => {
                input.set_method(InputMethod::Default(window, kind));
            }
            DatabaseInputMethod::Rpc => {
                input.set_method(InputMethod::Rpc(
                    window,
                    settings.input_method_rpc_server_url.clone(),
                ));
            }
        }
    }
}

impl SettingsService for DefaultSettingsService {
    fn settings(&self) -> Ref<'_, Settings> {
        self.settings.borrow()
    }

    fn update_settings(&mut self, settings: Settings) {
        *self.settings.borrow_mut() = settings;
    }

    fn apply_settings(
        &self,
        input: &mut dyn Input,
        input_receiver: &mut dyn InputReceiver,
        capture: &mut dyn Capture,
    ) {
        self.update_capture(capture, false);
        self.update_inputs(input, input_receiver, capture);
    }

    fn window_names(&self) -> Vec<String> {
        self.capture_name_window_pairs
            .iter()
            .map(|(name, _)| name)
            .cloned()
            .collect::<Vec<_>>()
    }

    fn update_windows(&mut self) {
        self.capture_name_window_pairs =
            query_capture_name_window_pairs().expect("supported platform");
    }

    fn selected_window_index(&self) -> Option<usize> {
        self.capture_selected_window_index
    }

    fn selected_window(&self) -> Window {
        self.capture_selected_window_index
            .and_then(|index| {
                self.capture_name_window_pairs
                    .get(index)
                    .map(|(_, handle)| handle)
                    .copied()
            })
            .unwrap_or(self.capture_default_window)
    }

    fn update_selected_window(&mut self, index: Option<usize>) {
        self.capture_selected_window_index = index;
    }

    fn apply_selected_window(
        &self,
        input: &mut dyn Input,
        input_rx: &mut dyn InputReceiver,
        capture: &mut dyn Capture,
    ) {
        self.update_capture(capture, true);
        self.update_inputs(input, input_rx, capture);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::bridge::{
        InputMethod as BridgeInputMethod, MockCapture, MockInput, MockInputReceiver,
    };
    use crate::{CaptureMode, CycleRunStopMode, InputMethod};

    #[test]
    fn settings_service_initialization() {
        let settings = Rc::new(RefCell::new(Settings::default()));
        let service = DefaultSettingsService::new(settings.clone());

        assert_eq!(service.selected_window_index(), None);
        assert_eq!(service.settings().input_method, InputMethod::Default);
    }

    #[test]
    fn current_handle_fallbacks_to_default() {
        let settings = Rc::new(RefCell::new(Settings::default()));
        let service = DefaultSettingsService::new(settings.clone());

        // Without selected handle index
        let default = service.capture_default_window;
        let current = service.selected_window();
        assert_eq!(current, default);
    }

    #[test]
    fn update_selected_handle_sets_index_and_updates() {
        let settings = Rc::new(RefCell::new(Settings {
            capture_mode: CaptureMode::WindowsGraphicsCapture,
            ..Default::default()
        }));
        let mut service = DefaultSettingsService::new(settings.clone());
        service.capture_name_window_pairs = vec![
            ("Foo".to_string(), Window::new("Foo")),
            ("Bar".to_string(), Window::new("Bar")),
        ];

        let mut mock_keys = MockInput::default();
        mock_keys.expect_set_method().withf(|method| match method {
            BridgeInputMethod::Rpc(_, _) => false,
            BridgeInputMethod::Default(window, kind) => {
                *window == Window::new("Bar") && matches!(kind, InputKind::Focused)
            }
        });

        let mut key_receiver = MockInputReceiver::default();
        key_receiver
            .expect_set_window_and_input_kind()
            .withf(|window, kind| {
                *window == Window::new("Bar") && matches!(kind, InputKind::Focused)
            });
        let mut capture = MockCapture::default();
        capture
            .expect_set_window()
            .withf(|window| *window == Window::new("Bar"))
            .once();
        capture
            .expect_mode()
            .once()
            .return_const(CaptureMode::WindowsGraphicsCapture);
        capture
            .expect_set_mode()
            .withf(|mode| *mode == CaptureMode::WindowsGraphicsCapture)
            .once();

        service.update_selected_window(Some(1));
        service.apply_selected_window(&mut mock_keys, &mut key_receiver, &mut capture);

        assert_eq!(service.selected_window_index(), Some(1));
        assert_eq!(service.selected_window(), Window::new("Bar"));
    }

    #[test]
    fn update_settings_replaces_state_and_updates_components() {
        let settings = Rc::new(RefCell::new(Settings::default()));
        let mut service = DefaultSettingsService::new(settings.clone());
        let new_settings = Settings {
            input_method: InputMethod::Rpc,
            input_method_rpc_server_url: "http://localhost:9000".to_string(),
            cycle_run_stop: CycleRunStopMode::Once,
            cycle_run_duration_millis: 1000,
            capture_mode: CaptureMode::WindowsGraphicsCapture,
            ..Default::default()
        };
        let mut mock_keys = MockInput::default();
        mock_keys.expect_set_method().withf(|method| match method {
            BridgeInputMethod::Rpc(window, url) => {
                *window == Window::new("MapleStoryClass") && url.as_str() == "http://localhost:9000"
            }
            BridgeInputMethod::Default(_, _) => false,
        });

        let mut key_receiver = MockInputReceiver::default();
        key_receiver
            .expect_set_window_and_input_kind()
            .withf(|window, kind| {
                *window == Window::new("MapleStoryClass") && matches!(kind, InputKind::Focused)
            });

        let mut capture = MockCapture::default();
        capture
            .expect_set_mode()
            .withf(|mode| *mode == CaptureMode::WindowsGraphicsCapture)
            .once();
        capture
            .expect_set_window()
            .withf(|window| *window == Window::new("MapleStoryClass"))
            .once();
        capture
            .expect_mode()
            .times(2)
            .return_const(CaptureMode::BitBlt);

        service.update_settings(new_settings.clone());
        service.apply_settings(&mut mock_keys, &mut key_receiver, &mut capture);

        let current = service.settings();

        assert_eq!(current.input_method, InputMethod::Rpc);
        assert_eq!(current.input_method_rpc_server_url, "http://localhost:9000");
    }

    #[test]
    fn update_settings_input_receiver_foreground() {
        let settings = Rc::new(RefCell::new(Settings::default()));
        let mut service = DefaultSettingsService::new(settings.clone());
        let new_settings = Settings {
            capture_mode: CaptureMode::BitBltArea,
            ..Default::default()
        };
        let mut mock_keys = MockInput::default();
        mock_keys.expect_set_method().once();
        let mut key_receiver = MockInputReceiver::default();
        key_receiver
            .expect_set_window_and_input_kind()
            .withf(|window, kind| {
                *window == Window::new("MapleStoryClass") && matches!(kind, InputKind::Foreground)
            });

        let mut capture = MockCapture::default();
        capture
            .expect_window()
            .once()
            .returning(|| Window::new("MapleStoryClass"));
        capture
            .expect_mode()
            .times(2)
            .return_const(CaptureMode::BitBltArea);

        service.update_settings(new_settings.clone());
        service.apply_settings(&mut mock_keys, &mut key_receiver, &mut capture);
    }
}
