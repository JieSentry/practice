use backend::{Character, KeyBindingConfiguration};
use dioxus::prelude::*;

use crate::{
    characters::{CharactersCheckbox, CharactersContext, CharactersKeyBindingConfigurationInput},
    components::section::Section,
};

#[component]
pub fn SectionBuffs() -> Element {
    let context = use_context::<CharactersContext>();
    let character = context.character;
    let save_character = context.save_character;
    let disabled = use_memo(move || character().id.is_none());

    rsx! {
        Section { title: "Buffs",
            div { class: "grid grid-cols-2 xl:grid-cols-4 gap-4",
                div { class: "col-span-full flex gap-2",
                    CharactersKeyBindingConfigurationInput {
                        label: "Familiar skill",
                        label_class: "flex-1",
                        disabled,
                        on_value: move |key_config: Option<KeyBindingConfiguration>| {
                            save_character(Character {
                                familiar_buff_key: key_config.expect("not optional"),
                                ..character.peek().clone()
                            });
                        },
                        value: character().familiar_buff_key,
                    }
                    CharactersKeyBindingConfigurationInput {
                        label: "Familiar essence",
                        label_class: "flex-1",
                        disabled,
                        on_value: move |key_config: Option<KeyBindingConfiguration>| {
                            save_character(Character {
                                familiar_essence_key: key_config.expect("not optional"),
                                ..character.peek().clone()
                            });
                        },
                        value: character().familiar_essence_key,
                    }
                    CharactersCheckbox {
                        label: "Enabled",
                        checked: character().familiar_buff_key.enabled,
                        on_checked: move |enabled| {
                            let character = character.peek().clone();
                            save_character(Character {
                                familiar_buff_key: KeyBindingConfiguration {
                                    enabled,
                                    ..character.familiar_buff_key
                                },
                                ..character
                            });
                        },
                        disabled,
                    }
                }
                Buff {
                    label: "Sayram's Elixir",
                    disabled,
                    on_value: move |sayram_elixir_key| {
                        save_character(Character {
                            sayram_elixir_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().sayram_elixir_key,
                }
                Buff {
                    label: "Aurelia's Elixir",
                    disabled,
                    on_value: move |aurelia_elixir_key| {
                        save_character(Character {
                            aurelia_elixir_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().aurelia_elixir_key,
                }
                Buff {
                    label: "2x EXP Coupon",
                    disabled,
                    on_value: move |exp_x2_key| {
                        save_character(Character {
                            exp_x2_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().exp_x2_key,
                }
                Buff {
                    label: "3x EXP Coupon",
                    disabled,
                    on_value: move |exp_x3_key| {
                        save_character(Character {
                            exp_x3_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().exp_x3_key,
                }
                Buff {
                    label: "4x EXP Coupon",
                    disabled,
                    on_value: move |exp_x4_key| {
                        save_character(Character {
                            exp_x4_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().exp_x4_key,
                }
                Buff {
                    label: "50% Bonus EXP Coupon",
                    disabled,
                    on_value: move |bonus_exp_key| {
                        save_character(Character {
                            bonus_exp_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().bonus_exp_key,
                }
                Buff {
                    label: "Legion's Wealth",
                    disabled,
                    on_value: move |legion_wealth_key| {
                        save_character(Character {
                            legion_wealth_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().legion_wealth_key,
                }
                Buff {
                    label: "Legion's Luck",
                    disabled,
                    on_value: move |legion_luck_key| {
                        save_character(Character {
                            legion_luck_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().legion_luck_key,
                }
                Buff {
                    label: "Wealth Acquisition Potion",
                    disabled,
                    on_value: move |wealth_acquisition_potion_key| {
                        save_character(Character {
                            wealth_acquisition_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().wealth_acquisition_potion_key,
                }
                Buff {
                    label: "EXP Accumulation Potion",
                    disabled,
                    on_value: move |exp_accumulation_potion_key| {
                        save_character(Character {
                            exp_accumulation_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().exp_accumulation_potion_key,
                }
                Buff {
                    label: "Small Wealth Acquisition Potion",
                    disabled,
                    on_value: move |small_wealth_acquisition_potion_key| {
                        save_character(Character {
                            small_wealth_acquisition_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().small_wealth_acquisition_potion_key,
                }
                Buff {
                    label: "Small EXP Accumulation Potion",
                    disabled,
                    on_value: move |small_exp_accumulation_potion_key| {
                        save_character(Character {
                            small_exp_accumulation_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().small_exp_accumulation_potion_key,
                }
                Buff {
                    label: "For The Guild",
                    disabled,
                    on_value: move |for_the_guild_key| {
                        save_character(Character {
                            for_the_guild_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().for_the_guild_key,
                }
                Buff {
                    label: "Hard Hitter",
                    disabled,
                    on_value: move |hard_hitter_key| {
                        save_character(Character {
                            hard_hitter_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().hard_hitter_key,
                }
                Buff {
                    label: "Extreme Red Potion",
                    disabled,
                    on_value: move |extreme_red_potion_key| {
                        save_character(Character {
                            extreme_red_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().extreme_red_potion_key,
                }
                Buff {
                    label: "Extreme Blue Potion",
                    disabled,
                    on_value: move |extreme_blue_potion_key| {
                        save_character(Character {
                            extreme_blue_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().extreme_blue_potion_key,
                }
                Buff {
                    label: "Extreme Green Potion",
                    disabled,
                    on_value: move |extreme_green_potion_key| {
                        save_character(Character {
                            extreme_green_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().extreme_green_potion_key,
                }

                Buff {
                    label: "Extreme Gold Potion",
                    disabled,
                    on_value: move |extreme_gold_potion_key| {
                        save_character(Character {
                            extreme_gold_potion_key,
                            ..character.peek().clone()
                        });
                    },
                    value: character().extreme_gold_potion_key,
                }
            }
        }
    }
}

#[component]
fn Buff(
    label: &'static str,
    value: KeyBindingConfiguration,
    on_value: Callback<KeyBindingConfiguration>,
    disabled: ReadSignal<bool>,
) -> Element {
    rsx! {
        div { class: "flex gap-2",
            CharactersKeyBindingConfigurationInput {
                label,
                value: Some(value),
                on_value: move |config: Option<KeyBindingConfiguration>| {
                    on_value(config.expect("not optional"));
                },
                disabled,
                label_class: "flex-1",
            }
            CharactersCheckbox {
                label: "Enabled",
                checked: value.enabled,
                on_checked: move |enabled| {
                    on_value(KeyBindingConfiguration {
                        enabled,
                        ..value
                    });
                },
                disabled,
            }
        }
    }
}
