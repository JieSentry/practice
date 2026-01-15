use std::{mem::discriminant, ops::Not};

use backend::{
    Action, ActionCondition, ActionKey, ActionKeyDirection, ActionKeyWith, ActionMove, KeyBinding,
    LinkKeyBinding, Position, WaitAfterBuffered,
};
use dioxus::prelude::*;

use crate::{
    AppState,
    actions::{
        ActionsCheckbox, ActionsKeyBindingInput, ActionsMillisInput, ActionsNumberInputI32,
        ActionsNumberInputU32, ActionsPositionInput, ActionsSelect,
    },
    components::{
        ContentSide,
        button::{Button, ButtonStyle},
    },
};

#[derive(Debug)]
enum PositionUpdate {
    X(i32),
    XRange(i32),
    Y(i32),
    Adjusting(bool),
}

fn update_position_optional(signal: WriteSignal<Option<Position>>, update: PositionUpdate) {
    if signal.peek().is_some() {
        let mapped = signal
            .map_mut(
                |value| value.as_ref().unwrap(),
                |value| value.as_mut().unwrap(),
            )
            .into();

        update_position(mapped, update);
    }
}

fn update_position(mut signal: WriteSignal<Position>, update: PositionUpdate) {
    signal.with_mut(|position| match update {
        PositionUpdate::X(value) => position.x = value,
        PositionUpdate::XRange(value) => position.x_random_range = value,
        PositionUpdate::Y(value) => position.y = value,
        PositionUpdate::Adjusting(adjusting) => position.allow_adjusting = adjusting,
    });
}

fn use_position_icon_callback_optional(value: WriteSignal<Option<Position>>) -> Callback<bool> {
    let update_callback = use_position_update_callback();

    use_callback(move |is_x| {
        update_position_optional(value, update_callback(is_x));
    })
}

fn use_position_icon_callback(value: WriteSignal<Position>) -> Callback<bool> {
    let update_callback = use_position_update_callback();

    use_callback(move |is_x| {
        update_position(value, update_callback(is_x));
    })
}

fn use_position_update_callback() -> Callback<bool, PositionUpdate> {
    let position = use_context::<AppState>().position;

    use_callback(move |is_x| {
        let current_pos = position.cloned();
        let position = if is_x { current_pos.0 } else { current_pos.1 };
        if is_x {
            PositionUpdate::X(position)
        } else {
            PositionUpdate::Y(position)
        }
    })
}

#[component]
pub fn ActionsInput(
    switchable: bool,
    modifying: bool,
    linkable: bool,
    positionable: bool,
    directionable: bool,
    bufferable: bool,
    on_copy: Option<Callback>,
    on_cancel: Callback,
    on_value: Callback<(Action, ActionCondition)>,
    value: ReadSignal<Action>,
) -> Element {
    let mut current_value = use_signal(&*value);

    // TODO: Check if there is a bug on Dioxus side that cause `on_copy` to be `Some` even if
    // TODO: `None` is explicitly passed.
    let on_copy = modifying.then_some(on_copy).flatten();

    let handle_switch = move |_| {
        let value = value.cloned();

        let value_type = discriminant(&value);
        let current_value_type = discriminant(&*current_value.peek());
        if value_type != current_value_type {
            current_value.set(value);
            return;
        }

        match value {
            Action::Move(_) => {
                current_value.set(Action::Key(ActionKey {
                    condition: value.condition(),
                    ..ActionKey::default()
                }));
            }
            Action::Key(_) => {
                current_value.set(Action::Move(ActionMove {
                    condition: value.condition(),
                    ..ActionMove::default()
                }));
            }
        }
    };

    use_effect(move || {
        current_value.set(value());
    });

    rsx! {
        div { class: "flex flex-col pb-10 overflow-y-auto max-h-100",
            if switchable || on_copy.is_some() {
                div { class: "grid grid-flow-col",
                    if switchable {
                        Button {
                            style: ButtonStyle::Primary,
                            on_click: handle_switch,
                            class: "text-xxs",
                            if matches!(current_value(), Action::Move(_)) {
                                "Switch to key"
                            } else {
                                "Switch to move"
                            }
                        }
                    }
                    if let Some(on_copy) = on_copy {
                        Button {
                            style: ButtonStyle::Primary,
                            on_click: on_copy,
                            class: "text-xxs",
                            "Copy"
                        }
                    }
                }
                div { class: "col-span-3 border-b border-primary-border mb-3" }
            }

            match current_value() {
                Action::Move(action) => rsx! {
                    ActionsMoveInput {
                        modifying,
                        linkable,
                        on_cancel,
                        on_value: move |(action, condition)| {
                            on_value((Action::Move(action), condition));
                        },
                        value: action,
                    }
                },
                Action::Key(action) => rsx! {
                    ActionsKeyInput {
                        modifying,
                        linkable,
                        positionable,
                        directionable,
                        bufferable,
                        on_cancel,
                        on_value: move |(action, condition)| {
                            on_value((Action::Key(action), condition));
                        },
                        value: action,
                    }
                },
            }
        }
    }
}

