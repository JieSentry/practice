use backend::{Map, Platform, key_receiver};
use dioxus::prelude::*;
use tokio::sync::broadcast::error::RecvError;

use crate::{
    AppState,
    actions::{
        ActionsCheckbox, ActionsContext, ActionsUpdate, ITEM_BORDER_CLASS, ITEM_TEXT_CLASS,
        popup::PopupPlatformInputContent,
    },
    components::{
        button::{Button, ButtonStyle},
        icons::XIcon,
        popup::{PopupContext, PopupTrigger},
        section::Section,
    },
};

#[derive(PartialEq, Clone, Copy)]
enum PopupContent {
    None,
    Edit { platform: Platform, index: usize },
    Add,
}

#[inline]
fn update_valid_platform_end(platform: &mut Platform) {
    platform.x_end = if platform.x_end <= platform.x_start {
        platform.x_start + 1
    } else {
        platform.x_end
    };
}

#[component]
pub fn SectionPlatforms(disabled: bool) -> Element {
    let coroutine = use_coroutine_handle::<ActionsUpdate>();
    let settings = use_context::<AppState>().settings;
    let position = use_context::<AppState>().position;
    let context = use_context::<ActionsContext>();

    let map = context.map;
    let save_map = context.save_map;

    let add_platform = move |platform| {
        let mut map = map();

        map.platforms.push(platform);
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    };

    let edit_platform = move |new_platform: Platform, index: usize| {
        let mut map = map();
        let Some(platform) = map.platforms.get_mut(index) else {
            return;
        };

        *platform = new_platform;
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    };

    let delete_platform = move |index| {
        let mut map = map();

        map.platforms.remove(index);
        coroutine.send(ActionsUpdate::UpdateMinimap(map));
    };

    let mut popup_content = use_signal(|| PopupContent::None);
    let mut popup_open = use_signal(|| false);

    use_future(move || async move {
        let mut platform = Platform::default();
        let mut key_receiver = key_receiver().await;
        loop {
            let key = match key_receiver.recv().await {
                Ok(value) => value,
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(_)) => continue,
            };
            let Some(settings) = &*settings.peek() else {
                continue;
            };

            if settings.platform_start_key.enabled && settings.platform_start_key.key == key {
                platform.x_start = position.peek().0;
                update_valid_platform_end(&mut platform);
                platform.y = position.peek().1;
                continue;
            }

            if settings.platform_end_key.enabled && settings.platform_end_key.key == key {
                platform.x_end = position.peek().0;
                update_valid_platform_end(&mut platform);
                platform.y = position.peek().1;
                continue;
            }

            if settings.platform_add_key.enabled && settings.platform_add_key.key == key {
                update_valid_platform_end(&mut platform);
                add_platform(platform);
                continue;
            }
        }
    });

    rsx! {
        PopupContext {
            open: popup_open,
            on_open: move |open: bool| {
                popup_open.set(open);
            },
            Section { title: "Platforms",
                div { class: "grid grid-cols-2 gap-3",
                    ActionsCheckbox {
                        label: "Rune pathing",
                        disabled,
                        on_checked: move |rune_platforms_pathing| {
                            save_map(Map {
                                rune_platforms_pathing,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().rune_platforms_pathing,
                    }

                    ActionsCheckbox {
                        label: "Up jump only",
                        disabled: disabled || !map().rune_platforms_pathing,
                        on_checked: move |rune_platforms_pathing_up_jump_only| {
                            save_map(Map {
                                rune_platforms_pathing_up_jump_only,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().rune_platforms_pathing_up_jump_only,
                    }

                    ActionsCheckbox {
                        label: "Auto-mobbing pathing",
                        disabled,
                        on_checked: move |auto_mob_platforms_pathing| {
                            save_map(Map {
                                auto_mob_platforms_pathing,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().auto_mob_platforms_pathing,
                    }

                    ActionsCheckbox {
                        label: "Up jump only",
                        disabled: disabled || !map().auto_mob_platforms_pathing,
                        on_checked: move |auto_mob_platforms_pathing_up_jump_only| {
                            save_map(Map {
                                auto_mob_platforms_pathing_up_jump_only,
                                ..map.peek().clone()
                            })
                        },
                        checked: map().auto_mob_platforms_pathing_up_jump_only,
                    }
                }

                if !map().platforms.is_empty() {
                    div { class: "mt-2" }
                }

                for (index , platform) in map().platforms.into_iter().enumerate() {
                    PopupTrigger {
                        PlatformItem {
                            platform,
                            on_item_click: move |_| {
                                popup_content
                                    .set(PopupContent::Edit {
                                        platform,
                                        index,
                                    });
                            },
                            on_item_delete: move |_| {
                                delete_platform(index);
                            },
                        }
                    }
                }

                PopupTrigger {
                    Button {
                        style: ButtonStyle::Secondary,
                        on_click: move |_| {
                            popup_content.set(PopupContent::Add);
                        },
                        disabled,
                        class: "mt-2 w-full",

                        "Add platform"
                    }
                }

                PopupPlatformInputContent {
                    modifying: match popup_content() {
                        PopupContent::None | PopupContent::Add => false,
                        PopupContent::Edit { .. } => true,
                    },
                    on_cancel: move |_| {
                        popup_open.set(false);
                    },
                    on_value: move |mut platform| {
                        update_valid_platform_end(&mut platform);
                        let content = *popup_content.peek();
                        match content {
                            PopupContent::None => unreachable!(),
                            PopupContent::Add => add_platform(platform),
                            PopupContent::Edit { index, .. } => edit_platform(platform, index),
                        }
                        popup_open.set(false);
                    },
                    value: match popup_content() {
                        PopupContent::None | PopupContent::Add => Platform::default(),
                        PopupContent::Edit { platform, .. } => platform,
                    },
                }
            }
        }
    }
}

#[component]
fn PlatformItem(platform: Platform, on_item_click: Callback, on_item_delete: Callback) -> Element {
    const ICON_CONTAINER_CLASS: &str = "w-4 h-6 flex justify-center items-center";
    const ICON_CLASS: &str = "size-3";

    rsx! {
        div { class: "flex group",
            div {
                class: "flex-grow grid grid-cols-2 h-6 text-xxs gap-2 text-secondary-text group-hover:bg-secondary-surface",
                onclick: move |_| {
                    on_item_click(());
                },
                div { class: "{ITEM_BORDER_CLASS} {ITEM_TEXT_CLASS}",
                    {format!("X / {} - {}", platform.x_start, platform.x_end)}
                }
                div { class: "{ITEM_TEXT_CLASS}", {format!("Y / {}", platform.y)} }
            }
            div { class: "self-stretch invisible group-hover:visible group-hover:bg-secondary-surface flex items-center pr-1",
                div {
                    class: ICON_CONTAINER_CLASS,
                    onclick: move |e| {
                        e.stop_propagation();
                        on_item_delete(());
                    },
                    XIcon { class: "{ICON_CLASS}" }
                }
            }
        }
    }
}
