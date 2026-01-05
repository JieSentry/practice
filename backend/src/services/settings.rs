use std::{
    cell::{Ref, RefCell},
    fmt::Debug,
    rc::Rc,
};

use crate::Settings;

/// A service to handle [`Settings`]-related incoming requests.
pub trait SettingsService: Debug {
    /// Gets the current [`Settings`] in use.
    fn settings(&self) -> Ref<'_, Settings>;

    /// Updates the currently in use [`Settings`] with new `settings`.
    fn update_settings(&mut self, settings: Settings);
}

#[derive(Debug)]
pub struct DefaultSettingsService {
    settings: Rc<RefCell<Settings>>,
}

impl DefaultSettingsService {
    pub fn new(settings: Rc<RefCell<Settings>>) -> Self {
        Self { settings }
    }
}

impl SettingsService for DefaultSettingsService {
    fn settings(&self) -> Ref<'_, Settings> {
        self.settings.borrow()
    }

    fn update_settings(&mut self, settings: Settings) {
        *self.settings.borrow_mut() = settings;
    }
}
