#![feature(new_range_api)]
#![feature(slice_pattern)]
#![feature(box_into_inner)]
#![feature(map_try_insert)]
#![feature(variant_count)]
#![feature(iter_array_chunks)]
#![feature(associated_type_defaults)]
#![feature(string_into_chars)]
#![feature(stmt_expr_attributes)]
#![feature(assert_matches)]

use std::{
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use strum::Display;
use tokio::{
    sync::{
        broadcast, mpsc,
        oneshot::{self, Sender},
    },
    task::spawn_blocking,
};

mod array;
mod bridge;
mod buff;
mod control;
mod database;
#[cfg(debug_assertions)]
mod debug;
mod detect;
mod ecs;
mod mat;
mod minimap;
mod models;
mod navigator;
mod notification;
mod operation;
mod pathing;
mod player;
mod rng;
mod rotator;
mod rpc;
mod run;
mod services;
mod skill;
mod task;
mod tracker;
mod utils;

pub use {
    database::{DatabaseEvent, database_event_receiver},
    models::*,
    pathing::MAX_PLATFORMS_COUNT,
    run::init,
    strum::{EnumMessage, IntoEnumIterator, ParseError},
};

type PendingRequest = (Request, Sender<Response>);

static REQUESTS: LazyLock<(
    mpsc::UnboundedSender<PendingRequest>,
    Mutex<mpsc::UnboundedReceiver<PendingRequest>>,
)> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded_channel();
    (tx, Mutex::new(rx))
});

macro_rules! send_request {
    ($variant:ident $(( $( $field:ident ),* ))?) => {{
        let request = Request::$variant$(( $( $field ),* ))?;
        let (tx, rx) = oneshot::channel();
        REQUESTS.0.send((request, tx)).expect("channel open");

        let response = rx.await.expect("successful response");
        match response {
            Response::$variant => (),
            _ => panic!("mismatch response and request type"),
        }}
    };

    ($variant:ident $(( $( $field:ident ),* ))? => ( $( $response:ident ),+ )) => {{
        let request = Request::$variant$(( $( $field ),* ))?;
        let (tx, rx) = oneshot::channel();
        REQUESTS.0.send((request, tx)).expect("channel open");

        let response = rx.await.expect("successful response");
        match response {
            Response::$variant($( $response ),+) => ($( $response),+),
            _ => panic!("mismatch response and request type"),
        }}
    };
}

/// Represents request from UI.
#[derive(Debug)]
enum Request {
    UpdateOperation(OperationUpdate),
    CreateMap(String),
    UpdateMap(Option<String>, Option<Map>),
    CreateNavigationPath,
    RecaptureNavigationPath(NavigationPath),
    NavigationSnapshotAsGrayscale(String),
    UpdateCharacter(Option<Character>),
    RedetectMinimap,
    GameStateReceiver,
    KeyReceiver,
    RefreshCaptureHandles,
    QueryCaptureHandles,
    SelectCaptureHandle(Option<usize>),
    QueryTemplate(GameTemplate),
    ConvertImageToBase64(Vec<u8>, bool),
    SaveCaptureImage(bool),
    #[cfg(debug_assertions)]
    DebugStateReceiver,
    #[cfg(debug_assertions)]
    AutoSaveRune(bool),
    #[cfg(debug_assertions)]
    InferRune,
    #[cfg(debug_assertions)]
    InferMinimap,
    #[cfg(debug_assertions)]
    RecordImages(bool),
    #[cfg(debug_assertions)]
    TestSpinRune,
}

