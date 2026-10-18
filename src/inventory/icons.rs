//! Pictures of items for the UI, and the stacks drawn with them in the hotbar and the inventory
//! screen.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::furnace::{container_stack, ContainerView};
use super::Inventory;
use crate::block::{BlockType, Face, RenderStyle, Tile};
use crate::chunks::generate::biome::{Biomes, Climate};
use crate::chunks::material::block_atlas;
use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::items::{Item, ItemStack};
use crate::menu::{font, TEXT_SHADOW};

// In pixels: a block's 16-pixel tile drawn twice the size.
const ICON_SIZE: f32 = 32.0;

/// The block atlas and the items' cut into their tiles, for drawing items in the UI.
#[derive(Resource)]
pub struct ItemIcons {
    atlas: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    items: Handle<Image>,
    items_layout: Handle<TextureAtlasLayout>,
    // pictures of the shapes from the front, drawn from their tiles, and which is whose
    shapes: Handle<Image>,
    shapes_layout: Handle<TextureAtlasLayout>,
    shape_icons: HashMap<BlockType, usize>,
    // what the greyscale grass and leaves tiles are tinted with
    grass: Color,
}

impl ItemIcons {
    /// A picture of `item`, for a slot or a button.
    pub fn icon(&self, item: Item) -> impl Bundle {
        let mut image = ImageNode::default();
        self.paint(&mut image, item);
        let size = Node {
            width: Val::Px(ICON_SIZE),
            height: Val::Px(ICON_SIZE),
            ..default()
        };
        (image, size)
    }

    /// The block atlas, which item icons are cut from.
    pub fn atlas(&self) -> &Handle<Image> {
        &self.atlas
    }

    /// The items' atlas, a tile for each item that isn't a block.
    pub fn items(&self) -> &Handle<Image> {
        &self.items
    }

    /// What greyscale tiles are tinted with.
    pub fn grass(&self) -> Color {
        self.grass
    }

    // A block is drawn with the tile its coarse LODs draw it with, the one that best stands
    // for the whole of it: grass shows its top, since its side's tile is just a fringe of grass
    // that's drawn over dirt. Shapes are pictures of them cut from their tiles, and other items
    // have tiles of their own.
    fn paint(&self, image: &mut ImageNode, item: Item) {
        let shape = item.block().and_then(|block| self.shape_icons.get(&block));
        let (atlas, layout, index, tinted) = match (item.block(), item.sprite()) {
            (Some(block), _) if shape.is_some() => (
                &self.shapes,
                &self.shapes_layout,
                *shape.unwrap(),
                block.lod_tile().tinted(),
            ),
            (Some(block), _) => {
                let tile = block.lod_tile();
                (&self.atlas, &self.layout, tile as usize, tile.tinted())
            }
            (None, sprite) => {
                let index = sprite.expect("an item that isn't a block has a sprite");
                (&self.items, &self.items_layout, index, false)
            }
        };
        image.image = atlas.clone();
        image.texture_atlas = Some(TextureAtlas {
            layout: layout.clone(),
            index,
        });
        image.color = if tinted { self.grass } else { Color::WHITE };
    }
}

pub(crate) fn load_icons(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let blocks = block_atlas("assets/textures/blocks.png");
    let (shapes, shape_icons) = shape_icons(&blocks);
    let mut strip = |atlas: Image| {
        let tile = atlas.height();
        let layout =
            TextureAtlasLayout::from_grid(UVec2::splat(tile), atlas.width() / tile, 1, None, None);
        (images.add(atlas), layouts.add(layout))
    };
    let (atlas, layout) = strip(blocks);
    let (items, items_layout) = strip(block_atlas("assets/textures/items.png"));
    let (shapes, shapes_layout) = strip(shapes);
    // as it is in a mild climate, which no biome's grass changes, whatever the seed
    let grass = Biomes::new(0).grass_tint(Climate::default());
    commands.insert_resource(ItemIcons {
        atlas,
        layout,
        items,
        items_layout,
        shapes,
        shapes_layout,
        shape_icons,
        grass: LinearRgba::rgb(grass.x, grass.y, grass.z).into(),
    });
}

