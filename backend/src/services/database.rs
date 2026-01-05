use std::ops::DerefMut;

use super::EventContext;
use crate::{
    CaptureMode, DatabaseEvent, bridge::InputMethod, models::InputMethod as DatabaseInputMethod,
    operation::OperationConfiguration, services::EventHandler,
};

pub struct DatabaseEventHandler;

impl EventHandler<DatabaseEvent> for DatabaseEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: DatabaseEvent) {
        match event {
            DatabaseEvent::MapUpdated(map) => {
                let id = map.id.expect("valid map id if updated from database");
                if Some(id) == context.map_service.map().and_then(|map| map.id) {
                    context
                        .mediator_service
                        .queue_update_map(context.map_service.preset(), Some(map))
                }
            }
            DatabaseEvent::MapDeleted(id) => {
                if Some(id) == context.map_service.map().and_then(|map| map.id) {
                    context
                        .mediator_service
                        .queue_update_map(context.map_service.preset(), None)
                }
            }
            DatabaseEvent::CharacterDeleted(id) => {
                let current_id = context
                    .character_service
                    .character()
                    .and_then(|character| character.id);
                if Some(id) == current_id {
                    context.mediator_service.queue_update_character(None);
                }
            }
            DatabaseEvent::CharacterUpdated(character) => {
                let id = character
                    .id
                    .expect("valid character id if updated from database");
                let current_id = context
                    .character_service
                    .character()
                    .and_then(|character| character.id);
                if Some(id) == current_id {
                    context
                        .mediator_service
                        .queue_update_character(Some(character));
                }
            }
            DatabaseEvent::SettingsUpdated(settings) => {
                let settings = {
                    context.settings_service.update_settings(settings);
                    context.settings_service.settings().clone()
                };
                context
                    .operation_service
                    .config(context.resources, OperationConfiguration::from(&settings));
                context.control_service.update(&settings);
                context.rotator_service.update_from_settings(&settings);
                context.rotator_service.apply(context.rotator);

                update_capture_and_input(context);
            }
            DatabaseEvent::LocalizationUpdated(localization) => context
                .localization_service
                .update_localization(localization),
            DatabaseEvent::NavigationPathsDeleted | DatabaseEvent::NavigationPathsUpdated => {
                context.navigator.mark_dirty(true)
            }
        }
    }
}

fn update_capture_and_input(context: &mut EventContext) {
    let settings = context.settings_service.settings();

    context
        .capture_service
        .apply_mode(context.capture, settings.capture_mode);

    let window = match settings.capture_mode {
        CaptureMode::BitBltArea => context.capture.window(),
        CaptureMode::WindowsGraphicsCapture | CaptureMode::BitBlt => {
            context.capture_service.selected_window()
        }
    };
    let method = match (settings.input_method, settings.capture_mode) {
        (DatabaseInputMethod::Default, CaptureMode::BitBltArea) => InputMethod::ForegroundDefault,
        (DatabaseInputMethod::Default, _) => InputMethod::FocusedDefault,
        (DatabaseInputMethod::Rpc, CaptureMode::BitBltArea) => {
            InputMethod::ForegroundRpc(settings.input_method_rpc_server_url.clone())
        }
        (DatabaseInputMethod::Rpc, _) => {
            InputMethod::FocusedRpc(settings.input_method_rpc_server_url.clone())
        }
    };

    let input = context.resources.input.deref_mut();
    context.input_service.apply_window(input, window);
    context.input_service.apply_method(input, method);
}
