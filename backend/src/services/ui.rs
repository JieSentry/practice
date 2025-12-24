use std::{collections::VecDeque, fmt::Debug, ops::DerefMut};

use opencv::{
    core::Vector,
    imgcodecs::{IMREAD_COLOR, IMREAD_GRAYSCALE, imdecode},
};
use tokio::sync::{broadcast::Receiver, oneshot::Sender};

#[cfg(debug_assertions)]
use crate::DebugState;
use crate::{
    Character, GameState, GameTemplate, KeyBinding, NavigationPath, OperationUpdate, Request,
    Response,
    detect::to_base64_from_mat,
    models::Map,
    poll_request,
    services::{Event, EventContext, EventHandler},
};

#[derive(Debug)]
pub enum UiEvent {
    External {
        request: Request,
        response: Sender<Response>,
    },
    Internal {
        request: Request,
    },
}

impl Event for UiEvent {}

/// A service to handle ui-related incoming requests.
pub trait UiService: Debug {
    /// Polls for any pending [`UiEvent`].
    fn poll(&mut self) -> Option<UiEvent>;

    /// Queues a [`UiEvent`] for character update.
    fn queue_update_character(&mut self, character: Option<Character>);

    /// Queues a [`UiEvent`] for map update.
    fn queue_update_map(&mut self, preset: Option<String>, map: Option<Map>);
}

#[derive(Debug, Default)]
pub struct DefaultUiService {
    pending_events: VecDeque<UiEvent>,
}

impl UiService for DefaultUiService {
    fn poll(&mut self) -> Option<UiEvent> {
        if let Some(event) = self.pending_events.pop_front() {
            return Some(event);
        }

        poll_request().map(|(request, response)| UiEvent::External { request, response })
    }

    fn queue_update_character(&mut self, character: Option<Character>) {
        self.pending_events.push_back(UiEvent::Internal {
            request: Request::UpdateCharacter(character),
        });
    }

    fn queue_update_map(&mut self, preset: Option<String>, map: Option<Map>) {
        self.pending_events.push_back(UiEvent::Internal {
            request: Request::UpdateMap(preset, map),
        });
    }
}

pub struct UiEventHandler;

impl EventHandler<UiEvent> for UiEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: UiEvent) {
        let (request, response) = match event {
            UiEvent::External { request, response } => (request, Some(response)),
            UiEvent::Internal { request } => (request, None),
        };
        let result = match request {
            Request::UpdateOperation(update) => {
                update_operation(context, update);
                Response::UpdateOperation
            }
            Request::CreateMap(name) => Response::CreateMap(create_map(context, name)),
            Request::UpdateMap(preset, map) => {
                update_map(context, preset, map);
                Response::UpdateMap
            }
            Request::CreateNavigationPath => {
                Response::CreateNavigationPath(create_navigation_path(context))
            }
            Request::RecaptureNavigationPath(path) => {
                Response::RecaptureNavigationPath(recapture_navigation_path(context, path))
            }
            Request::NavigationSnapshotAsGrayscale(base64) => {
                Response::NavigationSnapshotAsGrayscale(
                    convert_navigation_path_snapshot_to_grayscale(context, base64),
                )
            }
            Request::UpdateCharacter(character) => {
                update_character(context, character);
                Response::UpdateCharacter
            }
            Request::RedetectMinimap => {
                redetect_map_minimap(context);
                Response::RedetectMinimap
            }
            Request::GameStateReceiver => {
                Response::GameStateReceiver(subscribe_game_state(context))
            }
            Request::KeyReceiver => Response::KeyReceiver(subscribe_key(context)),
            Request::RefreshCaptureHandles => {
                refresh_capture_handles(context);
                Response::RefreshCaptureHandles
            }
            Request::QueryCaptureHandles => {
                Response::QueryCaptureHandles(query_capture_handles(context))
            }
            Request::SelectCaptureHandle(index) => {
                select_capture_handle(context, index);
                Response::SelectCaptureHandle
            }
            Request::QueryTemplate(template) => {
                Response::QueryTemplate(query_template(context, template))
            }
            Request::ConvertImageToBase64(image, is_grayscale) => {
                Response::ConvertImageToBase64(convert_image_to_base64(image, is_grayscale))
            }
            Request::SaveCaptureImage(is_grayscale) => {
                save_capture_image(context, is_grayscale);
                Response::SaveCaptureImage
            }
            #[cfg(debug_assertions)]
            Request::DebugStateReceiver => {
                Response::DebugStateReceiver(subscribe_debug_state(context))
            }
            #[cfg(debug_assertions)]
            Request::AutoSaveRune(auto_save) => {
                update_auto_save_rune(context, auto_save);
                Response::AutoSaveRune
            }
            #[cfg(debug_assertions)]
            Request::InferRune => {
                infer_rune(context);
                Response::InferRune
            }
            #[cfg(debug_assertions)]
            Request::InferMinimap => {
                infer_minimap(context);
                Response::InferMinimap
            }
            #[cfg(debug_assertions)]
            Request::RecordImages(start) => {
                record_images(context, start);
                Response::RecordImages
            }
            #[cfg(debug_assertions)]
            Request::TestSpinRune => {
                test_spin_rune(context);
                Response::TestSpinRune
            }
        };

        if let Some(response) = response {
            let _ = response.send(result);
        }
    }
}

