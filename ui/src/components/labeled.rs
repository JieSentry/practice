use dioxus::prelude::*;
use tw_merge::tw_merge;

use crate::components::{
    ContentAlign, ContentSide,
    icons::InfoIcon,
    tooltip::{Tooltip, TooltipContent, TooltipTrigger},
};

const DIV_CLASS: &str = "flex flex-col gap-1";
const LABEL_CLASS: &str =
    "text-xxs text-secondary-text whitespace-nowrap overflow-hidden text-ellipsis";

#[derive(Clone, PartialEq, Props)]
pub struct LabeledProps {
    label: String,
    children: Element,
    #[props(default)]
    tooltip: Option<String>,
    #[props(default = ContentSide::Top)]
    tooltip_side: ContentSide,
    #[props(default = ContentAlign::Start)]
    tooltip_align: ContentAlign,
    #[props(default)]
    class: String,
}

#[component]
pub fn Labeled(props: LabeledProps) -> Element {
    let class = props.class;
    let tooltip = props.tooltip;
    let side = props.tooltip_side;
    let align = props.tooltip_align;

    rsx! {
        div { class: tw_merge!(DIV_CLASS, class),
            div { class: "flex gap-1",
                label { class: LABEL_CLASS, {props.label} }
                if let Some(tooltip) = tooltip {
                    Tooltip { class: "leading-0",
                        TooltipTrigger {
                            InfoIcon { class: "fill-secondary-icon" }
                        }
                        TooltipContent { side, align, {tooltip} }
                    }
                }
            }
            {props.children}
        }
    }
}
