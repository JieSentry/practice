use std::{fmt::Debug, ops::DerefMut};

use opencv::{
    core::{MatTraitConst, MatTraitConstManual, Rect, Vec4b, Vector},
    imgcodecs::{IMREAD_COLOR, IMREAD_GRAYSCALE, imdecode},
};
use tokio::{
    spawn,
    sync::{
        broadcast::{self},
        mpsc, oneshot,
    },
    task::spawn_blocking,
};

#[cfg(debug_assertions)]
use crate::DebugState;
use crate::{
    BoundQuadrant, Character, DetectionTemplate, KeyBinding, NavigationPath, Operation,
    OperationUpdate, Request, Response, State,
    detect::to_base64_from_mat,
    ecs::{Resources, World},
    minimap::Minimap,
    models::Map,
    operation::Operation as InternalOperation,
    player::Quadrant,
    recv_request,
    services::{Event, EventContext, EventHandler},
    skill::SkillKind,
};

#[derive(Debug)]
pub enum MediatorEvent {
    Ui {
        request: Request,
        response: oneshot::Sender<Response>,
    },
    UpdateMap(Option<String>, Option<Map>),
    UpdateCharacter(Option<Character>),
}

impl Event for MediatorEvent {}

/// A service to handle mediation-related incoming requests.
pub trait MediatorService: Debug {
    fn subscribe_state(&self) -> broadcast::Receiver<State>;

    fn broadcast_state(&self, resources: &Resources, world: &World, map: Option<&Map>);

    /// Queues a [`MediatorEvent::UpdateCharacter`].
    fn queue_update_character(&self, character: Option<Character>);

    /// Queues a [`MediatorEvent::UpdateMap`].
    fn queue_update_map(&self, preset: Option<String>, map: Option<Map>);
}

#[derive(Debug)]
pub struct DefaultMediatorService {
    event_tx: mpsc::UnboundedSender<MediatorEvent>,
    state_tx: broadcast::Sender<State>,
}

impl DefaultMediatorService {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MediatorEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let event_tx_clone = event_tx.clone();
        spawn(async move {
            loop {
                if let Some((request, response)) = recv_request().await {
                    let _ = event_tx_clone.send(MediatorEvent::Ui { request, response });
                }
            }
        });

        let service = Self {
            event_tx,
            state_tx: broadcast::channel(1).0,
        };
        (service, event_rx)
    }
}

impl MediatorService for DefaultMediatorService {
    fn subscribe_state(&self) -> broadcast::Receiver<State> {
        self.state_tx.subscribe()
    }

    fn broadcast_state(&self, resources: &Resources, world: &World, map: Option<&Map>) {
        if !self.state_tx.is_empty() {
            return;
        }

        let player_context = &world.player.context;
        let state = world.player.state.to_string();
        let health = player_context.health();
        let normal_action = player_context.normal_action_name();
        let priority_action = player_context.priority_action_name();
        let position = player_context.last_known_pos.map(|pos| (pos.x, pos.y));
        let destinations = player_context
            .last_destinations
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|pos| (pos.x, pos.y))
            .collect();

        let erda_shower_state = world.skills[SkillKind::ErdaShower].state.to_string();

        let idle = match world.minimap.state {
            Minimap::Idle(idle) => Some(idle),
            Minimap::Detecting => None,
        };

        let platforms_bound = idle
            .zip(map.filter(|map| map.auto_mob_platforms_bound))
            .and_then(|(idle, _)| idle.platforms_bound.map(Into::into));

        let portals = idle
            .map(|idle| idle.portals().into_iter().map(Into::into).collect())
            .unwrap_or_default();

        let operation = match resources.operation {
            InternalOperation::HaltUntil { instant, .. } => Operation::HaltUntil(instant),
            InternalOperation::TemporaryHalting { resume, .. } => {
                Operation::TemporaryHalting(resume)
            }
            InternalOperation::Halting => Operation::Halting,
            InternalOperation::Running => Operation::Running,
            InternalOperation::RunUntil { instant, .. } => Operation::RunUntil(instant),
        };

