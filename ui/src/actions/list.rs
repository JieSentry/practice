use backend::{
    Action, ActionCondition, ActionKey, ActionKeyDirection, ActionKeyWith, ActionMove,
    LinkKeyBinding, Position,
};
use dioxus::prelude::*;

use crate::{
    actions::{ActionsContext, ITEM_BORDER_CLASS, ITEM_TEXT_CLASS, inner::filter_actions},
    components::{
        button::{Button, ButtonStyle},
        icons::XIcon,
        list::{List, ListItem, MoveEvent},
        popup::PopupTrigger,
    },
};

fn map_insert_index_local_to_global(filtered: Vec<(Action, usize)>, index: usize) -> usize {
    match index {
        0 => filtered.first().map(|first| first.1).unwrap_or(0),
        i if i < filtered.len() => filtered.get(i).unwrap().1,
        i if i == filtered.len() => filtered.last().map(|last| last.1 + 1).unwrap_or(0),
        _ => unreachable!(),
    }
}

#[derive(Debug)]
pub struct ItemClickEvent {
    pub action: Action,
    pub index: usize,
}

#[derive(Debug)]
pub struct ItemMoveEvent {
    pub from_index: usize,
    pub from_condition: ActionCondition,
    pub to_index_local: usize,
    pub to_index: usize,
    pub to_condition: ActionCondition,
}

#[component]
pub fn ActionsList(
    on_add_click: Callback,
    on_item_click: Callback<ItemClickEvent>,
    on_item_move: Callback<ItemMoveEvent>,
    on_item_delete: Callback<usize>,
    condition: ActionCondition,
    disabled: bool,
    actions: ReadSignal<Vec<Action>>,
) -> Element {
    let id = use_memo(move || condition.to_string());
    let filtered = use_memo(move || filter_actions(actions.cloned(), condition));

    let mut context = use_context::<ActionsContext>();

    let to_global_from_index_condition =
        move |index: usize| (filtered.get(index).unwrap().1, condition);

    let to_global_insert_index_condition = move |index: usize, list_id: &String| {
        let condition = context.lists.get(list_id).unwrap().cloned();
        let filtered = filter_actions(actions.cloned(), condition);
        let index = map_insert_index_local_to_global(filtered, index);

        (index, condition)
    };

    let handle_move = move |event: MoveEvent| {
        let (from_index, from_condition) = to_global_from_index_condition(event.from);
        let (to_index, to_condition) =
            to_global_insert_index_condition(event.to, &event.to_list_id);

        on_item_move(ItemMoveEvent {
            from_index,
            from_condition,
            to_index_local: event.to,
            to_index,
            to_condition,
        })
    };

    use_effect(move || {
        context.lists.insert(id(), condition);
    });

    use_drop(move || {
        context.lists.remove(&id());
    });

    rsx! {
        List {
            id: id(),
            group: "actions",
            class: "flex flex-col",
            on_move: handle_move,
            for (action , index) in filtered() {
                ListItem {
                    class: "flex group flex-grow",
                    on_click: move |_| {
                        on_item_click(ItemClickEvent { action, index });
                    },

                    PopupTrigger { class: "flex-grow",
                        match action {
                            Action::Move(action) => rsx! {
                                MoveItem { action }
                            },
                            Action::Key(action) => rsx! {
                                KeyItem { action }
                            },
                        }
                    }

                    ItemIcons {
                        condition,
                        action,
                        index,
                        on_item_delete,
                    }
                }
            }
        }

        PopupTrigger {
            Button {
                style: ButtonStyle::Secondary,
                on_click: move |_| {
                    on_add_click(());
                },
                disabled,
                class: "mt-2 w-full",

                "Add action"
            }
        }
    }
}

#[component]
fn MoveItem(action: ActionMove) -> Element {
    let ActionMove {
        position:
            Position {
                x,
                x_random_range,
                y,
                allow_adjusting,
            },
        condition,
        wait_after_move_millis,
    } = action;

    let x_min = (x - x_random_range).max(0);
    let x_max = (x + x_random_range).max(0);
    let x = if x_min == x_max {
        format!("{x}")
    } else {
        format!("{x_min}~{x_max}")
    };

    let allow_adjusting = if allow_adjusting { " / Adjust" } else { "" };

    let position = format!("{x}, {y}{allow_adjusting}");

    let linked_action = if matches!(condition, ActionCondition::Linked) {
        ""
    } else {
        "mt-2"
    };

    let wait_secs = format!("⏱︎ {:.2}s", wait_after_move_millis as f32 / 1000.0);

    rsx! {
        div { class: "grid grid-cols-[140px_100px_auto] h-6 text-xs text-secondary-text group-hover:bg-secondary-surface {linked_action}",
            div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}", "{position}" }
            div { class: "{ITEM_TEXT_CLASS}", "{wait_secs}" }
            div {}
        }
    }
}

