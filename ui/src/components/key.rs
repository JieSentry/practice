use std::rc::Rc;

use backend::KeyBinding;
use dioxus::prelude::*;
use tw_merge::tw_merge;

use crate::components::{icons::XIcon, use_controlled};

const DIV_CLASS: &str = "inline-block relative bg-secondary-surface h-6 group";
const INPUT_CLASS: &str = "absolute inset-0 outline-none size-full text-center text-xs text-secondary-text disabled:cursor-not-allowed disabled:text-tertiary-text";
const ACTIVE_DIV_CLASS: &str =
    "absolute inset-0 flex items-center justify-center bg-secondary-surface text-xs";
const OPTIONAL_DIV_CLASS: &str =
    "absolute invisible group-hover:visible top-0 right-1 w-fit h-full flex items-center";

#[derive(PartialEq, Props, Clone)]
pub struct KeyInputProps {
    value: ReadSignal<Option<Option<KeyBinding>>>,
    #[props(default)]
    on_value: Callback<Option<KeyBinding>>,
    #[props(default)]
    active: ReadSignal<Option<bool>>,
    #[props(default)]
    on_active: Callback<bool>,
    #[props(default)]
    optional: bool,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    class: String,
}

#[component]
pub fn KeyInput(props: KeyInputProps) -> Element {
    let class = props.class;
    let optional = props.optional;
    let disabled = props.disabled;
    let (value, set_value) = use_controlled(props.value, None, props.on_value);
    let (active, set_active) = use_controlled(props.active, false, props.on_active);

    let mut error = use_signal(|| false);
    let mut input = use_signal::<Option<Rc<MountedData>>>(|| None);

    let handle_focus = move |_: Event<_>| {
        set_active(true);
    };

    let handle_blur = move |_: Event<_>| {
        set_active(false);
        error.set(false);
    };

    let handle_key_down = move |e: Event<KeyboardData>| async move {
        e.prevent_default();
        if let Some(key) = map_key(e.key()) {
            if let Some(input) = input().as_ref() {
                let _ = input.set_focus(false).await;
            }

            error.set(false);
            set_active(false);
            set_value(Some(key));
        } else {
            error.set(true);
        }
    };

    rsx! {
        div { class: tw_merge!(DIV_CLASS, class),
            input {
                r#type: "text",
                disabled,
                onmounted: move |e| {
                    input.set(Some(e.data()));
                },
                class: INPUT_CLASS,
                readonly: true,
                onfocus: handle_focus,
                onblur: handle_blur,
                onkeydown: handle_key_down,
                placeholder: "Click to set",
                value: value().map(|key| key.to_string()),
            }
            if active() {
                div {
                    class: if error() { "{ACTIVE_DIV_CLASS} text-danger-text" },
                    class: if !error() { "{ACTIVE_DIV_CLASS} text-primary-text" },
                    "Press any key..."
                }
            }
            if optional && !active() && value().is_some() {
                div { class: OPTIONAL_DIV_CLASS,
                    div {
                        onclick: move |_| {
                            set_value(None);
                        },
                        XIcon { class: "size-3" }
                    }
                }
            }
        }
    }
}

fn map_key(key: Key) -> Option<KeyBinding> {
    Some(match key {
        Key::Character(s) => match s.to_lowercase().as_str() {
            "a" => KeyBinding::A,
            "b" => KeyBinding::B,
            "c" => KeyBinding::C,
            "d" => KeyBinding::D,
            "e" => KeyBinding::E,
            "f" => KeyBinding::F,
            "g" => KeyBinding::G,
            "h" => KeyBinding::H,
            "i" => KeyBinding::I,
            "j" => KeyBinding::J,
            "k" => KeyBinding::K,
            "l" => KeyBinding::L,
            "m" => KeyBinding::M,
            "n" => KeyBinding::N,
            "o" => KeyBinding::O,
            "p" => KeyBinding::P,
            "q" => KeyBinding::Q,
            "r" => KeyBinding::R,
            "s" => KeyBinding::S,
            "t" => KeyBinding::T,
            "u" => KeyBinding::U,
            "v" => KeyBinding::V,
            "w" => KeyBinding::W,
            "x" => KeyBinding::X,
            "y" => KeyBinding::Y,
            "z" => KeyBinding::Z,
            "0" => KeyBinding::Zero,
            "1" => KeyBinding::One,
            "2" => KeyBinding::Two,
            "3" => KeyBinding::Three,
            "4" => KeyBinding::Four,
            "5" => KeyBinding::Five,
            "6" => KeyBinding::Six,
            "7" => KeyBinding::Seven,
            "8" => KeyBinding::Eight,
            "9" => KeyBinding::Nine,
            "`" => KeyBinding::Tilde,
            "'" => KeyBinding::Quote,
            ";" => KeyBinding::Semicolon,
            "," => KeyBinding::Comma,
            "." => KeyBinding::Period,
            "/" => KeyBinding::Slash,
            " " => KeyBinding::Space,
            _ => return None,
        },
        Key::F1 => KeyBinding::F1,
        Key::F2 => KeyBinding::F2,
        Key::F3 => KeyBinding::F3,
        Key::F4 => KeyBinding::F4,
        Key::F5 => KeyBinding::F5,
        Key::F6 => KeyBinding::F6,
        Key::F7 => KeyBinding::F7,
        Key::F8 => KeyBinding::F8,
        Key::F9 => KeyBinding::F9,
        Key::F10 => KeyBinding::F10,
        Key::F11 => KeyBinding::F11,
        Key::F12 => KeyBinding::F12,
        Key::ArrowUp => KeyBinding::Up,
        Key::ArrowLeft => KeyBinding::Left,
        Key::ArrowRight => KeyBinding::Right,
        Key::ArrowDown => KeyBinding::Down,
        Key::Home => KeyBinding::Home,
        Key::End => KeyBinding::End,
        Key::PageUp => KeyBinding::PageUp,
        Key::PageDown => KeyBinding::PageDown,
        Key::Insert => KeyBinding::Insert,
        Key::Delete => KeyBinding::Delete,
        Key::Enter => KeyBinding::Enter,
        Key::Escape => KeyBinding::Esc,
        Key::Shift => KeyBinding::Shift,
        Key::Control => KeyBinding::Ctrl,
        Key::Alt => KeyBinding::Alt,
        _ => return None,
    })
}