        let auto_mob_quadrant =
            player_context
                .auto_mob_last_quadrant()
                .map(|quadrant| match quadrant {
                    Quadrant::TopLeft => BoundQuadrant::TopLeft,
                    Quadrant::TopRight => BoundQuadrant::TopRight,
                    Quadrant::BottomRight => BoundQuadrant::BottomRight,
                    Quadrant::BottomLeft => BoundQuadrant::BottomLeft,
                });
        let detector = resources
            .detector
            .as_ref()
            .map(|_| resources.detector_cloned());

        let sender = self.state_tx.clone();

        spawn_blocking(move || {
            let frame = detector
                .zip(idle)
                .map(|(detector, idle)| minimap_frame_from(idle.bbox, &detector.mat()));

            let state = State {
                position,
                health,
                state,
                normal_action,
                priority_action,
                erda_shower_state,
                destinations,
                operation,
                frame,
                platforms_bound,
                portals,
                auto_mob_quadrant,
            };

            let _ = sender.send(state);
        });
    }

    fn queue_update_character(&self, character: Option<Character>) {
        let _ = self
            .event_tx
            .send(MediatorEvent::UpdateCharacter(character));
    }

    fn queue_update_map(&self, preset: Option<String>, map: Option<Map>) {
        let _ = self.event_tx.send(MediatorEvent::UpdateMap(preset, map));
    }
}

pub struct MediatorEventHandler;

impl EventHandler<MediatorEvent> for MediatorEventHandler {
    fn handle(&mut self, context: &mut EventContext<'_>, event: MediatorEvent) {
        match event {
            MediatorEvent::Ui { request, response } => {
                handle_ui_request(context, request, response)
            }
            MediatorEvent::UpdateMap(preset, map) => update_map(context, preset, map),
            MediatorEvent::UpdateCharacter(character) => update_character(context, character),
        }
    }
}

fn handle_ui_request(
    context: &mut EventContext<'_>,
    request: Request,
    response: oneshot::Sender<Response>,
) {
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
        Request::NavigationSnapshotAsGrayscale(base64) => Response::NavigationSnapshotAsGrayscale(
            convert_navigation_path_snapshot_to_grayscale(context, base64),
        ),
        Request::UpdateCharacter(character) => {
            update_character(context, character);
            Response::UpdateCharacter
        }
        Request::RedetectMinimap => {
            redetect_map_minimap(context);
            Response::RedetectMinimap
        }
        Request::StateReceiver => Response::StateReceiver(subscribe_game_state(context)),
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
        Request::DebugStateReceiver => Response::DebugStateReceiver(subscribe_debug_state(context)),
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
    let _ = response.send(result);
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
    let settings = context.settings_service.settings();

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

fn subscribe_game_state(context: &mut EventContext<'_>) -> broadcast::Receiver<State> {
    context.mediator_service.subscribe_state()
}

fn subscribe_key(context: &mut EventContext<'_>) -> broadcast::Receiver<KeyBinding> {
    context.input_service.subscribe_key()
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
        capture_service.selected_window(),
    );
}

fn query_template(context: &mut EventContext<'_>, template: DetectionTemplate) -> String {
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
fn subscribe_debug_state(context: &mut EventContext<'_>) -> broadcast::Receiver<DebugState> {
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

#[inline]
fn minimap_frame_from(bbox: Rect, mat: &impl MatTraitConst) -> (Vec<u8>, usize, usize) {
    let minimap = mat
        .roi(bbox)
        .unwrap()
        .iter::<Vec4b>()
        .unwrap()
        .flat_map(|bgra| {
            let bgra = bgra.1;
            [bgra[2], bgra[1], bgra[0], 255]
        })
        .collect::<Vec<u8>>();
    (minimap, bbox.width as usize, bbox.height as usize)
}