#[component]
fn KeyItem(action: ActionKey) -> Element {
    let ActionKey {
        key,
        link_key,
        count,
        position,
        condition,
        direction,
        with,
        queue_to_front,
        wait_before_use_millis,
        wait_after_use_millis,
        ..
    } = action;

    let position = if let Some(Position {
        x,
        y,
        x_random_range,
        allow_adjusting,
    }) = position
    {
        let x_min = (x - x_random_range).max(0);
        let x_max = (x + x_random_range).max(0);
        let x = if x_min == x_max {
            format!("{x}")
        } else {
            format!("{x_min}~{x_max}")
        };
        let allow_adjusting = if allow_adjusting { " / Adjust" } else { "" };

        format!("{x}, {y}{allow_adjusting}")
    } else {
        "ㄨ".to_string()
    };

    let queue_to_front = if queue_to_front.unwrap_or_default() {
        "⇈ / "
    } else {
        ""
    };

    let linked_action = if matches!(condition, ActionCondition::Linked) {
        ""
    } else {
        "mt-2"
    };

    let link_key = match link_key {
        LinkKeyBinding::Before(key) => format!("{key} ↝ "),
        LinkKeyBinding::After(key) => format!("{key} ↜ "),
        LinkKeyBinding::AtTheSame(key) => format!("{key} ↭ "),
        LinkKeyBinding::Along(key) => format!("{key} ↷ "),
        LinkKeyBinding::None => "".to_string(),
    };

    let millis = if let ActionCondition::EveryMillis(millis) = condition {
        format!("⟳ {:.2}s / ", millis as f32 / 1000.0)
    } else {
        "".to_string()
    };

    let wait_before_secs = if wait_before_use_millis > 0 {
        Some(format!("⏱︎ {:.2}s", wait_before_use_millis as f32 / 1000.0))
    } else {
        None
    };

    let wait_after_secs = if wait_after_use_millis > 0 {
        Some(format!("⏱︎ {:.2}s", wait_after_use_millis as f32 / 1000.0))
    } else {
        None
    };

    let wait_secs = match (wait_before_secs, wait_after_secs) {
        (Some(before), None) => format!("{before} - ⏱︎ 0.00s / "),
        (None, None) => "".to_string(),
        (None, Some(after)) => format!("⏱︎ 0.00s - {after} / "),
        (Some(before), Some(after)) => format!("{before} - {after} / "),
    };

    let with = match with {
        ActionKeyWith::Any => "Any",
        ActionKeyWith::Stationary => "Stationary",
        ActionKeyWith::DoubleJump => "Double jump",
    };

    rsx! {
        div { class: "grid grid-cols-[140px_100px_30px_auto] h-6 text-xs text-secondary-text group-hover:bg-secondary-surface {linked_action}",
            div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}", "{queue_to_front}{position}" }
            div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}", "{link_key}{key} × {count}" }
            div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}",
                match direction {
                    ActionKeyDirection::Any => "⇆",
                    ActionKeyDirection::Left => "←",
                    ActionKeyDirection::Right => "→",
                }
            }
            div { class: "pl-1 pr-13 {ITEM_TEXT_CLASS}", "{millis}{wait_secs}{with}" }
        }
    }
}

#[component]
fn ItemIcons(
    condition: ActionCondition,
    action: Action,
    index: usize,
    on_item_delete: Callback<usize>,
) -> Element {
    const ICON_CONTAINER_CLASS: &str = "size-fit";
    const ICON_CLASS: &str = "size-3";

    let container_margin = if matches!(action.condition(), ActionCondition::Linked) {
        ""
    } else {
        "mt-2"
    };

    rsx! {
        div { class: "self-stretch invisible group-hover:visible group-hover:bg-secondary-surface flex gap-1 items-center {container_margin} pr-1",
            div {
                class: ICON_CONTAINER_CLASS,
                onclick: move |e| {
                    e.stop_propagation();
                    on_item_delete(index);
                },
                XIcon { class: "{ICON_CLASS}" }
            }
        }
    }
}
