use backend::{
    DebugState, TransparentShapeDifficulty, auto_record_lie_detector, auto_save_rune,
    debug_state_receiver, record_video, test_spin_rune, test_transparent_shape, test_violetta,
};
use dioxus::prelude::*;
use tokio::sync::broadcast::error::RecvError;

use crate::components::{
    button::{Button, ButtonStyle},
    section::Section,
};

#[component]
pub fn DebugScreen() -> Element {
    let mut state = use_signal(DebugState::default);

    use_future(move || async move {
        let mut rx = debug_state_receiver().await;
        loop {
            let current_state = match rx.recv().await {
                Ok(state) => state,
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(_)) => continue,
            };
            if current_state != *state.peek() {
                state.set(current_state);
            }
        }
    });

    rsx! {
        div { class: "flex flex-col h-full overflow-y-auto",
            Section { title: "Debug",
                div { class: "grid grid-cols-2 gap-3",
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async {
                            test_spin_rune().await;
                        },

                        "Test spin rune"
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async {
                            test_violetta().await;
                        },

                        "Test Violetta"
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async {
                            test_transparent_shape(TransparentShapeDifficulty::Normal).await;
                        },

                        "Test transparent shape normal"
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async {
                            test_transparent_shape(TransparentShapeDifficulty::Hard).await;
                        },

                        "Test transparent shape hard"
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async move {
                            record_video(!state.peek().is_recording).await;
                        },

                        if state().is_recording {
                            "Stop recording"
                        } else {
                            "Start recording"
                        }
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async move {
                            auto_save_rune(!state.peek().is_rune_auto_saving).await;
                        },

                        if state().is_rune_auto_saving {
                            "Stop auto saving rune"
                        } else {
                            "Start auto saving rune"
                        }
                    }
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| async move {
                            let recording = state.peek().is_lie_detector_auto_recording;
                            auto_record_lie_detector(!recording).await;
                        },

                        if state().is_lie_detector_auto_recording {
                            "Stop auto record lie detector"
                        } else {
                            "Start auto record lie detector"
                        }
                    }
                }
            }
        }
    }
}
