use dioxus::prelude::*;
use tw_merge::tw_merge;

const CLASS: &str = "inline-block h-6 text-xs text-center font-medium content-center px-2 bg-secondary-surface data-[disabled=false]:hover:bg-tertiary-surface data-[disabled=false]:cursor-pointer data-[disabled=true]:cursor-not-allowed data-[disabled=true]:text-tertiary-text";

#[derive(Copy, Clone, PartialEq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    OutlinePrimary,
    OutlineSecondary,
    Danger,
}

#[derive(Props, Clone, PartialEq)]
pub struct ButtonProps {
    #[props(default)]
    on_click: Callback,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default = ButtonStyle::Primary)]
    style: ButtonStyle,
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn Button(props: ButtonProps) -> Element {
    let class = props.class;
    let text_class = match props.style {
        ButtonStyle::Primary | ButtonStyle::OutlinePrimary => "text-primary-text",
        ButtonStyle::Secondary | ButtonStyle::OutlineSecondary => "text-secondary-text",
        ButtonStyle::Danger => "text-danger-text",
    };
    let border_class = match props.style {
        ButtonStyle::Danger | ButtonStyle::Primary | ButtonStyle::Secondary => "",
        ButtonStyle::OutlinePrimary | ButtonStyle::OutlineSecondary => {
            "border border-primary-border"
        }
    };

    rsx! {
        div {
            role: "button",
            class: tw_merge!(CLASS, text_class, border_class, class),
            "data-disabled": props.disabled,
            onclick: move |_| {
                if *props.disabled.peek() {
                    return;
                }
                props.on_click.call(());
            },
            {props.children}
        }
    }
}
