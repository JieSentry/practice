use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    fmt::{self, Debug},
    rc::Rc,
    sync::Arc,
};

use log::debug;
use platforms::{Window, input::InputKind};
use tokio::sync::broadcast::Receiver;

#[cfg(debug_assertions)]
use crate::services::debug::DebugService;
use crate::{
    Localization, Settings,
    bridge::{Capture, DefaultInputReceiver, Input},
    ecs::{Resources, World, WorldEvent},
    navigator::Navigator,
    rotator::Rotator,
    services::{
        character::{CharacterService, DefaultCharacterService},
        control::{ControlEventHandler, ControlService, DefaultControlService},
        game::{DefaultGameService, GameEventHandler, GameService},
        localization::{DefaultLocalizationService, LocalizationService},
        map::{DefaultMapService, MapService},
        navigator::{DefaultNavigatorService, NavigatorService},
        operation::{DefaultOperationService, OperationEventHandler, OperationService},
        rotator::{DefaultRotatorService, RotatorService},
        settings::{DefaultSettingsService, SettingsService},
        ui::{DefaultUiService, UiEventHandler, UiService},
        world::{DefaultWorldService, WorldEventHandler, WorldService},
    },
};

mod capture;
mod character;
mod control;
#[cfg(debug_assertions)]
mod debug;
mod game;
mod input;
mod localization;
mod map;
mod navigator;
mod operation;
mod rotator;
mod settings;
mod ui;
mod world;

pub trait Event: Any + Send + Sync + Debug + 'static {}

trait EventHandler<E: Event> {
    fn handle(&mut self, context: &mut EventContext<'_>, event: E);
}

type EventHandlerFn = Box<dyn FnMut(&mut EventContext<'_>, Box<dyn Any>)>;

struct EventBus {
    handlers: HashMap<TypeId, EventHandlerFn>,
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventBus")
            .field("handlers", &"HashMap { ... }")
            .finish()
    }
}

impl EventBus {
    fn subscribe<E: Event, H: EventHandler<E> + 'static>(&mut self, mut handler: H) {
        self.handlers
            .entry(TypeId::of::<E>())
            .or_insert(Box::new(move |context, event| {
                handler.handle(context, Box::into_inner(event.downcast::<E>().unwrap()));
            }));
    }

    fn emit(&mut self, context: &mut EventContext<'_>, event: Box<dyn Event>) {
        if let Some(handler) = self.handlers.get_mut(&event.as_ref().type_id()) {
            handler(context, event);
        }
    }
}

#[derive(Debug)]
struct EventContext<'a> {
    pub resources: &'a mut Resources,
    pub world: &'a mut World,
    pub rotator: &'a mut dyn Rotator,
    pub navigator: &'a mut dyn Navigator,
    pub capture: &'a mut dyn Capture,
    pub game_service: &'a mut Box<dyn GameService>,
    pub map_service: &'a mut Box<dyn MapService>,
    pub character_service: &'a mut Box<dyn CharacterService>,
    pub rotator_service: &'a mut Box<dyn RotatorService>,
    pub navigator_service: &'a mut Box<dyn NavigatorService>,
    pub settings_service: &'a mut Box<dyn SettingsService>,
    pub localization_service: &'a mut Box<dyn LocalizationService>,
    pub control_service: &'a mut Box<dyn ControlService>,
    pub operation_service: &'a mut Box<dyn OperationService>,
    pub ui_service: &'a mut Box<dyn UiService>,
    #[cfg(debug_assertions)]
    pub debug_service: &'a mut DebugService,
}

#[derive(Debug)]
pub struct Services {
    event_bus: EventBus,
    world: Box<dyn WorldService>,
    game: Box<dyn GameService>,
    map: Box<dyn MapService>,
    character: Box<dyn CharacterService>,
    rotator: Box<dyn RotatorService>,
    navigator: Box<dyn NavigatorService>,
    settings: Box<dyn SettingsService>,
    localization: Box<dyn LocalizationService>,
    control: Box<dyn ControlService>,
    operation: Box<dyn OperationService>,
    ui: Box<dyn UiService>,
    #[cfg(debug_assertions)]
    debug: DebugService,
}