fn update_operation(context: &mut EventContext<'_>, update: OperationUpdate) {
    if context.map_service.map().is_none() || context.character_service.character().is_none() {
        return;
    }
    context.operation_service.update(context.resources, update);
}

fn create_map(context: &mut EventContext<'_>, name: String) -> Option<Map> {
    context
        .map_service
        .create(context.world.minimap.state, name)
}

fn update_map(context: &mut EventContext<'_>, preset: Option<String>, map: Option<Map>) {
    let world = &mut context.world;
    let map_service = &mut context.map_service;
    map_service.update_map_preset(map, preset);
    map_service.apply(&mut world.minimap.context, &mut world.player.context);

    let rotator_service = &mut context.rotator_service;
    let character_service = &context.character_service;
    let map = map_service.map();
    let preset = map_service.preset();
    let character = character_service.character();
    let settings_service = &context.settings_service;
    let settings = settings_service.settings();
    rotator_service.update_actions(map, preset, character);
    rotator_service.apply(context.rotator.deref_mut(), map, character, &settings);

    context
        .navigator
        .mark_dirty_with_destination(map.and_then(|map| map.paths_id_index));
}

fn redetect_map_minimap(context: &mut EventContext<'_>) {
    context.map_service.redetect(&mut context.world.minimap);
    context.navigator.mark_dirty(true);
}

fn create_navigation_path(context: &mut EventContext<'_>) -> Option<NavigationPath> {
    context
        .navigator_service
        .create_path(context.resources, context.world.minimap.state)
}

fn recapture_navigation_path(
    context: &mut EventContext<'_>,
    path: NavigationPath,
) -> NavigationPath {
    context
        .navigator_service
        .recapture_path(context.resources, context.world.minimap.state, path)
}

fn convert_navigation_path_snapshot_to_grayscale(
    context: &mut EventContext<'_>,
    base64: String,
) -> String {
    context
        .navigator_service
        .navigation_snapshot_as_grayscale(base64)
}

fn update_character(context: &mut EventContext<'_>, character: Option<Character>) {
    let character_service = &mut context.character_service;
    character_service.update_character(character);
    character_service.apply_character(&mut context.world.player.context);

    let character = character_service.character();

    let map_service = &context.map_service;
    let map = map_service.map();
    let preset = map_service.preset();

    let settings_service = &context.settings_service;
    let settings = settings_service.settings();

    let rotator_service = &mut context.rotator_service;
    rotator_service.update_actions(map, preset, character);
    rotator_service.update_buffs(character);
    if let Some(character) = character {
        context.world.buffs.iter_mut().for_each(|buff| {
            buff.context.update_enabled_state(character, &settings);
        });
    }
    rotator_service.apply(context.rotator.deref_mut(), map, character, &settings);
}

fn subscribe_game_state(context: &mut EventContext<'_>) -> Receiver<GameState> {
    context.game_service.subscribe_state()
}

fn subscribe_key(context: &mut EventContext<'_>) -> Receiver<KeyBinding> {
    context.game_service.subscribe_key()
}

fn refresh_capture_handles(context: &mut EventContext<'_>) {
    context.capture_service.update_windows();
    select_capture_handle(context, None);
}

fn query_capture_handles(context: &mut EventContext<'_>) -> (Vec<String>, Option<usize>) {
    (
        context.capture_service.window_names(),
        context.capture_service.selected_window_index(),
    )
}

fn select_capture_handle(context: &mut EventContext<'_>, index: Option<usize>) {
    let capture_service = &mut context.capture_service;
    capture_service.update_selected_window(index);
    capture_service.apply_selected_window(context.capture);

    context.input_service.apply_window(
        context.resources.input.deref_mut(),
        context.game_service.input_receiver_mut(),
        capture_service.selected_window(),
    );
}

fn query_template(context: &mut EventContext<'_>, template: GameTemplate) -> String {
    context.localization_service.template(template)
}

fn convert_image_to_base64(image: Vec<u8>, is_grayscale: bool) -> Option<String> {
    let flag = if is_grayscale {
        IMREAD_GRAYSCALE
    } else {
        IMREAD_COLOR
    };
    let vector = Vector::<u8>::from_iter(image);
    let mat = imdecode(&vector, flag).ok()?;

    to_base64_from_mat(&mat).ok()
}

fn save_capture_image(context: &mut EventContext<'_>, is_grayscale: bool) {
    context
        .localization_service
        .save_capture_image(context.resources, is_grayscale);
}

#[cfg(debug_assertions)]
fn subscribe_debug_state(context: &mut EventContext<'_>) -> Receiver<DebugState> {
    context.debug_service.subscribe_state()
}

#[cfg(debug_assertions)]
fn update_auto_save_rune(context: &mut EventContext<'_>, auto_save: bool) {
    context
        .debug_service
        .set_auto_save_rune(context.resources, auto_save);
}

#[cfg(debug_assertions)]
fn infer_rune(context: &mut EventContext<'_>) {
    context.debug_service.infer_rune();
}

#[cfg(debug_assertions)]
fn infer_minimap(context: &mut EventContext<'_>) {
    context.debug_service.infer_minimap(context.resources);
}

#[cfg(debug_assertions)]
fn record_images(context: &mut EventContext<'_>, start: bool) {
    context.debug_service.record_images(start);
}

#[cfg(debug_assertions)]
fn test_spin_rune(context: &mut EventContext<'_>) {
    context.debug_service.test_spin_rune();
}
