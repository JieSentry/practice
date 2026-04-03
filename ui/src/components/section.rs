use dioxus::prelude::*;
use tw_merge::tw_merge;

#[derive(Props, PartialEq, Clone)]
pub struct SectionProps {
    title: String,
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn Section(props: SectionProps) -> Element {
    let class = props.class;

    rsx! {
        div { class: tw_merge!("flex flex-col pr-4 pb-3", class),
            div { class: "flex items-center text-xs text-primary-text font-medium h-10",
                {props.title}
            }
            {props.children}
        }
    }
}
