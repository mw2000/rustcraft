use std::error::Error;

use bevy::ecs::resource::Resource;
use serde::Deserialize;

use crate::chunks::lod::LodLayout;

#[derive(Default, Deserialize, Clone, Copy, Resource)]
pub struct Settings {
    pub renderer: RendererSettings,
    #[serde(default)]
    pub debug: DebugSettings,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(default)]
pub struct RendererSettings {
    /// How far terrain is drawn, in chunks.
    pub render_distance: u32,
    /// How far terrain is drawn at full detail, in chunks.
    pub full_detail_distance: u32,
    /// Each LOD level reaches this many times as far as the one before it, starting
    /// from `lod_base_distance`: level n reaches it times this to the n.
    pub lod_level_scale: u32,
    /// The distance, in chunks, LOD levels scale out from, the full-detail distance if unset
    /// or nearer. Full detail reaching further than this only narrows the first LOD level's
    /// ring, so levels stay where they were as it's drawn further, until that ring would be
    /// narrower than `LEVEL_ONE_MIN_RING`, when they move out just enough to keep it.
    pub lod_base_distance: Option<u32>,
    /// How long swapping between LOD levels takes to cross-fade, in seconds.
    pub lod_fade_seconds: f32,
    /// How far the sun casts shadows, in chunks.
    pub shadow_distance: u32,
    /// Shadow map cascades, each drawing the terrain in its slice again.
    pub shadow_cascades: usize,
}

/// Switches for testing what the renderer spends its time on, all off by default.
#[derive(Default, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct DebugSettings {
    /// Leaves stay still instead of swaying in the wind.
    pub still_leaves: bool,
    /// Leaves are drawn in their mean colour without their holes, in shadows too, and hide
    /// the faces of the leaves behind them, as coarse LODs draw them everywhere.
    pub solid_leaves: bool,
}

/// Where a file in `assets/` lives, found the way Bevy's `AssetPlugin` finds its assets: under
/// `BEVY_ASSET_ROOT`, else `CARGO_MANIFEST_DIR` (set by `cargo run`), else next to the executable.
/// A release launched by double-clicking has some other working directory, so a path relative to
/// that would miss. Pass the path inside the root, such as `assets/settings.toml`.
pub fn asset_path(file: &str) -> std::path::PathBuf {
    let root = std::env::var_os("BEVY_ASSET_ROOT")
        .or_else(|| std::env::var_os("CARGO_MANIFEST_DIR"))
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok()?.parent().map(|p| p.to_owned()))
        .unwrap_or_default();
    root.join(file)
}

impl Settings {
    pub fn read(file: &str) -> Result<Self, Box<dyn Error>> {
        Ok(toml::from_str(&std::fs::read_to_string(asset_path(file))?)?)
    }
}

impl RendererSettings {
    pub fn lod_layout(&self) -> LodLayout {
        let full_detail = self.full_detail_distance.max(1);
        let base = self
            .lod_base_distance
            .map_or(full_detail, |base| base.min(full_detail))
            .max(LodLayout::least_base_distance(
                full_detail,
                self.lod_level_scale,
            ));
        LodLayout::new(base, self.render_distance, self.lod_level_scale)
            .with_full_detail_distance(full_detail)
    }
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            render_distance: 8,
            full_detail_distance: 32,
            lod_level_scale: 4,
            lod_base_distance: None,
            lod_fade_seconds: 1.5,
            shadow_distance: 64,
            shadow_cascades: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RendererSettings, Settings};
    use crate::chunks::lod::{LodLayout, LEVEL_ONE_MIN_RING};

    #[test]
    fn test_missing_renderer_settings_use_defaults() {
        let settings: Settings = toml::from_str("[renderer]\nrender_distance = 64").unwrap();
        assert_eq!(64, settings.renderer.render_distance);
        assert_eq!(32, settings.renderer.full_detail_distance);
        assert!(!settings.debug.still_leaves && !settings.debug.solid_leaves);
        // with no base, levels scale from full detail, as they always did
        let layout = settings.renderer.lod_layout();
        assert_eq!([32.0, 128.0], [layout.reach(0), layout.reach(1)]);
    }

    // The shipped renderer settings with full detail out to `full_detail` chunks.
    fn shipped(full_detail: u32) -> RendererSettings {
        let file = std::fs::read_to_string("assets/settings.toml").unwrap();
        let settings: Settings = toml::from_str(&file).unwrap();
        RendererSettings {
            full_detail_distance: full_detail,
            ..settings.renderer
        }
    }

    #[test]
    fn test_full_detail_nearer_than_the_base_scales_levels_from_itself() {
        // as the Low and Medium presets draw: just as before there was a base
        for full_detail in [4, 8, 12, 16] {
            let layout = shipped(full_detail).lod_layout();
            let plain = LodLayout::new(full_detail, 1024, 2);
            for level in 0..=plain.max_level {
                assert_eq!(plain.reach(level), layout.reach(level));
            }
            assert_eq!(plain.max_level, layout.max_level);
        }
    }

    #[test]
    fn test_full_detail_past_the_base_keeps_the_levels_until_level_one_is_too_narrow() {
        let base = shipped(16).lod_layout();
        for full_detail in [17, 24, 28] {
            let layout = shipped(full_detail).lod_layout();
            assert_eq!(full_detail as f32, layout.reach(0));
            for level in 1..=base.max_level {
                assert_eq!(base.reach(level), layout.reach(level));
            }
        }
        // the slider reaches 32, where the levels move out just enough to leave level 1 its ring
        for full_detail in [29, 32] {
            let layout = shipped(full_detail).lod_layout();
            assert_eq!(full_detail as f32, layout.reach(0));
            assert!(layout.reach(1) - layout.reach(0) >= LEVEL_ONE_MIN_RING);
            assert!(layout.reach(1) - layout.reach(0) < LEVEL_ONE_MIN_RING + 2.0);
        }
    }

    #[test]
    fn test_debug_settings_parse() {
        let settings: Settings =
            toml::from_str("[renderer]\n[debug]\nsolid_leaves = true").unwrap();
        assert!(settings.debug.solid_leaves);
        assert!(!settings.debug.still_leaves);
    }

    #[test]
    fn test_settings_file_parses() {
        let file = std::fs::read_to_string("assets/settings.toml").unwrap();
        let settings: Settings = toml::from_str(&file).unwrap();
        assert_eq!(24, settings.renderer.full_detail_distance);
        assert_eq!(Some(16), settings.renderer.lod_base_distance);
        assert_eq!(2, settings.renderer.lod_level_scale);
        // full detail reaches further than the levels scale from, with level 1 still between
        // it and level 2
        let layout = settings.renderer.lod_layout();
        assert_eq!(24.0, layout.reach(0));
        assert_eq!([32.0, 64.0], [layout.reach(1), layout.reach(2)]);
        // the debug switches ship off
        assert!(!settings.debug.still_leaves && !settings.debug.solid_leaves);
    }
}
