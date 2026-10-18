//! The map's pictures of the land: a small image for each explored tile, rendered on a task from
//! the terrain's sampling (INV-1, INV-7), and coarser ones of four by four tiles for seeing
//! further, kept while they're wanted, up to a few hundred. Edits don't show: the map is the
//! land as generated.

use bevy::asset::RenderAssetUsages;
use bevy::math::DVec2;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{futures, AsyncComputeTaskPool, Task};

use super::explored::{Explored, TILE_BLOCKS};
use super::relief::Relief;
use super::MapView;
use crate::chunks::generate::noise::NoiseGenerator;

/// How many pixels across an explored tile's image is: one to every two blocks.
pub const TILE_PIXELS: usize = 32;

// Images kept, the least recently wanted let go past it: 4KB each, or 16KB for the coarser
// ones, and an image node each while the map's open. More than fill the biggest screens.
const KEPT: usize = 768;
// Tiles rendered at once, a few milliseconds each: plenty while the map's open, and a trickle
// while it's closed, keeping those round the player ready for opening it.
const RENDERING_OPEN: usize = 8;
const RENDERING_CLOSED: usize = 2;

/// One of the map's images: at level 0 an explored tile, `TILE_PIXELS` across, and at level 1
/// four by four of them, twice as many pixels across, for when the map's zoomed out.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: u8,
    pub at: IVec2,
}

impl TileKey {
    /// The image at `level` covering block column `at` (x, z).
    pub fn containing(level: u8, at: Vec2) -> Self {
        let key = Self {
            level,
            at: IVec2::ZERO,
        };
        Self {
            level,
            at: (at / key.blocks() as f32).floor().as_ivec2(),
        }
    }

    // How many explored tiles across it covers.
    fn span(self) -> i32 {
        1 << (2 * self.level)
    }

    /// How many blocks across it covers.
    pub fn blocks(self) -> i32 {
        TILE_BLOCKS * self.span()
    }

    pub fn pixels(self) -> usize {
        TILE_PIXELS << self.level
    }

    /// Its north west corner, in blocks.
    pub fn corner(self) -> Vec2 {
        (self.at * self.blocks()).as_vec2()
    }

    /// Which of the tiles it covers have been explored, a bit each, row by row from the north
    /// west.
    pub fn explored(self, explored: &Explored) -> u16 {
        let (span, first) = (self.span(), self.at * self.span());
        let mut bits = 0;
        for z in 0..span {
            for x in 0..span {
                if explored.contains(first + IVec2::new(x, z)) {
                    bits |= 1 << (z * span + x);
                }
            }
        }
        bits
    }
}

/// Renders the image `key` as sRGB pixels with alpha, row by row from the north west: the land
/// in the tiles `explored` says, as `Explored` has them, and nothing in the rest.
pub fn render(noise: &NoiseGenerator, key: TileKey, explored: u16) -> Vec<u8> {
    let pixels = key.pixels();
    let scale = key.blocks() as f64 / pixels as f64;
    // At the pixels' middles, with one more past each edge for the slopes there.
    let corner = key.corner().as_dvec2() - DVec2::splat(scale / 2.0);
    let relief = Relief::sample(noise, corner, scale, pixels + 2, 1);
    let per_tile = pixels / key.span() as usize;
    let mut rgba = Vec::with_capacity(pixels * pixels * 4);
    for z in 0..pixels {
        for x in 0..pixels {
            let tile = (z / per_tile) * key.span() as usize + x / per_tile;
            if explored & (1 << tile) == 0 {
                rgba.extend([0; 4]);
            } else {
                let [r, g, b] = relief.colour(x + 1, z + 1, Some(noise.biomes()));
                rgba.extend([r, g, b, 255]);
            }
        }
    }
    rgba
}

/// The images that `view` shows of a screen `screen` pixels across, with which of their tiles
/// are explored, nearest its middle first, leaving out those with nothing explored.
pub fn wanted(view: &MapView, screen: Vec2, explored: &Explored) -> Vec<(TileKey, u16)> {
    let level = view.level();
    let half = screen / 2.0 * view.blocks_per_pixel();
    let first = TileKey::containing(level, view.centre - half).at;
    let last = TileKey::containing(level, view.centre + half).at;
    let mut wanted = Vec::new();
    for z in first.y..=last.y {
        for x in first.x..=last.x {
            let key = TileKey {
                level,
                at: IVec2::new(x, z),
            };
            let bits = key.explored(explored);
            if bits != 0 {
                wanted.push((key, bits));
            }
        }
    }
    let middle = |key: TileKey| key.corner() + key.blocks() as f32 / 2.0;
    wanted.sort_by(|a, b| {
        let (a, b) = (middle(a.0), middle(b.0));
        a.distance_squared(view.centre)
            .total_cmp(&b.distance_squared(view.centre))
    });
    wanted.truncate(KEPT);
    wanted
}

