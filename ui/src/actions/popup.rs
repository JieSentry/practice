use backend::{Action, ActionCondition, ActionKey, Bound, MobbingKey, Platform};
use dioxus::prelude::*;

use crate::{
    AppState,
    actions::{ActionsNumberInputI32, ActionsPositionInput, input::ActionsInput},
    components::{
        button::{Button, ButtonStyle},
        popup::PopupContent,
    },
};

#[component]
pub fn PopupPlatformInputContent(
    modifying: bool,
    on_cancel: Callback,
    on_value: Callback<Platform>,
    value: Platform,
) -> Element {
    let position = use_context::<AppState>().position;
    let mut platform = use_signal(|| value);

    rsx! {
        PopupContent { title: if modifying { "Modify platform" } else { "Add platform" },
            div { class: "grid grid-cols-3 gap-3 pb-10 overflow-y-auto",
                ActionsPositionInput {
                    label: "X start",
                    on_icon_click: move |_| {
                        platform.write().x_start = position.peek().0;
                    },
                    on_value: move |x| {
                        platform.write().x_start = x;
                    },
                    value: platform().x_start,
                }
                ActionsPositionInput {
                    label: "X end",
                    on_icon_click: move |_| {
                        platform.write().x_end = position.peek().0;
                    },
                    on_value: move |x| {
                        platform.write().x_end = x;
                    },
                    value: platform().x_end,
                }
                ActionsPositionInput {
                    label: "Y",
                    on_icon_click: move |_| {
                        platform.write().y = position.peek().1;
                    },
                    on_value: move |y| {
                        platform.write().y = y;
                    },
                    value: platform().y,
                }
            }

            div { class: "flex w-full gap-3 absolute bottom-0 py-2 bg-secondary-surface",
                Button {
                    class: "flex-grow",
                    style: ButtonStyle::OutlinePrimary,
                    on_click: move |_| {
                        on_value(*platform.peek());
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
}

#[component]
pub fn PopupMobbingBoundInputContent(
    on_cancel: Callback,
    on_value: Callback<Bound>,
    value: Bound,
) -> Element {
    let mut value = use_signal(|| value);

    rsx! {
        PopupContent { title: "Modify mobbing bound",
            div { class: "grid grid-cols-2 gap-3 pb-10 overflow-y-auto",
                ActionsNumberInputI32 {
                    label: "X offset",
                    on_value: move |x| {
                        value.write().x = x;
                    },
                    value: value().x,
                }

                ActionsNumberInputI32 {
                    label: "Y offset",
                    on_value: move |y| {
                        value.write().y = y;
                    },
                    value: value().y,
                }

                ActionsNumberInputI32 {
                    label: "Width",
                    on_value: move |width| {
                        value.write().width = width;
                    },
                    value: value().width,
                }

                ActionsNumberInputI32 {
                    label: "Height",
                    on_value: move |height| {
                        value.write().height = height;
                    },
                    value: value().height,
                }
            }

            div { class: "flex w-full gap-3 absolute bottom-0 py-2 bg-secondary-surface",
                Button {
                    class: "flex-grow",
                    style: ButtonStyle::OutlinePrimary,
                    on_click: move |_| {
                        on_value(*value.peek());
                    },

                    "Save"
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
}

#[component]
pub fn PopupMobbingKeyInputContent(
    on_cancel: Callback,
    on_value: Callback<MobbingKey>,
    value: MobbingKey,
) -> Element {
    let value_action_key = ActionKey {
        key: value.key,
        key_hold_millis: value.key_hold_millis,
        link_key: value.link_key,
        count: value.count,
        with: value.with,
        wait_before_use_millis: value.wait_before_millis,
        wait_before_use_millis_random_range: value.wait_before_millis_random_range,
        wait_after_use_millis: value.wait_after_millis,
        wait_after_use_millis_random_range: value.wait_after_millis_random_range,
        ..ActionKey::default()
    };
    let value_action = Action::Key(value_action_key);

    rsx! {
        PopupContent { title: "Modify mobbing key",
            ActionsInput {
                switchable: false,
                modifying: true,
                linkable: false,
                positionable: false,
                directionable: false,
                bufferable: false,
                on_copy: None,
                on_cancel,
                on_value: move |(action, _)| {
                    let key = match action {
                        Action::Move(_) => unreachable!(),
                        Action::Key(action) => action,
                    };

                    let key = MobbingKey {
                        key: key.key,
                        key_hold_millis: key.key_hold_millis,
                        link_key: key.link_key,
                        count: key.count,
                        with: key.with,
                        wait_before_millis: key.wait_before_use_millis,
                        wait_before_millis_random_range: key.wait_before_use_millis_random_range,
                        wait_after_millis: key.wait_after_use_millis,
                        wait_after_millis_random_range: key.wait_after_use_millis_random_range,
                    };

                    on_value(key);
                },
                value: value_action,
            }
        }
    }
}

#[component]
pub fn PopupActionsInputContent(
    modifying: bool,
    linkable: bool,
    on_copy: Option<Callback>,
    on_cancel: Callback,
    on_value: Callback<(Action, ActionCondition)>,
    value: Action,
) -> Element {
    let name = match value.condition() {
        backend::ActionCondition::Any => "normal",
        backend::ActionCondition::EveryMillis(_) => "every milliseconds",
        backend::ActionCondition::ErdaShowerOffCooldown => "Erda Shower off cooldown",
        backend::ActionCondition::Linked => "linked",
    };
    let title = if modifying {
        format!("Modify a {name} action")
    } else {
        format!("Add a new {name} action")
    };

    rsx! {
        PopupContent { title,
            ActionsInput {
                switchable: true,
                modifying,
                linkable,
                positionable: true,
                directionable: true,
                bufferable: true,
                on_copy,
                on_cancel,
                on_value,
                value,
            }
        }
    }
}
