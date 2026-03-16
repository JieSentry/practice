use super::EventContext;
use crate::{
    DatabaseEvent, bridge::InputMethod, operation::OperationConfiguration, services::EventHandler,
};

pub struct DatabaseEventHandler;

impl EventHandler<DatabaseEvent> for DatabaseEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: DatabaseEvent) {
        match event {
            DatabaseEvent::MapUpdated(_)
            | DatabaseEvent::MapDeleted(_)
            | DatabaseEvent::CharacterDeleted(_)
            | DatabaseEvent::CharacterUpdated(_) => { /* Handled by UI. */ }
            DatabaseEvent::SettingsUpdated(settings) => {
                let settings = {
                    context.settings_service.update_settings(settings);
                    context.settings_service.settings().clone()
                };
                context
                    .operation_service
                    .config(context.resources, OperationConfiguration::from(&settings));
                context.rotator_service.update_from_settings(&settings);
                context.rotator_service.apply(context.rotator);

                update_capture_and_input(context);
            }
            DatabaseEvent::LocalizationUpdated(localization) => context
                .localization_service
                .update_localization(localization),
        }
    }
}

fn update_capture_and_input(context: &mut EventContext) {
    let settings = context.settings_service.settings();

    context
        .capture_service
        .apply_mode(context.capture, settings.capture_mode);

    let window = context.capture.window();
    let method = InputMethod::from(&*settings);
    let input = &mut *context.resources.input;
    context.input_service.apply_window(input, window);
    context.input_service.apply_method(input, method);
}
