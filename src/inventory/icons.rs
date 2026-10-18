//! Pictures of items for the UI, and the stacks drawn with them in the hotbar and the inventory
//! screen.

use bevy::prelude::*;

use super::Inventory;
use crate::chunks::generate::biome::{grass_tint, Climate};
use crate::chunks::material::block_atlas;
use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::items::{Item, ItemStack};
use crate::menu::{font, TEXT_SHADOW};

// In pixels: a block's 16-pixel tile drawn twice the size.
const ICON_SIZE: f32 = 32.0;

/// The block atlas cut into its tiles, for drawing items in the UI.
#[derive(Resource)]
pub struct ItemIcons {
    atlas: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    // what the greyscale grass and leaves tiles are tinted with
    grass: Color,
}

impl ItemIcons {
    /// A picture of `item`, for a slot or a button.
    pub fn icon(&self, item: Item) -> impl Bundle {
        let atlas = TextureAtlas {
            layout: self.layout.clone(),
            index: 0,
        };
        let mut image = ImageNode::from_atlas_image(self.atlas.clone(), atlas);
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

    /// What greyscale tiles are tinted with.
    pub fn grass(&self) -> Color {
        self.grass
    }

    // A block is drawn with the tile its coarse LODs draw it with, the one that best stands
    // for the whole of it: grass shows its top, since its side's tile is just a fringe of grass
    // that's drawn over dirt.
    fn paint(&self, image: &mut ImageNode, item: Item) {
        let tile = match item {
            Item::Block(block) => block.lod_tile(),
        };
        if let Some(atlas) = &mut image.texture_atlas {
            atlas.index = tile as usize;
        }
        image.color = if tile.tinted() {
            self.grass
        } else {
            Color::WHITE
        };
    }
}

pub(crate) fn load_icons(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let atlas = block_atlas("assets/textures/blocks.png");
    let tile = atlas.height();
    let layout =
        TextureAtlasLayout::from_grid(UVec2::splat(tile), atlas.width() / tile, 1, None, None);
    // as it is in a mild climate
    let grass = grass_tint(Climate::default());
    commands.insert_resource(ItemIcons {
        atlas: images.add(atlas),
        layout: layouts.add(layout),
        grass: LinearRgba::rgb(grass.x, grass.y, grass.z).into(),
    });
}

/// Which of the local player's stacks an icon shows.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackView {
    Slot(usize),
    /// The one on the cursor in the inventory screen.
    Cursor,
}

/// The count in the corner of a stack's icon.
#[derive(Component)]
pub struct StackCount;

/// An icon showing one of the local player's stacks, with how many are in it in the corner,
/// as `draw_stacks` keeps it. Clicks go through it to the slot it's in.
pub fn stack_view(icons: &ItemIcons, shows: StackView) -> impl Bundle {
    (
        shows,
        // painted once there's a stack to show
        icons.icon(Item::Block(default())),
        Visibility::Hidden,
        Pickable::IGNORE,
        children![(
            StackCount,
            Text::default(),
            font(16.0),
            TEXT_SHADOW,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(-2.0),
                bottom: Val::Px(-3.0),
                ..default()
            },
        )],
    )
}

/// Draws the local player's stacks in every view of them, when they change. Counts are shown
/// in survival, for stacks of more than one, as creative never runs out.
#[allow(clippy::type_complexity)]
pub(crate) fn draw_stacks(
    icons: Res<ItemIcons>,
    player: Query<
        (&Inventory, &GameMode),
        (
            With<LocalPlayer>,
            Or<(Changed<Inventory>, Changed<GameMode>)>,
        ),
    >,
    mut views: Query<(&StackView, &mut ImageNode, &mut Visibility, &Children)>,
    mut counts: Query<&mut Text, With<StackCount>>,
) {
    let Ok((inventory, mode)) = player.single() else {
        return;
    };
    for (shows, mut image, mut visibility, children) in &mut views {
        let stack = match *shows {
            StackView::Slot(slot) => inventory.slots[slot],
            StackView::Cursor => inventory.cursor,
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
            Some(ItemStack { count, .. }) if count > 1 && !mode.has_every_block() => {
                count.to_string()
            }
            _ => String::new(),
        };
    }
}
