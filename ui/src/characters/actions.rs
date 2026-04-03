use backend::ActionConfigurationCondition;
use dioxus::prelude::*;

use crate::{
    characters::{
        CharactersContext,
        list::{ActionConfigurationsList, ItemClickEvent, ItemMoveEvent, ItemToggleEvent},
    },
    components::section::Section,
};

#[component]
pub fn SectionFixedActions() -> Element {
    let context = use_context::<CharactersContext>();
    let character = context.character;
    let save_character = context.save_character;

    let add_action = move |action| {
        let mut character = character.peek().clone();

        character.actions.push(action);
        save_character(character);
    };

    let edit_action = move |event: ItemClickEvent| {
        let mut character = character.peek().clone();
        let current_action = character.actions.get_mut(event.index).unwrap();

        *current_action = event.action;
        save_character(character);
    };

    let delete_action = move |index| {
        let mut character = character.peek().clone();

        character.actions.remove(index);
        save_character(character);
    };

    let toggle_action = move |event: ItemToggleEvent| {
        let mut character = character.peek().clone();
        let action = character.actions.get_mut(event.index).unwrap();

        action.enabled = event.enabled;
        save_character(character);
    };

    let move_action = move |event: ItemMoveEvent| {
        let ItemMoveEvent {
            from_index,
            to_index,
        } = event;

        let mut character = character.peek().clone();
        let action = character.actions.remove(from_index);

        let insert_index = if from_index >= to_index {
            to_index
        } else {
            to_index - 1
        };
        let insert_index = insert_index.min(character.actions.len());

        let action_ref = character.actions.insert_mut(insert_index, action);
        if to_index == 0 {
            action_ref.condition = ActionConfigurationCondition::default();
        }

        save_character(character);
    };

    rsx! {
        Section { title: "Fixed actions",
            ActionConfigurationsList {
                disabled: character().id.is_none(),
                on_item_add: add_action,
                on_item_click: edit_action,
                on_item_delete: delete_action,
                on_item_toggle: toggle_action,
                on_item_move: move_action,
                actions: character().actions,
            }
        }
    }
}
