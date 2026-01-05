use std::fmt::Debug;

use crate::{
    minimap::{Minimap, MinimapContext, MinimapEntity},
    models::Map,
    pathing::Platform,
    player::PlayerContext,
};

/// A service to handle map-related incoming requests.
pub trait MapService: Debug {
    /// Creates a new [`Map`] from currently detected minimap with `name`.
    fn create(&self, minimap_state: Minimap, name: String) -> Option<Map>;

    /// Gets the currently in use [`Map`].
    fn map(&self) -> Option<&Map>;

    /// Gets the currently in use preset.
    fn preset(&self) -> Option<String>;

    /// Sets new `map` and `preset` to be used.
    fn update_map_preset(&mut self, map: Option<Map>, preset: Option<String>);

    /// Updates `minimap_context` and `player_context` with information from the currently in use
    /// [`Map`] and preset.
    fn apply(&self, minimap_context: &mut MinimapContext, player_context: &mut PlayerContext);

    /// Re-detects current minimap.
    fn redetect(&self, minimap: &mut MinimapEntity);
}

#[derive(Debug, Default)]
pub struct DefaultMapService {
    map: Option<Map>,
    preset: Option<String>,
}

impl MapService for DefaultMapService {
    fn create(&self, minimap_state: Minimap, name: String) -> Option<Map> {
        if let Minimap::Idle(idle) = minimap_state {
            Some(Map {
                name,
                width: idle.bbox.width,
                height: idle.bbox.height,
                ..Map::default()
            })
        } else {
            None
        }
    }

    fn map(&self) -> Option<&Map> {
        self.map.as_ref()
    }

    fn preset(&self) -> Option<String> {
        self.preset.clone()
    }

    fn update_map_preset(&mut self, map: Option<Map>, preset: Option<String>) {
        self.map = map;
        self.preset = preset;
    }

    fn apply(&self, minimap_context: &mut MinimapContext, player_context: &mut PlayerContext) {
        let platforms = self
            .map()
            .map(|data| {
                data.platforms
                    .iter()
                    .copied()
                    .map(Platform::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        minimap_context.set_platforms(platforms);

        player_context.reset();
        if let Some(map) = self.map() {
            player_context.config.rune_platforms_pathing = map.rune_platforms_pathing;
            player_context.config.rune_platforms_pathing_up_jump_only =
                map.rune_platforms_pathing_up_jump_only;
            player_context.config.auto_mob_platforms_pathing = map.auto_mob_platforms_pathing;
            player_context
                .config
                .auto_mob_platforms_pathing_up_jump_only =
                map.auto_mob_platforms_pathing_up_jump_only;
            player_context.config.auto_mob_platforms_bound = map.auto_mob_platforms_bound;
            player_context.config.auto_mob_use_key_when_pathing = map.auto_mob_use_key_when_pathing;
            player_context
                .config
                .auto_mob_use_key_when_pathing_update_millis =
                map.auto_mob_use_key_when_pathing_update_millis;
        }
    }

    fn redetect(&self, minimap: &mut MinimapEntity) {
        minimap.state = Minimap::Detecting;
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches::assert_matches;

    use opencv::core::Rect;

    use super::*;
    use crate::{
        Platform as DatabasePlatform,
        minimap::{Minimap, MinimapIdle},
        pathing::Platform,
    };

    fn mock_idle_minimap() -> Minimap {
        let mut idle = MinimapIdle::default();
        idle.bbox = Rect::new(0, 0, 100, 100);
        Minimap::Idle(idle)
    }

    fn mock_minimap_data() -> Map {
        Map {
            name: "MapData".to_string(),
            width: 100,
            height: 100,
            rune_platforms_pathing: true,
            rune_platforms_pathing_up_jump_only: true,
            auto_mob_platforms_pathing: true,
            auto_mob_platforms_bound: true,
            ..Default::default()
        }
    }

    #[test]
    fn create_returns_some_when_idle_minimap() {
        let service = DefaultMapService::default();

        let result = service.create(mock_idle_minimap(), "MapData".to_string());

        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            Map {
                name: "MapData".to_string(),
                width: 100,
                height: 100,
                ..Default::default()
            }
        );
    }

    #[test]
    fn create_returns_none_when_not_idle_minimap() {
        let service = DefaultMapService::default();

        let result = service.create(Minimap::Detecting, "ShouldNotExist".to_string());

        assert!(result.is_none());
    }

    #[test]
    fn set_minimap_and_preset() {
        let mut service = DefaultMapService::default();
        let minimap = mock_minimap_data();
        let preset = Some("custom".to_string());

        service.update_map_preset(Some(minimap.clone()), preset.clone());

        assert_eq!(service.map, Some(minimap));
        assert_eq!(service.preset, preset);
    }

    #[test]
    fn redetect_sets_minimap_to_detecting() {
        let service = DefaultMapService::default();
        let mut minimap = MinimapEntity {
            state: mock_idle_minimap(),
            context: MinimapContext::default(),
        };

        service.redetect(&mut minimap);

        assert_matches!(minimap.state, Minimap::Detecting);
    }

    #[test]
    fn update_reset_minimap_state_platforms() {
        let service = DefaultMapService::default();
        let mut player_context = PlayerContext::default();
        let mut minimap = MinimapEntity {
            state: mock_idle_minimap(),
            context: MinimapContext::default(),
        };
        minimap
            .context
            .set_platforms(vec![Platform::from(DatabasePlatform {
                x_start: 3,
                x_end: 3,
                y: 10,
            })]);

        service.apply(&mut minimap.context, &mut player_context);

        assert!(service.map.is_none());
        assert!(service.preset.is_none());
        assert!(minimap.context.platforms().is_empty());
    }

    #[test]
    fn update_keep_player_config() {
        let service = DefaultMapService::default();
        let mut minimap_context = MinimapContext::default();
        let mut player_context = PlayerContext::default();
        player_context.config.rune_platforms_pathing = true;
        player_context.config.rune_platforms_pathing_up_jump_only = true;

        service.apply(&mut minimap_context, &mut player_context);
        assert!(player_context.config.rune_platforms_pathing); // Doesn't change
        assert!(player_context.config.rune_platforms_pathing_up_jump_only); // Doesn't change
    }

    #[test]
    fn update_change_player_config() {
        let service = DefaultMapService {
            map: Some(mock_minimap_data()),
            preset: Some("preset".to_string()),
        };
        let mut minimap_context = MinimapContext::default();
        let mut player_state = PlayerContext::default();

        service.apply(&mut minimap_context, &mut player_state);

        assert!(player_state.config.rune_platforms_pathing);
        assert!(player_state.config.rune_platforms_pathing_up_jump_only);
        assert!(player_state.config.auto_mob_platforms_pathing);
        assert!(player_state.config.auto_mob_platforms_bound);
    }
}