// Texels a tile is across.
const TILE: u32 = 16;

// A strip of pictures of each shape from the front, from `atlas`'s tiles, and which is whose.
fn shape_icons(atlas: &Image) -> (Image, HashMap<BlockType, usize>) {
    let shapes: Vec<BlockType> = BlockType::ALL
        .into_iter()
        .filter(|block| block.is_shape())
        .collect();
    let mut strip = Image::new_fill(
        Extent3d {
            width: TILE * shapes.len() as u32,
            height: TILE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    strip.sampler = ImageSampler::nearest();
    for (i, &block) in shapes.iter().enumerate() {
        for y in 0..TILE {
            for x in 0..TILE {
                if let Some((tile, from_x, from_y)) = shape_texel(block, x, y) {
                    let texel = atlas
                        .get_color_at(tile as u32 * TILE + from_x, from_y)
                        .expect("a tile's texel");
                    strip
                        .set_color_at(i as u32 * TILE + x, y, texel)
                        .expect("an icon's texel");
                }
            }
        }
    }
    let indices = shapes
        .into_iter()
        .enumerate()
        .map(|(i, b)| (b, i))
        .collect();
    (strip, indices)
}

// Which texel of which tile a shape's icon shows at `x`, `y`, if any: stairs climbing to the
// right, a slab, a fence's two posts and rails, a wall, a door's two halves squeezed into one
// tile, a lantern's glass and cap, and the rest their own tile.
fn shape_texel(block: BlockType, x: u32, y: u32) -> Option<(Tile, u32, u32)> {
    let side = block.tile(Face::Side);
    let within =
        |x0: u32, y0: u32, x1: u32, y1: u32| (x0..x1).contains(&x) && (y0..y1).contains(&y);
    let own = Some((side, x, y));
    match block.style() {
        RenderStyle::Stairs => (y >= 8 || x >= 8).then_some((side, x, y)),
        RenderStyle::Slab => (y >= 8).then_some((side, x, y)),
        RenderStyle::Fence => {
            let posts = within(2, 0, 6, 16) || within(10, 0, 14, 16);
            let rails = within(0, 4, 16, 7) || within(0, 10, 16, 13);
            (posts || rails).then_some((side, x, y))
        }
        RenderStyle::Wall => {
            let posts = within(1, 0, 6, 16) || within(10, 0, 15, 16);
            (posts || within(0, 3, 16, 16)).then_some((side, x, y))
        }
        RenderStyle::Door => within(4, 0, 12, 16).then(|| {
            let half = block.tile(if y < 8 { Face::Top } else { Face::Side });
            (half, (x - 4) * 2, (y % 8) * 2)
        }),
        // the body's side twice the size, under its cap
        RenderStyle::Lantern if within(2, 2, 14, 16) => Some((side, (x - 2) / 2, 9 + (y - 2) / 2)),
        RenderStyle::Lantern if within(4, 0, 12, 2) => Some((side, 6 + (x - 4) / 2, 14 + y)),
        RenderStyle::Lantern => None,
        _ => own,
    }
}

/// Which of the local player's stacks an icon shows.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackView {
    Slot(usize),
    /// The one on the cursor in the inventory screen.
    Cursor,
    /// The one in a slot of the container the inventory screen's open at.
    Container(usize),
}

/// The count in the corner of a stack's icon.
#[derive(Component)]
pub struct StackCount;

/// The bar under a worn tool's icon.
#[derive(Component)]
pub struct WearBar;

/// The part of a wear bar filled, with what's left of the tool's durability.
#[derive(Component)]
pub struct WearLeft;

// In pixels, under the icon. The bar goes from green when new, through yellow, to red.
const WEAR_BAR_WIDTH: f32 = 26.0;
const WEAR_BAR_HEIGHT: f32 = 3.0;
const WEAR_BAR_COLOR: Color = Color::srgb(0.1, 0.1, 0.1);

/// An icon showing one of the local player's stacks, with how many are in it in the corner,
/// and how worn it is under it if it's a worn tool, as `draw_stacks` keeps it. Clicks go
/// through it to the slot it's in.
pub fn stack_view(icons: &ItemIcons, shows: StackView) -> impl Bundle {
    (
        shows,
        // painted once there's a stack to show
        icons.icon(Item::Block(default())),
        Visibility::Hidden,
        Pickable::IGNORE,
        children![
            (StackCount, count_text(String::new())),
            (
                WearBar,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px((ICON_SIZE - WEAR_BAR_WIDTH) / 2.0),
                    bottom: Val::Px(-2.0),
                    width: Val::Px(WEAR_BAR_WIDTH),
                    height: Val::Px(WEAR_BAR_HEIGHT),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(WEAR_BAR_COLOR),
                Pickable::IGNORE,
                children![(
                    WearLeft,
                    Node {
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    Pickable::IGNORE,
                )],
            ),
        ],
    )
}

/// A picture of `count` of `item`, with the count in its corner if it's more than one, as a
/// recipe shows what it takes and makes.
pub fn counted_icon(icons: &ItemIcons, item: Item, count: u8) -> impl Bundle {
    let count = if count > 1 {
        count.to_string()
    } else {
        String::new()
    };
    (
        icons.icon(item),
        Pickable::IGNORE,
        children![count_text(count)],
    )
}

fn count_text(count: String) -> impl Bundle {
    (
        Text::new(count),
        font(16.0),
        TEXT_SHADOW,
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(-2.0),
            bottom: Val::Px(-3.0),
            ..default()
        },
    )
}

/// Draws the local player's stacks, and the open container's, in every view of them, when they
/// change. Counts are shown in survival, for stacks of more than one, as creative never runs
/// out, and so is wear.
#[allow(clippy::type_complexity)]
pub(crate) fn draw_stacks(
    icons: Res<ItemIcons>,
    player: Query<(Ref<Inventory>, Ref<GameMode>), With<LocalPlayer>>,
    // not there without the inventory screen
    container: Option<Res<ContainerView>>,
    mut views: Query<(&StackView, &mut ImageNode, &mut Visibility, &Children)>,
    mut counts: Query<&mut Text, With<StackCount>>,
    mut bars: Query<(&mut Node, &Children), With<WearBar>>,
    mut left: Query<(&mut Node, &mut BackgroundColor), (With<WearLeft>, Without<WearBar>)>,
) {
    let Ok((inventory, mode)) = player.single() else {
        return;
    };
    let container_changed = container.as_ref().is_some_and(|view| view.is_changed());
    if !inventory.is_changed() && !mode.is_changed() && !container_changed {
        return;
    }
    let counted = !mode.has_every_block();
    for (shows, mut image, mut visibility, children) in &mut views {
        let stack = match *shows {
            StackView::Slot(slot) => inventory.get(slot),
            StackView::Cursor => inventory.cursor,
            StackView::Container(slot) => container
                .as_ref()
                .and_then(|view| container_stack(view, slot)),
        };
        *visibility = match stack {
            Some(stack) => {
                icons.paint(&mut image, stack.item);
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
        let mut count = counts
            .get_mut(children[0])
            .expect("a stack view has its count");
        count.0 = match stack {
            Some(ItemStack { count, .. }) if count > 1 && counted => count.to_string(),
            _ => String::new(),
        };
        let (mut bar, bar_children) = bars.get_mut(children[1]).expect("and its wear bar");
        let durability = stack.and_then(ItemStack::durability_left);
        bar.display = match durability {
            Some(_) if counted => Display::Flex,
            _ => Display::None,
        };
        if let (Some(durability), Ok((mut fill, mut colour))) =
            (durability, left.get_mut(bar_children[0]))
        {
            fill.width = Val::Percent(durability * 100.0);
            // red at none left, yellow at half, green when new
            colour.0 = Color::hsl(durability * 120.0, 0.9, 0.45);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::chunks::material::block_atlas;
    use crate::items::Item;

    #[test]
    fn test_the_items_atlas_has_a_tile_for_each_item_that_isnt_a_block() {
        let atlas = block_atlas("assets/textures/items.png");
        assert_eq!(16, atlas.height());
        assert_eq!(16 * Item::sprites() as u32, atlas.width());
    }
}
