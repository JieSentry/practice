use dioxus::prelude::*;

use crate::components::use_controlled_required;

#[derive(Clone, Copy, Debug)]
struct PopupContext {
    open: ReadSignal<bool>,
    set_open: Callback<bool>,
}

#[derive(Props, Clone, PartialEq)]
pub struct PopupProps {
    open: ReadSignal<bool>,
    #[props(default)]
    on_open: Callback<bool>,
    children: Element,
}

#[component]
pub fn PopupContext(props: PopupProps) -> Element {
    let (open, set_open) = use_controlled_required(props.open, props.on_open);

    use_context_provider(|| PopupContext { open, set_open });

    rsx! {
        div { {props.children} }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct PopupTriggerProps {
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn PopupTrigger(props: PopupTriggerProps) -> Element {
    let context = use_context::<PopupContext>();
    let class = props.class;

    let handle_click = move |_: Event<MouseData>| {
        context.set_open.call(true);
    };

    rsx! {
        div { class, onclick: handle_click, {props.children} }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct PopupContentProps {
    title: String,
    children: Element,
}

#[component]
pub fn PopupContent(props: PopupContentProps) -> Element {
    let title = props.title;
    let context = use_context::<PopupContext>();
    let open = context.open.cloned();
    let visible_class = if open { "visible" } else { "invisible" };

    rsx! {
        div { class: "absolute inset-0 z-1000 bg-primary-surface/80 flex {visible_class}",
            div { class: "bg-secondary-surface px-2 w-fit h-fit m-auto",
                div { class: "flex flex-col h-full gap-2 relative",
                    div { class: "flex flex-none items-center text-xs text-primary-text font-medium h-10",
                        {title}
                    }
                    {props.children}
                }
            }
        }
    }
}
