use std::{collections::HashMap, fmt::Display, mem::discriminant, ops::Range};

use backend::{
    Action, ActionCondition, ActionKey, IntoEnumIterator, KeyBinding, Map, Platform, key_receiver,
    update_map, upsert_map,
};
use dioxus::{html::FileData, prelude::*};
use futures_util::StreamExt;
use list::ActionsList;
use popup::{PopupActionsInputContent, PopupPlatformInputContent};
use rotation::SectionRotation;
use tokio::sync::broadcast::error::RecvError;

use crate::{
    AppState,
    actions::list::{ItemClickEvent, ItemMoveEvent},
    components::{
        ContentAlign, ContentSide,
        button::{Button, ButtonStyle},
        checkbox::Checkbox,
        file::{FileInput, FileOutput},
        icons::XIcon,
        key::KeyInput,
        labeled::Labeled,
        named_select::NamedSelect,
        numbers::{MillisInput, PrimitiveIntegerInput},
        popup::{PopupContext, PopupTrigger},
        position::PositionInput,
        section::Section,
        select::{Select, SelectOption},
    },
};

mod input;
mod list;
mod popup;
mod rotation;

const ITEM_TEXT_CLASS: &str =
    "text-center inline-block pt-1 text-ellipsis overflow-hidden whitespace-nowrap";
const ITEM_BORDER_CLASS: &str = "border-r-2 border-secondary-border";

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ActionsUpdate {
    Set,
    Create(String),
    Delete,
    Update(Vec<Action>),
    UpdateMinimap(Map),
}

#[derive(PartialEq, Copy, Clone)]
struct ActionsContext {
    map: Memo<Map>,
    save_map: Callback<Map>,
    lists: Signal<HashMap<String, ActionCondition>>,
}

