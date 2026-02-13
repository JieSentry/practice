use backend::{Character, KeyBindingConfiguration};
use dioxus::prelude::*;

use crate::{
    characters::{CharactersContext, CharactersKeyBindingConfigurationInput},
    components::section::Section,
};

#[component]
pub fn SectionKeyBindings() -> Element {
    let context = use_context::<CharactersContext>();
    let character = context.character;
    let save_character = context.save_character;

    rsx! {
        Section { title: "Key bindings",
            div { class: "grid grid-cols-2 2xl:grid-cols-4 gap-4",
                CharactersKeyBindingConfigurationInput {
                    label: "Rope lift",
                    optional: true,
                    disabled: character().id.is_none(),
                    on_value: move |ropelift_key| {
                        save_character(Character {
                            ropelift_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().ropelift_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Teleport",
                    optional: true,
                    disabled: character().id.is_none(),
                    on_value: move |teleport_key| {
                        save_character(Character {
                            teleport_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().teleport_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Jump",
                    disabled: character().id.is_none(),
                    on_value: move |key_config: Option<KeyBindingConfiguration>| {
                        save_character(Character {
                            jump_key: key_config.expect("not optional"),
                            ..character.peek().clone()
                        });
                    },
                    value: character().jump_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Up jump",
                    optional: true,
                    tooltip: "This is meant for classes that have a separate skill to up jump. Classes that use up arrow should set this key to up arrow.",
                    disabled: character().id.is_none(),
                    on_value: move |up_jump_key| {
                        save_character(Character {
                            up_jump_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().up_jump_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Interact",
                    disabled: character().id.is_none(),
                    on_value: move |key_config: Option<KeyBindingConfiguration>| {
                        save_character(Character {
                            interact_key: key_config.expect("not optional"),
                            ..character.peek().clone()
                        });
                    },
                    value: character().interact_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Cash shop",
                    optional: true,
                    disabled: character().id.is_none(),
                    tooltip: "Cash shop is used to reset spin rune to a normal rune. This only happens if solving rune fails 8 times consecutively.",
                    on_value: move |cash_shop_key| {
                        save_character(Character {
                            cash_shop_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().cash_shop_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "To town",
                    optional: true,
                    disabled: character().id.is_none(),
                    tooltip: "This key must be set to use navigation or run/stop cycle features.",
                    on_value: move |to_town_key| {
                        save_character(Character {
                            to_town_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().to_town_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Change channel",
                    optional: true,
                    disabled: character().id.is_none(),
                    tooltip: "This key must be set to use panic mode or elite boss spawns behavior features.",
                    on_value: move |change_channel_key| {
                        save_character(Character {
                            change_channel_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().change_channel_key,
                }
                CharactersKeyBindingConfigurationInput {
                    label: "Familiar menu",
                    optional: true,
                    tooltip: "This key must be set to use familiars swapping feature.",
                    disabled: character().id.is_none(),
                    on_value: move |familiar_menu_key| {
                        save_character(Character {
                            familiar_menu_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().familiar_menu_key,
                }
            }
        }
    }
}