#[component]
fn ActionsMoveInput(
    modifying: bool,
    linkable: bool,
    on_cancel: Callback,
    on_value: Callback<(ActionMove, ActionCondition)>,
    value: ReadSignal<ActionMove>,
) -> Element {
    let value_condition = value().condition;

    let mut current_value = use_signal(&*value);
    let current_value_position = WriteSignal::from(
        current_value.map_mut(|value| &value.position, |value| &mut value.position),
    );

    let handle_icon_click = use_position_icon_callback(current_value_position);

    let handle_x_icon_click = move |_| handle_icon_click(true);

    let handle_y_icon_click = move |_| handle_icon_click(false);

    let handle_adjusting_update = move |adjusting: bool| {
        update_position(current_value_position, PositionUpdate::Adjusting(adjusting));
    };

    use_effect(move || {
        current_value.set(value());
    });

    rsx! {
        div { class: "grid grid-cols-3 gap-3",
            // Position
            ActionsCheckbox {
                label: "Adjust",
                on_checked: handle_adjusting_update,
                checked: current_value().position.allow_adjusting,
            }

            div { class: "col-span-2" }

            ActionsPositionInput {
                label: "X",
                on_icon_click: handle_x_icon_click,
                on_value: move |x| {
                    update_position(current_value_position, PositionUpdate::X(x));
                },
                value: current_value().position.x,
            }

            ActionsNumberInputI32 {
                label: "X random range",
                on_value: move |x| {
                    update_position(current_value_position, PositionUpdate::XRange(x));
                },
                value: current_value().position.x_random_range,
            }

            ActionsPositionInput {
                label: "Y",
                on_icon_click: handle_y_icon_click,
                on_value: move |y| {
                    update_position(current_value_position, PositionUpdate::Y(y));
                },
                value: current_value().position.y,
            }

            ActionsMillisInput {
                label: "Wait after move",
                on_value: move |millis| {
                    let mut action = current_value.write();
                    action.wait_after_move_millis = millis;
                },
                value: current_value().wait_after_move_millis,
            }

            if linkable {
                ActionsCheckbox {
                    label: "Linked action",
                    on_checked: move |is_linked: bool| {
                        let mut action = current_value.write();
                        action.condition = if is_linked {
                            ActionCondition::Linked
                        } else {
                            value_condition
                        };
                    },
                    checked: matches!(current_value().condition, ActionCondition::Linked),
                }
            }
        }

        div { class: "flex w-full gap-3 absolute bottom-0 py-2 bg-secondary-surface",
            Button {
                class: "flex-grow",
                style: ButtonStyle::OutlinePrimary,
                on_click: move |_| {
                    on_value((*current_value.peek(), value_condition));
                },
                if modifying {
                    "Save"
                } else {
                    "Add"
                }
            }

            Button {
                class: "flex-grow",
                style: ButtonStyle::OutlineSecondary,
                on_click: move |_| {
                    on_cancel(());
                },
                "Cancel"
            }
        }
    }
}