#[component]
pub fn ActionsScreen() -> Element {
    let mut map = use_context::<AppState>().map;
    let mut map_preset = use_context::<AppState>().map_preset;
    // Non-null view of map
    let map_view = use_memo(move || map().unwrap_or_default());
    // Maps currently selected `map` to presets
    let map_presets = use_memo(move || {
        map()
            .map(|map| map.actions.into_keys().collect::<Vec<String>>())
            .unwrap_or_default()
    });
    // Maps currently selected `map_preset` to actions
    let map_preset_actions = use_memo(move || {
        map()
            .zip(map_preset())
            .and_then(|(map, preset)| map.actions.get(&preset).cloned())
            .unwrap_or_default()
    });
    // Maps currently selected `map_preset` to the index in `map_presets`
    let map_preset_index = use_memo(move || {
        let presets = map_presets();
        map_preset().and_then(|preset| {
            presets
                .into_iter()
                .enumerate()
                .find(|(_, p)| &preset == p)
                .map(|(i, _)| i)
        })
    });

    // Handles async operations for action-related
    let coroutine = use_coroutine(move |mut rx: UnboundedReceiver<ActionsUpdate>| async move {
        while let Some(message) = rx.next().await {
            match message {
                ActionsUpdate::Set => {
                    update_map(map_preset(), map()).await;
                }
                ActionsUpdate::Create(preset) => {
                    let Some(mut current_map) = map() else {
                        continue;
                    };
                    if current_map
                        .actions
                        .try_insert(preset.clone(), vec![])
                        .is_err()
                    {
                        continue;
                    }
                    if let Some(current_map) = upsert_map(current_map).await {
                        map_preset.set(Some(preset));
                        map.set(Some(current_map));
                        update_map(map_preset(), map()).await;
                    }
                }
                ActionsUpdate::Delete => {
                    let Some(mut current_map) = map() else {
                        continue;
                    };
                    let Some(preset) = map_preset() else {
                        continue;
                    };

                    if current_map.actions.remove(&preset).is_none() {
                        continue;
                    }
                    if let Some(current_map) = upsert_map(current_map).await {
                        map_preset.set(current_map.actions.keys().next().cloned());
                        map.set(Some(current_map));
                        update_map(map_preset(), map()).await;
                    }
                }
                ActionsUpdate::Update(actions) => {
                    let Some(mut current_map) = map() else {
                        continue;
                    };
                    let Some(preset) = map_preset() else {
                        continue;
                    };

                    current_map.actions.insert(preset, actions);
                    if let Some(current_map) = upsert_map(current_map).await {
                        map.set(Some(current_map));
                    }
                }
                ActionsUpdate::UpdateMinimap(new_map) => {
                    if let Some(new_map) = upsert_map(new_map).await {
                        map.set(Some(new_map));
                    }
                }
            }
        }
    });

    let save_map = use_callback(move |map: Map| {
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    });
    let select_preset = use_callback(move |index: usize| {
        let selected = map_presets.peek().get(index).cloned().unwrap();

        map_preset.set(Some(selected));
        coroutine.send(ActionsUpdate::Set);
    });

    let lists = use_signal::<HashMap<String, ActionCondition>>(HashMap::default);
    use_context_provider(|| ActionsContext {
        map: map_view,
        save_map,
        lists,
    });

    rsx! {
        div { class: "flex flex-col pb-15 h-full gap-3 overflow-y-auto pr-2",
            SectionRotation { disabled: map().is_none() }
            SectionPlatforms { disabled: map().is_none() }
            SectionActions {
                actions: map_preset_actions,
                disabled: map().is_none() || map_preset().is_none(),
            }
            SectionLegends {}
        }

        div { class: "flex items-center w-full h-10 pr-2 bg-primary-surface absolute bottom-0",
            NamedSelect {
                class: "flex-grow",
                on_create: move |name| {
                    coroutine.send(ActionsUpdate::Create(name));
                },
                on_delete: move |_| {
                    coroutine.send(ActionsUpdate::Delete);
                },
                disabled: map().is_none(),
                delete_disabled: map_presets().is_empty(),

                Select::<usize> {
                    class: "w-full",
                    placeholder: "Create an actions preset for the selected map...",
                    disabled: map_presets().is_empty(),
                    on_selected: select_preset,

                    for (i , name) in map_presets().into_iter().enumerate() {
                        SelectOption::<usize> {
                            value: i,
                            selected: map_preset_index() == Some(i),
                            label: name,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SectionPlatforms(disabled: bool) -> Element {
    #[component]
    fn PlatformItem(
        platform: Platform,
        on_item_click: Callback,
        on_item_delete: Callback,
    ) -> Element {
        const ICON_CONTAINER_CLASS: &str = "w-4 h-6 flex justify-center items-center";
        const ICON_CLASS: &str = "size-3";

        rsx! {
            div { class: "flex group",
                div {
                    class: "flex-grow grid grid-cols-2 h-6 text-xxs gap-2 text-secondary-text group-hover:bg-secondary-surface",
                    onclick: move |_| {
                        on_item_click(());
                    },
                    div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}",
                        {format!("X / {} - {}", platform.x_start, platform.x_end)}
                    }
                    div { class: "{ITEM_TEXT_CLASS}", {format!("Y / {}", platform.y)} }
                }
                div { class: "self-stretch invisible group-hover:visible group-hover:bg-secondary-surface flex items-center pr-1",
                    div {
                        class: ICON_CONTAINER_CLASS,
                        onclick: move |e| {
                            e.stop_propagation();
                            on_item_delete(());
                        },
                        XIcon { class: "{ICON_CLASS}" }
                    }
                }
            }
        }
    }

    #[derive(PartialEq, Clone, Copy)]
    enum PopupContent {
        None,
        Edit { platform: Platform, index: usize },
        Add,
    }

    let coroutine = use_coroutine_handle::<ActionsUpdate>();
    let settings = use_context::<AppState>().settings;
    let position = use_context::<AppState>().position;
    let context = use_context::<ActionsContext>();

    let map = context.map;
    let save_map = context.save_map;

    let add_platform = use_callback(move |platform| {
        let mut map = map();

        map.platforms.push(platform);
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    });
    let edit_platform = use_callback(move |(new_platform, index): (Platform, usize)| {
        let mut map = map();
        let Some(platform) = map.platforms.get_mut(index) else {
            return;
        };

        *platform = new_platform;
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    });
    let delete_platform = use_callback(move |index| {
        let mut map = map();

        map.platforms.remove(index);
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    });

    let mut popup_content = use_signal(|| PopupContent::None);
    let mut popup_open = use_signal(|| false);

    use_future(move || async move {
        let mut platform = Platform::default();
        let mut key_receiver = key_receiver().await;
        loop {
            let key = match key_receiver.recv().await {
                Ok(value) => value,
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(_)) => continue,
            };
            let Some(settings) = &*settings.peek() else {
                continue;
            };

            if settings.platform_start_key.enabled && settings.platform_start_key.key == key {
                platform.x_start = position.peek().0;
                update_valid_platform_end(&mut platform);
                platform.y = position.peek().1;
                continue;
            }

            if settings.platform_end_key.enabled && settings.platform_end_key.key == key {
                platform.x_end = position.peek().0;
                update_valid_platform_end(&mut platform);
                platform.y = position.peek().1;
                continue;
            }

            if settings.platform_add_key.enabled && settings.platform_add_key.key == key {
                update_valid_platform_end(&mut platform);
                add_platform(platform);
                continue;
            }
        }
    });

    rsx! {
        PopupContext {
            open: popup_open,
            on_open: move |open: bool| {
                popup_open.set(open);
            },
            Section { title: "Platforms",
                div { class: "grid grid-cols-3 gap-3",
                    ActionsCheckbox {
                        label: "Rune pathing",
                        disabled,
                        on_checked: move |rune_platforms_pathing| {
                            save_map(Map {
                                rune_platforms_pathing,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().rune_platforms_pathing,
                    }
                    ActionsCheckbox {
                        label: "Up jump only",
                        disabled: disabled || !map().rune_platforms_pathing,
                        on_checked: move |rune_platforms_pathing_up_jump_only| {
                            save_map(Map {
                                rune_platforms_pathing_up_jump_only,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().rune_platforms_pathing_up_jump_only,
                    }
                    div {}
                    ActionsCheckbox {
                        label: "Auto-mobbing pathing",
                        disabled,
                        on_checked: move |auto_mob_platforms_pathing| {
                            save_map(Map {
                                auto_mob_platforms_pathing,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().auto_mob_platforms_pathing,
                    }
                    ActionsCheckbox {
                        label: "Up jump only",
                        disabled: disabled || !map().auto_mob_platforms_pathing,
                        on_checked: move |auto_mob_platforms_pathing_up_jump_only| {
                            save_map(Map {
                                auto_mob_platforms_pathing_up_jump_only,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().auto_mob_platforms_pathing_up_jump_only,
                    }
                    ActionsCheckbox {
                        label: "Bound by platforms",
                        tooltip: "Auto-mobbing bound is computed based on the provided platforms instead of the provided bound.",
                        disabled,
                        on_checked: move |auto_mob_platforms_bound| {
                            save_map(Map {
                                auto_mob_platforms_bound,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().auto_mob_platforms_bound,
                    }
                }
                if !map().platforms.is_empty() {
                    div { class: "mt-2" }
                }
                for (index , platform) in map().platforms.into_iter().enumerate() {
                    PopupTrigger {
                        PlatformItem {
                            platform,
                            on_item_click: move |_| {
                                popup_content
                                    .set(PopupContent::Edit {
                                        platform,
                                        index,
                                    });
                            },
                            on_item_delete: move |_| {
                                delete_platform(index);
                            },
                        }
                    }
                }

                PopupTrigger {
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| {
                            popup_content.set(PopupContent::Add);
                        },
                        disabled,
                        class: "mt-2 w-full",

                        "Add platform"
                    }
                }

                PopupPlatformInputContent {
                    modifying: match popup_content() {
                        PopupContent::None | PopupContent::Add => false,
                        PopupContent::Edit { .. } => true,
                    },
                    on_cancel: move |_| {
                        popup_open.set(false);
                    },
                    on_value: move |mut platform| {
                        update_valid_platform_end(&mut platform);
                        let content = *popup_content.peek();
                        match content {
                            PopupContent::None => unreachable!(),
                            PopupContent::Add => add_platform(platform),
                            PopupContent::Edit { index, .. } => edit_platform((platform, index)),
                        }
                        popup_open.set(false);
                    },
                    value: match popup_content() {
                        PopupContent::None | PopupContent::Add => Platform::default(),
                        PopupContent::Edit { platform, .. } => platform,
                    },
                }
            }
        }
    }
}

#[component]
fn SectionLegends() -> Element {
    rsx! {
        Section { title: "Action legends", class: "text-xs text-primary-text",
            p { "⟳ - Repeat" }
            p { "⏱︎  - Wait" }
            p { "ㄨ - No position" }
            p { "⇈ - Queue to front" }
            p { "⇆ - Any direction" }
            p { "← - Left direction" }
            p { "→ - Right direction" }
            p { "A ~ B - Random range between A and B" }
            p { "A ↝ B - Use A key then B key" }
            p { "A ↜ B - Use B key then A key" }
            p { "A ↭ B - Use A and B keys at the same time" }
            p { "A ↷ B - Use A key then B key while A is held down" }
        }
    }
}

#[component]
fn SectionActions(actions: Memo<Vec<Action>>, disabled: bool) -> Element {
    #[derive(Clone, Copy, PartialEq)]
    enum PopupContent {
        None,
        Add(Action),
        Edit { action: Action, index: usize },
    }

    let coroutine = use_coroutine_handle::<ActionsUpdate>();
    let map = use_context::<ActionsContext>().map;

    let export_name = use_memo(move || format!("{}.json", map().name));
    let export_content = move |_| serde_json::to_vec_pretty(&*actions.peek()).unwrap_or_default();

    let import_actions = use_callback(move |file: FileData| async move {
        let mut actions = actions();

        let Ok(bytes) = file.read_bytes().await else {
            return;
        };
        let Ok(import_actions) = serde_json::from_slice::<'_, Vec<Action>>(&bytes) else {
            return;
        };

        let mut i = 0;
        while i < import_actions.len() {
            let action = import_actions[i];
            if matches!(action.condition(), ActionCondition::Linked) {
                // Malformed
                i += 1;
                continue;
            }

            actions.push(action);
            if let Some(range) = find_linked_action_range(&import_actions, i) {
                actions.extend(import_actions[range.clone()].iter().copied());
                i += range.count();
            }
            i += 1;
        }

        coroutine.send(ActionsUpdate::Update(actions));
    });

    let add_action = use_callback(move |(action, condition): (Action, ActionCondition)| {
        let mut actions = actions();
        let index = if matches!(action.condition(), ActionCondition::Linked) {
            find_last_linked_action_index(&actions, condition)
                .map(|index| index + 1)
                .unwrap_or(actions.len())
        } else {
            actions.len()
        };

        actions.insert(index, action);
        coroutine.send(ActionsUpdate::Update(actions));
    });

    let edit_action = use_callback(move |(new_action, index): (Action, usize)| {
        let mut actions = actions();
        let Some(action) = actions.get_mut(index) else {
            return;
        };

        *action = new_action;
        coroutine.send(ActionsUpdate::Update(actions));
    });

    let delete_action = use_callback(move |index: usize| {
        let mut actions = actions();
        let Some(condition) = actions.get(index).map(|action| action.condition()) else {
            return;
        };

        // Replaces the first linked action to this `action` condition
        // TODO: Maybe replace find_linked_action_range with a simple lookahead
        if !matches!(condition, ActionCondition::Linked)
            && find_linked_action_range(&actions, index).is_some()
        {
            actions[index + 1] = actions[index + 1].with_condition(condition);
        }
        actions.remove(index);
        coroutine.send(ActionsUpdate::Update(actions));
    });

    let move_action = use_callback(move |event: ItemMoveEvent| {
        let ItemMoveEvent {
            from_index,
            from_condition,
            to_index_local,
            to_index,
            to_condition,
        } = event;
        let mut actions = actions();
        let action = actions.remove(from_index);

        let insert_index = if from_condition == to_condition || from_index >= to_index {
            to_index
        } else {
            to_index - 1
        };
        let insert_index = insert_index.min(actions.len());
        let action_ref = actions.insert_mut(insert_index, action);
        debug!(target: "actions", "move action from {from_index} to {insert_index}");

        if from_condition != to_condition || to_index_local == 0 {
            *action_ref = action_ref.with_condition(to_condition);
        }

        coroutine.send(ActionsUpdate::Update(actions));
    });

    let mut popup_content = use_signal(|| PopupContent::None);
    let mut popup_open = use_signal(|| false);

    let mut handle_add_action_click = move |condition: ActionCondition| {
        let action = Action::Key(ActionKey::default()).with_condition(condition);
        let content = PopupContent::Add(action);
        popup_content.set(content);
    };

    let handle_edit_action_click = move |event: ItemClickEvent| {
        popup_content.set(PopupContent::Edit {
            action: event.action,
            index: event.index,
        });
    };

    rsx! {
        PopupContext {
            open: popup_open,
            on_open: move |open: bool| {
                popup_open.set(open);
            },
            Section { title: "Normal actions",
                ActionsList {
                    on_add_click: move |_| {
                        handle_add_action_click(ActionCondition::Any);
                    },
                    on_item_click: handle_edit_action_click,
                    on_item_move: move_action,
                    on_item_delete: delete_action,
                    condition: ActionCondition::Any,
                    disabled,
                    actions: actions(),
                }
            }
            Section { title: "Erda Shower off cooldown priority actions",
                ActionsList {
                    on_add_click: move |_| {
                        handle_add_action_click(ActionCondition::ErdaShowerOffCooldown);
                    },
                    on_item_click: handle_edit_action_click,
                    on_item_move: move_action,
                    on_item_delete: delete_action,
                    condition: ActionCondition::ErdaShowerOffCooldown,
                    disabled,
                    actions: actions(),
                }
            }
            Section { title: "Every milliseconds priority actions",
                ActionsList {
                    on_add_click: move |_| {
                        handle_add_action_click(ActionCondition::EveryMillis(0));
                    },
                    on_item_click: handle_edit_action_click,
                    on_item_move: move_action,
                    on_item_delete: delete_action,
                    condition: ActionCondition::EveryMillis(0),
                    disabled,
                    actions: actions(),
                }
            }
            Section { title: "Import/export actions",
                div { class: "flex gap-2",
                    FileInput {
                        class: "flex-grow",
                        on_file: move |file| async move {
                            import_actions(file).await;
                        },
                        disabled,
                        Button {
                            class: "w-full",
                            style: ButtonStyle::Primary,
                            disabled,
                            "Import"
                        }
                    }
                    FileOutput {
                        class: "flex-grow",
                        on_file: export_content,
                        download: export_name(),
                        disabled,
                        Button {
                            class: "w-full",
                            style: ButtonStyle::Primary,
                            disabled,
                            "Export"
                        }
                    }
                }
            }

            match popup_content() {
                #[allow(clippy::double_parens)]
                PopupContent::None => rsx! {},
                PopupContent::Add(action) => rsx! {
                    PopupActionsInputContent {
                        modifying: false,
                        linkable: !filter_actions(actions(), action.condition()).is_empty(),
                        on_cancel: move |_| {
                            popup_open.set(false);
                            popup_content.set(PopupContent::None);
                        },
                        on_value: move |args| {
                            add_action(args);
                            popup_open.set(false);
                            popup_content.set(PopupContent::None);
                        },
                        value: action,
                    }
                },
                PopupContent::Edit { action, index } => rsx! {
                    PopupActionsInputContent {
                        modifying: true,
                        linkable: filter_actions(actions(), action.condition())
                            .into_iter()
                            .next()
                            .map(|first| first.1 != index)
                            .unwrap_or_default(),
                        on_copy: move |_| {
                            popup_content.set(PopupContent::Add(action));
                        },
                        on_cancel: move |_| {
                            popup_open.set(false);
                            popup_content.set(PopupContent::None);
                        },
                        on_value: move |(action, _)| {
                            edit_action((action, index));
                            popup_open.set(false);
                            popup_content.set(PopupContent::None);
                        },
                        value: action,
                    }
                },
            }
        }
    }
}

#[component]
fn ActionsSelect<T: 'static + Clone + PartialEq + Display + IntoEnumIterator>(
    label: &'static str,
    #[props(default)] tooltip: Option<String>,
    #[props(default = ContentAlign::Start)] tooltip_align: ContentAlign,
    disabled: bool,
    on_selected: Callback<T>,
    selected: ReadSignal<T>,
) -> Element {
    let selected_equal =
        use_callback(move |value: T| discriminant(&selected()) == discriminant(&value));

    rsx! {
        Labeled { label, tooltip, tooltip_align,
            Select::<T> { on_selected, disabled,

                for value in T::iter() {
                    SelectOption::<T> {
                        value: value.clone(),
                        label: value.to_string(),
                        selected: selected_equal(value),
                        disabled,
                    }
                }
            }
        }
    }
}

#[component]
fn ActionsPositionInput(
    label: &'static str,
    #[props(default)] disabled: bool,
    on_icon_click: ReadSignal<Option<Callback>>,
    on_value: Callback<i32>,
    value: i32,
) -> Element {
    rsx! {
        Labeled { label,
            PositionInput {
                disabled,
                on_icon_click,
                on_value,
                value,
            }
        }
    }
}

#[component]
fn ActionsNumberInputI32(
    label: &'static str,
    #[props(default)] disabled: bool,
    on_value: Callback<i32>,
    value: i32,
) -> Element {
    rsx! {
        Labeled { label,
            PrimitiveIntegerInput { disabled, on_value, value }
        }
    }
}

#[component]
fn ActionsNumberInputU32(
    label: &'static str,
    #[props(default)] disabled: bool,
    on_value: Callback<u32>,
    value: u32,
) -> Element {
    rsx! {
        Labeled { label,
            PrimitiveIntegerInput {
                disabled,
                on_value,
                value,
                min_value: 1,
            }
        }
    }
}

#[component]
fn ActionsMillisInput(
    label: &'static str,
    #[props(default)] disabled: bool,
    on_value: Callback<u64>,
    value: u64,
) -> Element {
    rsx! {
        Labeled { label,
            MillisInput { disabled, on_value, value }
        }
    }
}

#[component]
fn ActionsCheckbox(
    label: &'static str,
    #[props(default)] tooltip: Option<String>,
    #[props(default = ContentSide::Left)] tooltip_side: ContentSide,
    #[props(default = ContentAlign::End)] tooltip_align: ContentAlign,
    #[props(default)] disabled: bool,
    on_checked: Callback<bool>,
    checked: bool,
) -> Element {
    rsx! {
        Labeled {
            label,
            tooltip,
            tooltip_side,
            tooltip_align,
            Checkbox { disabled, on_checked, checked }
        }
    }
}

#[component]
fn ActionsKeyBindingInput(
    label: &'static str,
    disabled: bool,
    on_value: Callback<Option<KeyBinding>>,
    value: Option<KeyBinding>,
) -> Element {
    rsx! {
        Labeled { label,
            KeyInput {
                class: "border border-primary-border",
                disabled,
                on_value: move |value: Option<KeyBinding>| {
                    on_value(value);
                },
                value,
            }
        }
    }
}

/// Finds the linked action index range where `action_index` is a non-linked action.
fn find_linked_action_range(actions: &[Action], action_index: usize) -> Option<Range<usize>> {
    if action_index + 1 >= actions.len() {
        return None;
    }
    let start = action_index + 1;
    if !matches!(actions[start].condition(), ActionCondition::Linked) {
        return None;
    }

    let mut end = start + 1;
    while end < actions.len() {
        if !matches!(actions[end].condition(), ActionCondition::Linked) {
            break;
        }
        end += 1;
    }

    Some(start..end)
}

/// Finds the last linked action index of the last action matching `condition`.
fn find_last_linked_action_index(actions: &[Action], condition: ActionCondition) -> Option<usize> {
    let condition_filter = discriminant(&condition);
    let (mut last_index, _) = actions
        .iter()
        .enumerate()
        .rev()
        .find(|(_, action)| condition_filter == discriminant(&action.condition()))?;

    if let Some(range) = find_linked_action_range(actions, last_index) {
        last_index += range.count();
    }

    Some(last_index)
}

/// Filters `actions` to find action with condition matching `condition` including linked
/// action(s) of that matching action.
///
/// Returns a [`Vec<(Action, usize)>`] where [`usize`] is the index of the action inside the
/// original `actions`.
fn filter_actions(actions: Vec<Action>, condition: ActionCondition) -> Vec<(Action, usize)> {
    let condition_filter = discriminant(&condition);
    let mut filtered = Vec::with_capacity(actions.len());
    let mut i = 0;
    while i < actions.len() {
        let action = actions[i];
        if condition_filter != discriminant(&action.condition()) {
            i += 1;
            continue;
        }

        filtered.push((action, i));
        if let Some(range) = find_linked_action_range(&actions, i) {
            filtered.extend(actions[range.clone()].iter().copied().zip(range.clone()));
            i += range.count();
        }
        i += 1;
    }

    filtered
}

#[inline]
fn update_valid_platform_end(platform: &mut Platform) {
    platform.x_end = if platform.x_end <= platform.x_start {
        platform.x_start + 1
    } else {
        platform.x_end
    };
}
