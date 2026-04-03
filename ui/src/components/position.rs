use dioxus::prelude::*;

use crate::components::{icons::PositionIcon, numbers::PrimitiveIntegerInput};

const ICON_CONTAINER_CLASS: &str =
    "absolute invisible group-hover:visible top-0 right-1 w-fit h-full flex items-center";
const ICON_CLASS: &str = "size-3";

#[derive(Props, PartialEq, Clone)]
pub struct PositionInputProps {
    value: i32,
    on_value: Callback<i32>,
    on_icon_click: ReadSignal<Option<Callback>>,
    #[props(default)]
    disabled: ReadSignal<bool>,
}

#[component]
pub fn PositionInput(props: PositionInputProps) -> Element {
    let value = props.value;
    let on_value = props.on_value;
    let on_icon_click = props.on_icon_click;
    let disabled = props.disabled;

    rsx! {
        div { class: "relative group inline-block leading-0",
            PrimitiveIntegerInput { on_value, value, disabled }

            if let Some(on_icon_click) = on_icon_click() {
                div {
                    class: ICON_CONTAINER_CLASS,
                    onclick: move |_| {
                        on_icon_click(());
                    },
                    PositionIcon { class: ICON_CLASS }
                }
            }
        }
    }
}