/// The images rendered and rendering, each rendered image drawn by a node of the map's.
#[derive(Resource, Default)]
pub struct MapTiles {
    kept: HashMap<TileKey, Kept>,
    rendering: Vec<Rendering>,
    // how many times what's wanted has been worked out, to let go of the least recently wanted
    pass: u64,
    // whether more was wanted than could be rendered at once
    backlog: bool,
}

struct Kept {
    node: Entity,
    // the tiles explored as it was rendered
    explored: u16,
    wanted: u64,
}

struct Rendering {
    key: TileKey,
    explored: u16,
    task: Task<Vec<u8>>,
}

impl MapTiles {
    pub fn kept(&self) -> usize {
        self.kept.len()
    }

    pub fn rendering(&self) -> usize {
        self.rendering.len()
    }
}

/// The node the map's images go in.
#[derive(Component)]
pub(super) struct TileLayer;

/// Starts rendering what the map wants and doesn't have, or has with fewer of its tiles
/// explored than there are now, nearest first.
pub(super) fn request_tiles(
    view: Res<MapView>,
    explored: Res<Explored>,
    world: Res<crate::world::World>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    open: Res<State<crate::menu::GameState>>,
    mut tiles: ResMut<MapTiles>,
) {
    if !view.is_changed() && !explored.is_changed() && !tiles.backlog {
        return;
    }
    let at_once = if *open.get() == crate::menu::GameState::Map {
        RENDERING_OPEN
    } else {
        RENDERING_CLOSED
    };
    let tiles = &mut *tiles;
    tiles.pass += 1;
    tiles.backlog = false;
    for (key, bits) in wanted(&view, window.size(), &explored) {
        if let Some(kept) = tiles.kept.get_mut(&key) {
            kept.wanted = tiles.pass;
            if kept.explored == bits {
                continue;
            }
        }
        if tiles.rendering.iter().any(|r| r.key == key) {
            continue;
        }
        if tiles.rendering.len() == at_once {
            tiles.backlog = true;
            continue;
        }
        let regions = world.region_store.clone();
        tiles.rendering.push(Rendering {
            key,
            explored: bits,
            task: AsyncComputeTaskPool::get()
                .spawn(async move { render(regions.noise(), key, bits) }),
        });
    }
}

/// Puts each image rendered on a node of its own, or on its old one if it's been rendered again,
/// and lets go of the least recently wanted past `KEPT`.
pub(super) fn finish_tiles(
    mut tiles: ResMut<MapTiles>,
    mut images: ResMut<Assets<Image>>,
    layer: Single<Entity, With<TileLayer>>,
    mut nodes: Query<&mut ImageNode>,
    mut commands: Commands,
) {
    let tiles = &mut *tiles;
    let mut finished = Vec::new();
    tiles.rendering.retain_mut(
        |rendering| match futures::check_ready(&mut rendering.task) {
            Some(rgba) => {
                finished.push((rendering.key, rendering.explored, rgba));
                false
            }
            None => true,
        },
    );
    for (key, explored, rgba) in finished {
        let size = key.pixels() as u32;
        let image = images.add(Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ));
        let pass = tiles.pass;
        match tiles.kept.get_mut(&key) {
            Some(kept) => {
                kept.explored = explored;
                if let Ok(mut node) = nodes.get_mut(kept.node) {
                    node.image = image;
                }
            }
            None => {
                let node = commands
                    .spawn((
                        key,
                        ImageNode::new(image),
                        Node {
                            position_type: PositionType::Absolute,
                            display: Display::None,
                            ..default()
                        },
                        ChildOf(*layer),
                    ))
                    .id();
                tiles.kept.insert(
                    key,
                    Kept {
                        node,
                        explored,
                        wanted: pass,
                    },
                );
            }
        }
    }

    if tiles.kept.len() > KEPT {
        let mut by_age: Vec<_> = tiles
            .kept
            .iter()
            .map(|(k, kept)| (kept.wanted, *k))
            .collect();
        by_age.sort_unstable_by_key(|(wanted, _)| *wanted);
        for (_, key) in &by_age[..tiles.kept.len() - KEPT] {
            if let Some(kept) = tiles.kept.remove(key) {
                // with its image, which only it holds
                commands.entity(kept.node).despawn();
            }
        }
    }
}

