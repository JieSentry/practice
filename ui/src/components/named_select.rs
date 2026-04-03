use dioxus::prelude::*;
use tw_merge::tw_merge;

use crate::components::{
    button::{Button, ButtonStyle},
    text::TextInput,
};

const BUTTON_CLASS: &str = "w-20 h-full flex-none";

#[derive(Props, PartialEq, Clone)]
pub struct NamedSelectProps {
    #[props(default)]
    on_create: Callback<String>,
    #[props(default)]
    on_delete: Callback,
    #[props(default)]
    class: String,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    delete_disabled: ReadSignal<bool>,
    children: Element,
}

#[derive(Clone, PartialEq)]
enum State {
    Select,
    Create { name: String, error: bool },
}

#[component]
pub fn NamedSelect(props: NamedSelectProps) -> Element {
    let class = props.class;
    let disabled = props.disabled;
    let delete_disabled = props.delete_disabled;

    let mut state = use_signal(|| State::Select);

    let handle_click_first = move |_| {
        let current_state = state.peek().clone();
        match current_state {
            State::Select => state.set(State::Create {
                name: String::default(),
                error: false,
            }),
            State::Create { name, .. } => {
                if name.is_empty() {
                    state.set(State::Create { name, error: true });
                } else {
                    state.set(State::Select);
                    props.on_create.call(name);
                }
            }
        }
    };

    let handle_click_second = move |_| {
        let current_state = state.peek().clone();
        match current_state {
            State::Select => props.on_delete.call(()),
            State::Create { .. } => {
                state.set(State::Select);
            }
        }
    };

    rsx! {
        div { class: tw_merge!("flex gap-3 h-6", class),
            div { class: "flex-grow",
                match state() {
                    State::Select => rsx! {
                        {props.children}
                    },
                    State::Create { name, error } => rsx! {
                        TextInput {
                            class: "size-full",
                            placeholder: "Enter a name...",
                            value: name,
                            disabled,
                            on_value: move |name| {
                                state.set(State::Create { name, error });
                            },
                        }
                    },
                }
            }
            Button {
                class: BUTTON_CLASS,
                style: ButtonStyle::Primary,
                disabled,
                on_click: handle_click_first,
                match state() {
                    State::Select => "Create",
                    State::Create { .. } => "Save",
                }
            }
            Button {
                class: BUTTON_CLASS,
                style: ButtonStyle::Danger,
                disabled: match state() {
                    State::Select => delete_disabled(),
                    State::Create { .. } => false,
                },
                on_click: handle_click_second,
                match state() {
                    State::Select => "Delete",
                    State::Create { .. } => "Cancel",
                }
            }
        }
    }
}
