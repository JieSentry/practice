use dioxus::prelude::*;
use tw_merge::tw_merge;

use crate::components::use_controlled;

#[derive(Props, PartialEq, Clone)]
pub struct TextInputProps {
    value: ReadSignal<Option<String>>,
    #[props(default)]
    on_value: Callback<String>,
    #[props(default)]
    sensitive: bool,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    placeholder: Option<String>,
    #[props(default)]
    class: String,
}

#[component]
pub fn TextInput(props: TextInputProps) -> Element {
    const DIV_CLASS: &str = "inline-block text-xs text-primary-text px-1 border border-primary-border disabled:text-tertiary-text";
    const INPUT_CLASS: &str =
        "outline-none disabled:cursor-not-allowed size-full whitespace-nowrap text-ellipsis";

    let placeholder = props.placeholder;
    let disabled = props.disabled;
    let sensitive = props.sensitive;
    let class = props.class;
    let (value, set_value) = use_controlled(props.value, String::default(), props.on_value);

    rsx! {
        div { class: tw_merge!(DIV_CLASS, class), "data-disabled": disabled,
            input {
                class: INPUT_CLASS,
                disabled,
                placeholder,
                r#type: if sensitive { "password" } else { "text" },
                oninput: move |e| {
                    set_value(e.parsed::<String>().unwrap());
                },
                value,
            }
        }
    }
}
