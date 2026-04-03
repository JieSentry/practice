//! Copied from [`DioxusLabs/components`].
//!
//! [`DioxusLabs/components`]: https://github.com/DioxusLabs/components/blob/3571d90aa55773c59b05119d9fff8879da6b8a3d/primitives/src/tooltip.rs

use dioxus::prelude::*;
use tw_merge::tw_merge;

use crate::components::{ContentAlign, ContentSide, use_controlled};

#[derive(Clone, Copy)]
struct TooltipContext {
    open: Memo<bool>,
    set_open: Callback<bool>,
}

#[derive(Props, Clone, PartialEq)]
pub struct TooltipProps {
    pub open: ReadSignal<Option<bool>>,
    #[props(default)]
    pub on_open: Callback<bool>,
    #[props(default)]
    pub class: String,
    pub children: Element,
}

#[component]
pub fn Tooltip(props: TooltipProps) -> Element {
    let class = props.class;
    let (open, set_open) = use_controlled(props.open, false, props.on_open);

    use_context_provider(|| TooltipContext { open, set_open });

    rsx! {
        div { class: tw_merge!("relative inline-block", class), {props.children} }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct TooltipTriggerProps {
    pub children: Element,
}

#[component]
pub fn TooltipTrigger(props: TooltipTriggerProps) -> Element {
    let context = use_context::<TooltipContext>();

    let handle_mouse_enter = move |_: Event<MouseData>| {
        context.set_open.call(true);
    };
    let handle_mouse_leave = move |_: Event<MouseData>| {
        context.set_open.call(false);
    };
    let handle_focus = move |_: Event<FocusData>| {
        context.set_open.call(true);
    };
    let handle_blur = move |_: Event<FocusData>| {
        context.set_open.call(false);
    };
    let handle_keydown = move |event: Event<KeyboardData>| {
        if event.key() == Key::Escape && (context.open)() {
            event.prevent_default();
            context.set_open.call(false);
        }
    };

    rsx! {
        div {
            class: "inline-block",
            onmouseenter: handle_mouse_enter,
            onmouseleave: handle_mouse_leave,
            onfocus: handle_focus,
            onblur: handle_blur,
            onkeydown: handle_keydown,
            {props.children}
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub struct TooltipContentProps {
    #[props(default = ContentSide::Top)]
    pub side: ContentSide,
    #[props(default = ContentAlign::Center)]
    pub align: ContentAlign,
    pub children: Element,
}

#[component]
pub fn TooltipContent(props: TooltipContentProps) -> Element {
    const CLASS: &str = "absolute z-1000 min-w-32 max-w-xs p-1 text-xs text-primary-text border border-primary-border bg-secondary-surface";

    let context = use_context::<TooltipContext>();
    let open_class = use_memo(move || if (context.open)() { "block" } else { "hidden" });
    let side_class = match props.side {
        ContentSide::Top => "bottom-full left-1/2 mb-2 -translate-x-1/2",
        ContentSide::Right => "left-full top-1/2 ml-2 -translate-y-1/2",
        ContentSide::Bottom => "top-full left-1/2 mt-2 -translate-x-1/2",
        ContentSide::Left => "right-full top-1/2 mr-2 -translate-y-1/2",
    };
    let align_class =
        match (props.side, props.align) {
            (ContentSide::Top, ContentAlign::Start)
            | (ContentSide::Bottom, ContentAlign::Start) => "left-0 translate-none",

            (ContentSide::Top, ContentAlign::End) | (ContentSide::Bottom, ContentAlign::End) => {
                "left-auto right-0 translate-none"
            }

            (ContentSide::Left, ContentAlign::Start)
            | (ContentSide::Right, ContentAlign::Start) => "top-0 translate-none",

            (ContentSide::Left, ContentAlign::Center)
            | (ContentSide::Right, ContentAlign::Center) => "top-1/2 -translate-y-1/2",

            (ContentSide::Left, ContentAlign::End) | (ContentSide::Right, ContentAlign::End) => {
                "top-auto bottom-0 translate-none"
            }

            (ContentSide::Top, ContentAlign::Center)
            | (ContentSide::Bottom, ContentAlign::Center) => "",
        };

    rsx! {
        div { class: tw_merge!(CLASS, open_class(), side_class, align_class,), {props.children} }
    }
}