#[component]
fn ActionsKeyInput(
    modifying: ReadSignal<bool>,
    linkable: ReadSignal<bool>,
    positionable: ReadSignal<bool>,
    directionable: ReadSignal<bool>,
    bufferable: ReadSignal<bool>,
    on_cancel: Callback,
    on_value: Callback<(ActionKey, ActionCondition)>,
    value: ReadSignal<ActionKey>,
) -> Element {
    let value_condition = value().condition;

    let mut current_value = use_signal(&*value);
    let current_value_position =
        current_value.map_mut(|value| &value.position, |value| &mut value.position);

    use_effect(move || {
        current_value.set(value());
    });

    rsx! {
        div { class: "grid grid-cols-3 gap-3 pr-2 overflow-y-auto",
            if positionable() {
                KeyPositionInput { value: current_value_position }
            }

            // Key, count and link key
            ActionsKeyBindingInput {
                label: "Key",
                disabled: false,
                on_value: move |key: Option<KeyBinding>| {
                    let mut action = current_value.write();
                    action.key = key.expect("not optional");
                },
                value: Some(current_value().key),
            }
            div { class: "grid grid-cols-2 gap-3",
                ActionsNumberInputU32 {
                    label: "Use count",
                    on_value: move |count| {
                        let mut action = current_value.write();
                        action.count = count;
                    },
                    value: current_value().count,
                }
                ActionsMillisInput {
                    label: "Hold for",
                    on_value: move |millis| {
                        let mut action = current_value.write();
                        action.key_hold_millis = millis;
                    },
                    value: current_value().key_hold_millis,
                }
            }
            if bufferable() {
                ActionsCheckbox {
                    label: "Holding buffered",
                    tooltip: "Require [Wait after buffered] to be enabled and without [Link key]. When enabled, the holding time will be added to [Wait after] during the last key use. Useful for holding down key and moving simultaneously.",
                    tooltip_side: ContentSide::Bottom,
                    on_checked: move |checked| {
                        let mut action = current_value.write();
                        action.key_hold_buffered_to_wait_after = checked;
                    },
                    checked: current_value().key_hold_buffered_to_wait_after,
                }
            } else {
                div {}
            }


            ActionsKeyBindingInput {
                label: "Link key",
                disabled: matches!(current_value().link_key, LinkKeyBinding::None),
                on_value: move |key: Option<KeyBinding>| {
                    let mut action = current_value.write();
                    action.link_key = action.link_key.with_key(key.expect("not optional"));
                },
                value: current_value().link_key.key().unwrap_or_default(),
            }
            ActionsSelect::<LinkKeyBinding> {
                label: "Link key type",
                disabled: false,
                on_selected: move |link_key: LinkKeyBinding| {
                    let mut action = current_value.write();
                    action.link_key = link_key;
                },
                selected: current_value().link_key,
            }
            if linkable() {
                ActionsCheckbox {
                    label: "Linked action",
                    on_checked: move |is_linked: bool| {
                        let mut action = current_value.write();
                        action.condition = if is_linked {
                            ActionCondition::Linked
                        } else {
                            value_condition
                        };
                        action.queue_to_front = None;
                    },
                    checked: matches!(current_value().condition, ActionCondition::Linked),
                }
            } else {
                div {} // Spacer
            }

            // Use with, direction

            ActionsSelect::<ActionKeyWith> {
                label: "Use with",
                disabled: false,
                on_selected: move |with| {
                    let mut action = current_value.write();
                    action.with = with;
                },
                selected: current_value().with,
            }
            if directionable() {
                ActionsSelect::<ActionKeyDirection> {
                    label: "Use direction",
                    disabled: false,
                    on_selected: move |direction| {
                        let mut action = current_value.write();
                        action.direction = direction;
                    },
                    selected: current_value().direction,
                }
            } else {
                div {} // Spacer
            }
            if matches!(
                current_value().condition,
                ActionCondition::EveryMillis(_) | ActionCondition::ErdaShowerOffCooldown
            )
            {
                ActionsCheckbox {
                    label: "Queue to front",
                    on_checked: move |queue_to_front: bool| {
                        let mut action = current_value.write();
                        action.queue_to_front = Some(queue_to_front);
                    },
                    checked: current_value().queue_to_front.is_some(),
                }
            } else {
                div {} // Spacer
            }
            if let ActionCondition::EveryMillis(millis) = current_value().condition {
                ActionsMillisInput {
                    label: "Use every",
                    on_value: move |millis| {
                        let mut action = current_value.write();
                        action.condition = ActionCondition::EveryMillis(millis);
                    },
                    value: millis,
                }
                div { class: "col-span-2" }
            }

            // Wait before use
            ActionsMillisInput {
                label: "Wait before use",
                on_value: move |millis| {
                    let mut action = current_value.write();
                    action.wait_before_use_millis = millis;
                },
                value: current_value().wait_before_use_millis,
            }
            ActionsMillisInput {
                label: "Wait random range",
                on_value: move |millis| {
                    let mut action = current_value.write();
                    action.wait_before_use_millis_random_range = millis;
                },
                value: current_value().wait_before_use_millis_random_range,
            }
            div {} // Spacer

            // Wait after use
            ActionsMillisInput {
                label: "Wait after use",
                on_value: move |millis| {
                    let mut action = current_value.write();
                    action.wait_after_use_millis = millis;
                },
                value: current_value().wait_after_use_millis,
            }
            ActionsMillisInput {
                label: "Wait random range",
                on_value: move |millis| {
                    let mut action = current_value.write();
                    action.wait_after_use_millis_random_range = millis;
                },
                value: current_value().wait_after_use_millis_random_range,
            }
            if bufferable() {
                ActionsSelect::<WaitAfterBuffered> {
                    label: "Wait after buffered",
                    tooltip: "After the last key use, instead of waiting inplace, the bot is allowed to execute the next action partially. This can be useful for movable skill with casting animation.",
                    disabled: false,
                    on_selected: move |wait_after_buffered: WaitAfterBuffered| {
                        let mut action = current_value.write();
                        action.wait_after_buffered = wait_after_buffered;
                    },
                    selected: current_value().wait_after_buffered,
                }
            }
        }
        div { class: "flex w-full gap-3 absolute bottom-0 py-2 bg-secondary-surface",
            Button {
                class: "flex-grow",
                style: ButtonStyle::OutlinePrimary,
                on_click: move |_| {
                    on_value((*current_value.peek(), value_condition));
                },
                if modifying() {
                    "Save"
                } else {
                    "Add"
                }
            }
            Button {
                class: "flex-grow",
                style: ButtonStyle::OutlineSecondary,
                on_click: move |_| {
                    on_cancel(());
                },
                "Cancel"
            }
        }
    }
}

