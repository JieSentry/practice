use std::{fmt::Display, str::FromStr};

use dioxus::prelude::*;
use num_traits::PrimInt;
use tw_merge::tw_merge;

use crate::components::{use_auto_numeric, use_controlled, use_unique_id};

const CLASS: &str = "h-6 text-xs text-primary-text outline-none px-1 border border-primary-border disabled:text-tertiary-text disabled:cursor-not-allowed w-full";

#[derive(Props, Clone, PartialEq)]
pub struct NumberInputProps<T: Clone + PartialEq + 'static> {
    value: ReadSignal<Option<T>>,
    #[props(default)]
    on_value: Callback<T>,
    #[props(default)]
    min_value: Option<T>,
    #[props(default)]
    max_value: Option<T>,
    #[props(default)]
    suffix: String,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    class: String,
}

#[component]
pub fn MillisInput(props: NumberInputProps<u64>) -> Element {
    rsx! {
        PrimitiveIntegerInput { suffix: "ms".into(), ..props }
    }
}

#[component]
pub fn PercentageInput(props: NumberInputProps<u32>) -> Element {
    rsx! {
        PrimitiveIntegerInput {
            min_value: 0.into(),
            max_value: 100.into(),
            suffix: "%".into(),
            ..props,
        }
    }
}

#[component]
pub fn PrimitiveIntegerInput<T>(props: NumberInputProps<T>) -> Element
where
    T: PrimInt + FromStr + Display,
{
    let id = use_unique_id();

    let class = props.class;
    let disabled = props.disabled;
    let min_value = props.min_value.unwrap_or(T::zero());
    let max_value = props.max_value.unwrap_or(T::max_value());
    let (value, set_value) = use_controlled(props.value, min_value, props.on_value);

    let set_numeric_value =
        use_auto_numeric(id, value, set_value, min_value, max_value, props.suffix);

    use_effect(move || {
        if let Some(value) = (props.value)() {
            set_numeric_value(value);
        }
    });

    rsx! {
        input { id: id(), disabled, class: tw_merge!(CLASS, class) }
    }
}