/// Puts the nodes of the images at the view's level where the view has them, and hides the rest.
pub(super) fn place_tiles(
    view: Res<MapView>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut nodes: Query<(&TileKey, &mut Node)>,
) {
    let (screen, level, scale) = (window.size(), view.level(), view.blocks_per_pixel());
    for (key, mut node) in &mut nodes {
        let at = view.to_screen(key.corner(), screen);
        let size = key.blocks() as f32 / scale;
        let shown = key.level == level
            && at.x < screen.x
            && at.y < screen.y
            && at.x + size > 0.0
            && at.y + size > 0.0;
        let placed = Node {
            position_type: PositionType::Absolute,
            display: if shown { Display::Flex } else { Display::None },
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(size),
            height: Val::Px(size),
            ..default()
        };
        // a hidden one only needs hiding, and only changed nodes are laid out again
        if (shown || node.display != Display::None) && *node != placed {
            *node = placed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_images_cover_their_tiles() {
        let key = TileKey::containing(1, Vec2::new(-1.0, 300.0));
        assert_eq!(IVec2::new(-1, 1), key.at);
        assert_eq!(Vec2::new(-256.0, 256.0), key.corner());
        assert_eq!(64, key.pixels());
        let mut explored = Explored::default();
        explored.reveal_around(Vec2::new(-400.0, 300.0));
        let bits = key.explored(&explored);
        // its north west tile is (-4, 4), and the second row's first (-4, 5)
        assert_eq!(explored.contains(IVec2::new(-4, 5)), bits & (1 << 4) != 0);
        assert_ne!(0, bits);
        assert_ne!(u16::MAX, bits);
    }

    #[test]
    fn test_tiles_render_the_same_for_a_seed_and_differ_between_seeds() {
        let key = TileKey {
            level: 0,
            at: IVec2::new(3, -2),
        };
        let a = render(&NoiseGenerator::new(7), key, 1);
        assert_eq!(TILE_PIXELS * TILE_PIXELS * 4, a.len());
        assert_eq!(a, render(&NoiseGenerator::new(7), key, 1));
        assert_ne!(a, render(&NoiseGenerator::new(8), key, 1));
    }

    #[test]
    fn test_tiles_match_the_world_map() {
        let noise = NoiseGenerator::new(7);
        for key in [
            TileKey {
                level: 0,
                at: IVec2::new(5, -3),
            },
            TileKey {
                level: 1,
                at: IVec2::new(-2, 1),
            },
        ] {
            let tile = render(&noise, key, u16::MAX);
            // `world_map --center <the tile's middle> --scale <its> --pixels <twice its>`
            let pixels = key.pixels();
            let scale = key.blocks() as f64 / pixels as f64;
            let centre = key.corner().as_dvec2() + key.blocks() as f64 / 2.0 + scale / 2.0;
            let corner = centre - (pixels as f64) * scale;
            let world_map = Relief::sample(&noise, corner, scale, pixels * 2, 4);
            let offset = pixels / 2;
            for z in 0..pixels {
                for x in 0..pixels {
                    let i = (z * pixels + x) * 4;
                    assert_eq!(
                        world_map.colour(x + offset, z + offset, Some(noise.biomes())),
                        tile[i..i + 3],
                        "{key:?} at {x}, {z}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_coarse_images_leave_out_whats_unexplored() {
        let noise = NoiseGenerator::new(7);
        let key = TileKey {
            level: 1,
            at: IVec2::ZERO,
        };
        // just the north west tile and the one east of it
        let rgba = render(&noise, key, 0b11);
        let alpha = |x: usize, z: usize| rgba[(z * key.pixels() + x) * 4 + 3];
        assert_eq!(255, alpha(0, 0));
        assert_eq!(255, alpha(31, 15));
        assert_eq!(0, alpha(32, 0));
        assert_eq!(0, alpha(0, 16));
    }

    #[test]
    fn test_the_nearest_explored_images_are_wanted_first() {
        let mut explored = Explored::default();
        explored.reveal_around(Vec2::ZERO);
        let round_the_origin = explored.len();
        explored.reveal_around(Vec2::new(5000.0, 0.0));
        let view = MapView {
            centre: Vec2::new(100.0, 10.0),
            zoom: 1,
        };
        let wanted = wanted(&view, Vec2::new(1280.0, 720.0), &explored);
        // not those 5km off the screen
        assert_eq!(round_the_origin, wanted.len());
        assert!(wanted
            .iter()
            .all(|(key, bits)| key.level == 0 && *bits == 1));
        assert_eq!(IVec2::new(1, 0), wanted[0].0.at);
    }
}
