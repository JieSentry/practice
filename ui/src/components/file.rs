use dioxus::{html::FileData, prelude::*};
use tw_merge::tw_merge;

use crate::components::use_unique_id;

#[derive(Props, PartialEq, Clone)]
pub struct FileOutputProps {
    #[props(default)]
    on_file: Callback<(), Vec<u8>>,
    #[props(default = "application/json".to_string())]
    file_type: String,
    #[props(default = "download.json".to_string())]
    download: String,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn FileOutput(props: FileOutputProps) -> Element {
    let class = props.class;
    let disabled = props.disabled;
    let file_type = props.file_type;
    let download = props.download;

    let id = use_unique_id();
    let disabled = disabled();
    let handle_output = move |_| {
        if disabled {
            return;
        }

        let bytes = props.on_file.call(());
        let js = format!(
            r#"
            const element = document.getElementById("{}");
            if (element === null) {{
                return;
            }}

            const bytes = new Uint8Array(await dioxus.recv());
            let blobData = bytes;
            if ("{file_type}".startsWith("text/") || "{file_type}".includes("json")) {{
                blobData = new TextDecoder("utf-8").decode(bytes.buffer);
            }}

            const blob = new Blob([blobData], {{ type: "{file_type}" }});
            const url = URL.createObjectURL(blob);

            element.setAttribute("href", url);
            element.setAttribute("download", "{download}");
            element.addEventListener('click', e => e.stopPropagation(), {{ once: true }});
            element.click();
            "#,
            id(),
        );
        let eval = document::eval(js.as_str());
        let _ = eval.send(bytes);
    };

    rsx! {
        div {
            class: tw_merge!("inline-block relative", class),
            onclick: handle_output,
            a { id, class: "sr-only" }
            {props.children}
        }
    }
}

#[derive(Props, PartialEq, Clone)]
pub struct FileInputProps {
    #[props(default)]
    on_file: Callback<FileData>,
    #[props(default = ".json,application/json".to_string())]
    accept: String,
    #[props(default)]
    disabled: ReadSignal<bool>,
    #[props(default)]
    class: String,
    children: Element,
}

#[component]
pub fn FileInput(props: FileInputProps) -> Element {
    let class = props.class;
    let accept = props.accept;
    let disabled = props.disabled;

    let handle_on_change = move |e: Event<FormData>| {
        if let Some(file) = e.data.files().into_iter().next() {
            props.on_file.call(file);
        }
    };

    rsx! {
        label { class: tw_merge!("inline-block relative", class),
            input {
                class: "sr-only",
                r#type: "file",
                accept,
                disabled,
                onchange: handle_on_change,
            }
            {props.children}
        }
    }
}
