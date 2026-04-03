use dioxus::{core::use_drop, prelude::*};

use crate::components::use_unique_id;

const SELECT_CLASS: &str = "items-center text-xs text-primary-text outline-none px-1 border border-primary-border disabled:text-tertiary-text disabled:cursor-not-allowed whitespace-nowrap text-ellipsis";

const OPTION_CLASS: &str =
    "bg-secondary-surface text-xs text-primary-text pl-1 pr-2 hover:bg-tertiary-surface";

#[derive(Clone, Copy)]
struct SelectContext<T: Clone + PartialEq + 'static> {
    options: Signal<Vec<OptionState<T>>>,
}

#[derive(Clone)]
struct OptionState<T: Clone + PartialEq + 'static> {
    id: String,
    selected: ReadSignal<bool>,
    value: ReadSignal<T>,
}

#[derive(PartialEq, Props, Clone)]
pub struct SelectProps<T: Clone + PartialEq + 'static> {
    #[props(default)]
    on_selected: Callback<T>,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    placeholder: String,
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn Select<T>(props: SelectProps<T>) -> Element
where
    T: Clone + PartialEq + 'static,
{
    let class = props.class;
    let disabled = props.disabled;
    let on_selected = props.on_selected;

    let options = use_signal::<Vec<OptionState<T>>>(Vec::new);

    use_context_provider(|| SelectContext { options });

    rsx! {
        select {
            class: "{SELECT_CLASS} {class}",
            disabled,
            onchange: move |e| {
                let id = e.value();
                let option = options.iter().find(|state| state.id == id);
                let Some(option) = option else {
                    return;
                };
                let value = option.value.peek().clone();
                on_selected(value);
            },
            option {
                class: "{OPTION_CLASS}",
                selected: options.iter().all(|state| !state.selected.cloned()),
                disabled: true,
                hidden: true,
                value: "",
                label: props.placeholder,
            }
            {props.children}
        }
    }
}

#[derive(PartialEq, Props, Clone)]
pub struct SelectOptionProps<T: Clone + PartialEq + 'static> {
    value: ReadSignal<T>,
    #[props(default)]
    label: Option<String>,
    #[props(default)]
    selected: ReadSignal<bool>,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    class: String,
}

#[component]
pub fn SelectOption<T>(props: SelectOptionProps<T>) -> Element
where
    T: Clone + PartialEq + 'static,
{
    let class = props.class;
    let disabled = props.disabled;
    let label = props.label;
    let selected = props.selected;
    let id = use_unique_id();

    let mut context = use_context::<SelectContext<T>>();

    use_effect(move || {
        if selected() {
            document::eval(
                format!(
                    r#"
                    const option = document.getElementById("{id}");
                    if (option !== null && !option.selected) {{
                        option.selected = true;
                    }}
                    "#
                )
                .as_str(),
            );
        }
    });

    use_effect(move || {
        context.options.push(OptionState {
            id: id(),
            selected,
            value: props.value,
        });
    });

    use_drop(move || {
        context.options.retain(|state| state.id != id());
    });

    rsx! {
        option {
            id: id(),
            class: "{OPTION_CLASS} {class}",
            selected,
            disabled,
            value: id(),
            label,
        }
    }
}