impl Services {
    pub fn new(
        settings: Rc<RefCell<Settings>>,
        localization: Rc<RefCell<Arc<Localization>>>,
        event_rx: Receiver<WorldEvent>,
    ) -> Self {
        let settings_service = DefaultSettingsService::new(settings.clone());
        let window = settings_service.selected_window();
        let input_rx = DefaultInputReceiver::new(window, InputKind::Focused);
        let mut control = DefaultControlService::default();
        control.update(&settings_service.settings());

        let mut event_bus = EventBus {
            handlers: HashMap::default(),
        };
        event_bus.subscribe(UiEventHandler);
        event_bus.subscribe(GameEventHandler);
        event_bus.subscribe(ControlEventHandler);
        event_bus.subscribe(WorldEventHandler);
        event_bus.subscribe(OperationEventHandler);

        Self {
            event_bus,
            world: Box::new(DefaultWorldService::new(event_rx)),
            game: Box::new(DefaultGameService::new(input_rx)),
            map: Box::new(DefaultMapService::default()),
            character: Box::new(DefaultCharacterService::default()),
            rotator: Box::new(DefaultRotatorService::default()),
            navigator: Box::new(DefaultNavigatorService),
            settings: Box::new(settings_service),
            localization: Box::new(DefaultLocalizationService::new(localization)),
            control: Box::new(control),
            operation: Box::new(DefaultOperationService::default()),
            ui: Box::new(DefaultUiService::default()),
            #[cfg(debug_assertions)]
            debug: DebugService::default(),
        }
    }

    pub fn selected_window(&self) -> Window {
        self.settings.selected_window()
    }

    pub fn update_window(&mut self, input: &mut dyn Input, capture: &mut dyn Capture) {
        self.settings
            .apply_selected_window(input, self.game.input_receiver_mut(), capture);
    }

    #[inline]
    pub fn poll(
        &mut self,
        resources: &mut Resources,
        world: &mut World,
        rotator: &mut dyn Rotator,
        navigator: &mut dyn Navigator,
        capture: &mut dyn Capture,
    ) {
        let mut events = Vec::<Box<dyn Event>>::new();
        if let Some(event) = self.ui.poll() {
            events.push(Box::new(event));
        }
        self.game
            .poll(
                &self.settings.settings(),
                self.map.map().and_then(|map| map.id),
                self.character
                    .character()
                    .and_then(|character| character.id),
            )
            .into_iter()
            .for_each(|event| {
                events.push(Box::new(event));
            });
        if let Some(event) = self.operation.poll() {
            events.push(Box::new(event));
        }
        if let Some(event) = self.world.poll() {
            events.push(Box::new(event));
        }
        if let Some(event) = self.control.poll() {
            events.push(Box::new(event));
        }
        #[cfg(debug_assertions)]
        self.debug.poll(resources);

        let mut context = EventContext {
            resources,
            world,
            rotator,
            navigator,
            capture,
            game_service: &mut self.game,
            map_service: &mut self.map,
            character_service: &mut self.character,
            rotator_service: &mut self.rotator,
            navigator_service: &mut self.navigator,
            settings_service: &mut self.settings,
            localization_service: &mut self.localization,
            control_service: &mut self.control,
            operation_service: &mut self.operation,
            ui_service: &mut self.ui,
            #[cfg(debug_assertions)]
            debug_service: &mut self.debug,
        };
        for event in events {
            debug!(target: "services", "processing event {event:?}");
            self.event_bus.emit(&mut context, event);
        }

        context.game_service.broadcast_state(
            context.resources,
            context.world,
            context.map_service.map(),
        );
    }
}