/// Represents response to UI [`Request`].
///
/// All internal (e.g. OpenCV) structs must be converted to either database structs
/// or appropriate counterparts before passing to UI.
#[derive(Debug)]
enum Response {
    UpdateOperation,
    CreateMap(Option<Map>),
    UpdateMap,
    CreateNavigationPath(Option<NavigationPath>),
    RecaptureNavigationPath(NavigationPath),
    NavigationSnapshotAsGrayscale(String),
    UpdateCharacter,
    RedetectMinimap,
    GameStateReceiver(broadcast::Receiver<GameState>),
    KeyReceiver(broadcast::Receiver<KeyBinding>),
    RefreshCaptureHandles,
    QueryCaptureHandles((Vec<String>, Option<usize>)),
    SelectCaptureHandle,
    QueryTemplate(String),
    ConvertImageToBase64(Option<String>),
    SaveCaptureImage,
    #[cfg(debug_assertions)]
    DebugStateReceiver(broadcast::Receiver<DebugState>),
    #[cfg(debug_assertions)]
    AutoSaveRune,
    #[cfg(debug_assertions)]
    InferRune,
    #[cfg(debug_assertions)]
    InferMinimap,
    #[cfg(debug_assertions)]
    RecordImages,
    #[cfg(debug_assertions)]
    TestSpinRune,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GameTemplate {
    CashShop,
    ChangeChannel,
    Timer,
    PopupConfirm,
    PopupYes,
    PopupNext,
    PopupEndChat,
    PopupOkNew,
    PopupOkOld,
    PopupCancelNew,
    PopupCancelOld,
    FamiliarsLevelSort,
    FamiliarsSaveButton,
    HexaErdaConversionButton,
    HexaBoosterButton,
    HexaMaxButton,
    HexaConvertButton,
}

/// The four quads of a bound.
#[derive(Clone, Copy, Debug, Display)]
pub enum BoundQuadrant {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

/// A struct for storing debug information.
#[derive(Clone, PartialEq, Default, Debug)]
#[cfg(debug_assertions)]
pub struct DebugState {
    pub is_recording: bool,
    pub is_rune_auto_saving: bool,
}

/// A struct for storing game information.
#[derive(Clone, Debug)]
pub struct GameState {
    pub position: Option<(i32, i32)>,
    pub health: Option<(u32, u32)>,
    pub state: String,
    pub normal_action: Option<String>,
    pub priority_action: Option<String>,
    pub erda_shower_state: String,
    pub destinations: Vec<(i32, i32)>,
    pub operation: BotOperation,
    pub frame: Option<(Vec<u8>, usize, usize)>,
    pub platforms_bound: Option<Bound>,
    pub portals: Vec<Bound>,
    pub auto_mob_quadrant: Option<BoundQuadrant>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum BotOperation {
    Halting,
    TemporaryHalting(Duration),
    HaltUntil(Instant),
    Running,
    RunUntil(Instant),
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum OperationUpdate {
    Halt,
    TemporaryHalt,
    Run,
}

/// Updates the bot current's operation.
pub async fn update_operation(update: OperationUpdate) {
    send_request!(UpdateOperation(update))
}

/// Queries localization from the database.
pub async fn query_localization() -> Localization {
    spawn_blocking(database::query_or_upsert_localization)
        .await
        .unwrap()
}

/// Upserts `localization` to the database.
///
/// Returns the updated [`Localization`] or original if fails.
pub async fn upsert_localization(mut localization: Localization) -> Localization {
    spawn_blocking(move || {
        let _ = database::upsert_localization(&mut localization);
        localization
    })
    .await
    .unwrap()
}

/// Queries settings from the database.
pub async fn query_settings() -> Settings {
    spawn_blocking(database::query_settings).await.unwrap()
}

/// Upserts `settings` to the database.
///
/// Returns the updated [`Settings`] or original if fails.
pub async fn upsert_settings(mut settings: Settings) -> Settings {
    spawn_blocking(move || {
        let _ = database::upsert_settings(&mut settings);
        settings
    })
    .await
    .unwrap()
}

/// Queries maps from the database.
pub async fn query_maps() -> Option<Vec<Map>> {
    spawn_blocking(database::query_maps).await.unwrap().ok()
}

/// Creates a new map from the currently detected map.
///
/// This function does not insert the created map into the database.
pub async fn create_map(name: String) -> Option<Map> {
    send_request!(CreateMap(name) => (map))
}

/// Upserts `map` to the database.
///
/// If `map` does not previously exist, a new one will be created and its `id` will
/// be updated.
///
/// Returns the updated [`Minimap`] on success.
pub async fn upsert_map(mut map: Map) -> Option<Map> {
    spawn_blocking(move || database::upsert_map(&mut map).is_ok().then_some(map))
        .await
        .unwrap()
}

/// Updates the current map used by the main game loop.
pub async fn update_map(preset: Option<String>, map: Option<Map>) {
    send_request!(UpdateMap(preset, map))
}

/// Deletes `map` from the database.
///
/// Returns `true` if the map was deleted.
pub async fn delete_map(map: Map) -> bool {
    spawn_blocking(move || database::delete_map(&map).is_ok())
        .await
        .unwrap()
}

/// Queries navigation paths from the database.
pub async fn query_navigation_paths() -> Option<Vec<NavigationPaths>> {
    spawn_blocking(database::query_navigation_paths)
        .await
        .unwrap()
        .ok()
}

/// Creates a navigation path from currently detected map.
pub async fn create_navigation_path() -> Option<NavigationPath> {
    send_request!(CreateNavigationPath => (path))
}

/// Upserts `paths` to the database.
///
/// Returns the updated [`NavigationPaths`] on success.
pub async fn upsert_navigation_paths(mut paths: NavigationPaths) -> Option<NavigationPaths> {
    spawn_blocking(move || {
        database::upsert_navigation_paths(&mut paths)
            .is_ok()
            .then_some(paths)
    })
    .await
    .unwrap()
}

/// Recaptures snapshots for the provided `path`.
///
/// Snapshots include name and map will be recaptured and re-assigned to the given `path` if
/// the map is currently detected.
///
/// Returns the updated [`NavigationPath`] or original if map is currently not detectable.
pub async fn recapture_navigation_path(path: NavigationPath) -> NavigationPath {
    send_request!(RecaptureNavigationPath(path) => (path))
}

pub async fn navigation_snapshot_as_grayscale(base64: String) -> String {
    send_request!(NavigationSnapshotAsGrayscale(base64) => (base64))
}

/// Deletes `paths` from the database.
///
/// Returns `true` if `paths` was deleted.
pub async fn delete_navigation_paths(paths: NavigationPaths) -> bool {
    spawn_blocking(move || database::delete_navigation_paths(&paths).is_ok())
        .await
        .unwrap()
}

/// Queries characters from the database.
pub async fn query_characters() -> Option<Vec<Character>> {
    spawn_blocking(database::query_characters)
        .await
        .unwrap()
        .ok()
}

/// Upserts `character` to the database.
///
/// If `character` does not previously exist, a new one will be created and its `id` will
/// be updated.
///
/// Returns the updated [`Character`] on success.
pub async fn upsert_character(mut character: Character) -> Option<Character> {
    spawn_blocking(move || {
        database::upsert_character(&mut character)
            .is_ok()
            .then_some(character)
    })
    .await
    .unwrap()
}

/// Updates the current character used by the main game loop.
pub async fn update_character(character: Option<Character>) {
    send_request!(UpdateCharacter(character))
}

/// Deletes `character` from the database.
///
/// Returns `true` if the `character` was deleted.
pub async fn delete_character(character: Character) -> bool {
    spawn_blocking(move || database::delete_character(&character).is_ok())
        .await
        .unwrap()
}

pub async fn redetect_minimap() {
    send_request!(RedetectMinimap)
}

pub async fn game_state_receiver() -> broadcast::Receiver<GameState> {
    send_request!(GameStateReceiver => (receiver))
}

pub async fn key_receiver() -> broadcast::Receiver<KeyBinding> {
    send_request!(KeyReceiver => (receiver))
}

pub async fn refresh_capture_handles() {
    send_request!(RefreshCaptureHandles)
}

pub async fn query_capture_handles() -> (Vec<String>, Option<usize>) {
    send_request!(QueryCaptureHandles => (pair))
}

pub async fn select_capture_handle(index: Option<usize>) {
    send_request!(SelectCaptureHandle(index))
}

pub async fn query_template(template: GameTemplate) -> String {
    send_request!(QueryTemplate(template) => (base64))
}

pub async fn convert_image_to_base64(image: Vec<u8>, is_grayscale: bool) -> Option<String> {
    send_request!(ConvertImageToBase64(image, is_grayscale) => (base64))
}

pub async fn save_capture_image(is_grayscale: bool) {
    send_request!(SaveCaptureImage(is_grayscale))
}

#[cfg(debug_assertions)]
pub async fn debug_state_receiver() -> broadcast::Receiver<DebugState> {
    send_request!(DebugStateReceiver => (receiver))
}

#[cfg(debug_assertions)]
pub async fn auto_save_rune(auto_save: bool) {
    send_request!(AutoSaveRune(auto_save))
}

#[cfg(debug_assertions)]
pub async fn infer_rune() {
    send_request!(InferRune)
}

#[cfg(debug_assertions)]
pub async fn infer_minimap() {
    send_request!(InferMinimap)
}

#[cfg(debug_assertions)]
pub async fn record_images(start: bool) {
    send_request!(RecordImages(start))
}

#[cfg(debug_assertions)]
pub async fn test_spin_rune() {
    send_request!(TestSpinRune)
}

fn poll_request() -> Option<PendingRequest> {
    LazyLock::force(&REQUESTS).1.lock().unwrap().try_recv().ok()
}