#[component]
fn KeyPositionInput(value: WriteSignal<Option<Position>>) -> Element {
    let disabled = use_memo(move || value().is_none());

    let handle_icon_click = use_position_icon_callback_optional(value);

    let handle_x_icon_click = use_callback(move |_| handle_icon_click(true));

    let handle_y_icon_click = use_callback(move |_| handle_icon_click(false));

    let handle_adjusting_update = move |adjusting: bool| {
        update_position_optional(value, PositionUpdate::Adjusting(adjusting));
    };

    let handle_positioned_update = move |positioned: bool| {
        *value.write() = positioned.then_some(Position::default());
    };

    rsx! {
        div { class: "grid grid-cols-2 gap-3",
            ActionsPositionInput {
                label: "X",
                disabled: disabled(),
                on_icon_click: disabled().not().then_some(handle_x_icon_click),
                on_value: move |x| {
                    update_position_optional(value, PositionUpdate::X(x));
                },
                value: value().map(|pos| pos.x).unwrap_or_default(),
            }

            ActionsNumberInputI32 {
                label: "X range",
                disabled: disabled(),
                on_value: move |x| {
                    update_position_optional(value, PositionUpdate::XRange(x));
                },
                value: value().map(|pos| pos.x_random_range).unwrap_or_default(),
            }
        }

        ActionsPositionInput {
            label: "Y",
            disabled: disabled(),
            on_icon_click: disabled().not().then_some(handle_y_icon_click),
            on_value: move |y| {
                update_position_optional(value, PositionUpdate::Y(y));
            },
            value: value().map(|pos| pos.y).unwrap_or_default(),
        }

        div { class: "grid grid-cols-2 gap-3",
            ActionsCheckbox {
                label: "Adjust",
                disabled: disabled(),
                on_checked: handle_adjusting_update,
                checked: value().map(|pos| pos.allow_adjusting).unwrap_or_default(),
            }

            ActionsCheckbox {
                label: "Positioned",
                on_checked: handle_positioned_update,
                checked: !disabled(),
            }
        }
    }
}
