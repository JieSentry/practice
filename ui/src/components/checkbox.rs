use dioxus::prelude::*;

use crate::components::use_controlled;

#[derive(Props, Clone, PartialEq)]
pub struct CheckboxProps {
    checked: ReadSignal<Option<bool>>,
    #[props(default)]
    on_checked: Callback<bool>,
    #[props(default)]
    disabled: ReadSignal<bool>,
}

#[component]
pub fn Checkbox(props: CheckboxProps) -> Element {
    let (checked, set_checked) = use_controlled(props.checked, false, props.on_checked);

    rsx! {
        div { class: "inline-block size-6 border border-primary-border",
            input {
                class: "appearance-none size-full disabled:cursor-not-allowed",
                r#type: "checkbox",
                disabled: props.disabled,
                "data-disabled": props.disabled,
                onclick: move |_| {
                    set_checked(!checked());
                },
                checked,
            }
        }
    }
}
