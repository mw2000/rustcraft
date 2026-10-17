use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::generate::biome::{grass_tint, Climate};
use crate::chunks::material::block_atlas;
use crate::commands::command_bar_closed;
use crate::menu::GameState;
use crate::player::Player;

/// How many slots the hotbar has.
pub const SLOTS: usize = 9;

const SLOT_KEYS: [KeyCode; SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

// Touchpads scroll in pixels, which are gathered up into notches of a mouse wheel.
const PIXELS_PER_NOTCH: f32 = 40.0;

// In pixels: a strip of slots, each with a block's 16-pixel tile drawn twice the size in it.
const ICON_SIZE: f32 = 32.0;
const SLOT_SIZE: f32 = 40.0;
const SLOT_BORDER: f32 = 2.0;
const SLOT_GAP: f32 = 2.0;
const MARGIN: f32 = 6.0;
/// How far up from the bottom of the screen the hotbar reaches, in pixels, for UI to go above.
pub const HOTBAR_TOP: f32 = MARGIN + SLOT_SIZE + 2.0 * SLOT_GAP;

// Like the command bar's, with the selected slot framed in white.
const STRIP_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const SLOT_BORDER_COLOR: Color = Color::srgba(0.55, 0.55, 0.55, 0.6);
const SELECTED_BORDER_COLOR: Color = Color::WHITE;
const SELECTED_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);

/// The blocks a player has to hand, along the bottom of the screen, one of them selected for
/// placing (§5.2). Keys 1 to 9 and the mouse wheel select a slot.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "SavedHotbar", try_from = "SavedHotbar")]
pub struct Hotbar {
    /// What each slot holds, if anything. M1.1 makes these stacks of items, with counts.
    pub slots: [Option<BlockType>; SLOTS],
    pub selected: usize,
}

// Blocks to build with, for creative.
impl Default for Hotbar {
    fn default() -> Self {
        use BlockType::*;
        Self {
            slots: [
                Stone,
                Cobblestone,
                StoneBricks,
                Bricks,
                Planks,
                Log,
                Dirt,
                Grass,
                Sandstone,
            ]
            .map(Some),
            selected: 0,
        }
    }
}

impl Hotbar {
    /// What's in the selected slot.
    pub fn held(&self) -> Option<BlockType> {
        self.slots[self.selected]
    }

    /// Moves the selection `by` slots to the right, or left if it's negative, wrapping round.
    pub fn scroll(&mut self, by: i32) {
        self.selected = (self.selected as i32 + by).rem_euclid(SLOTS as i32) as usize;
    }

    /// Selects the slot holding `block`, as middle click does, or if none does and `fill`,
    /// puts it in the selected slot.
    pub fn pick(&mut self, block: BlockType, fill: bool) {
        if let Some(slot) = self.slots.iter().position(|&held| held == Some(block)) {
            self.selected = slot;
        } else if fill {
            self.slots[self.selected] = Some(block);
        }
    }
}

// How a hotbar is saved: its blocks by name (§4.1), with "" for an empty slot.
#[derive(Serialize, Deserialize)]
struct SavedHotbar {
    slots: [String; SLOTS],
    selected: usize,
}

impl From<Hotbar> for SavedHotbar {
    fn from(hotbar: Hotbar) -> Self {
        Self {
            slots: hotbar
                .slots
                .map(|slot| slot.map_or("", |block| block.name()).into()),
            selected: hotbar.selected,
        }
    }
}

impl TryFrom<SavedHotbar> for Hotbar {
    type Error = String;

    fn try_from(saved: SavedHotbar) -> Result<Self, String> {
        if saved.selected >= SLOTS {
            return Err(format!("the hotbar has no slot {}", saved.selected));
        }
        let mut slots = [None; SLOTS];
        for (slot, name) in slots.iter_mut().zip(&saved.slots) {
            if !name.is_empty() {
                let block = BlockType::from_name(name);
                *slot = Some(block.ok_or_else(|| format!("there's no block called {name}"))?);
            }
        }
        Ok(Self {
            slots,
            selected: saved.selected,
        })
    }
}

/// The player's hotbar on screen, and choosing a slot in it.
pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_icons, spawn_hotbar).chain())
            .add_systems(
                Update,
                (
                    // and while the block picker's open, to choose the slot a block goes in
                    select_slot
                        .run_if(not(in_state(GameState::Paused)))
                        .run_if(command_bar_closed),
                    draw_hotbar,
                )
                    .chain(),
            );
    }
}

/// The block atlas cut into its tiles, for drawing blocks in the UI.
#[derive(Resource)]
pub struct BlockIcons {
    atlas: Handle<Image>,
    layout: Handle<TextureAtlasLayout>,
    // what the greyscale grass and leaves tiles are tinted with
    grass: Color,
}

impl BlockIcons {
    /// A picture of `block`, for a slot or a button.
    pub fn icon(&self, block: BlockType) -> impl Bundle {
        let atlas = TextureAtlas {
            layout: self.layout.clone(),
            index: 0,
        };
        let mut image = ImageNode::from_atlas_image(self.atlas.clone(), atlas);
        self.paint(&mut image, block);
        let size = Node {
            width: Val::Px(ICON_SIZE),
            height: Val::Px(ICON_SIZE),
            ..default()
        };
        (image, size)
    }

    // A block is drawn with the tile its coarse LODs draw it with, the one that best stands
    // for the whole of it: grass shows its top, since its side's tile is just a fringe of grass
    // that's drawn over dirt.
    fn paint(&self, image: &mut ImageNode, block: BlockType) {
        let tile = block.lod_tile();
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
    commands.insert_resource(BlockIcons {
        atlas: images.add(atlas),
        layout: layouts.add(layout),
        grass: LinearRgba::rgb(grass.x, grass.y, grass.z).into(),
    });
}

#[derive(Component)]
struct Slot(usize);

#[derive(Component)]
struct SlotIcon(usize);

fn spawn_hotbar(mut commands: Commands, icons: Res<BlockIcons>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(MARGIN),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // over the block picker, so its slots can be seen filling
            GlobalZIndex(2),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::all(Val::Px(SLOT_GAP)),
                    column_gap: Val::Px(SLOT_GAP),
                    ..default()
                },
                BackgroundColor(STRIP_COLOR),
            ))
            .with_children(|strip| {
                for slot in 0..SLOTS {
                    strip
                        .spawn((
                            Slot(slot),
                            Node {
                                width: Val::Px(SLOT_SIZE),
                                height: Val::Px(SLOT_SIZE),
                                border: UiRect::all(Val::Px(SLOT_BORDER)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BorderColor::all(SLOT_BORDER_COLOR),
                            BackgroundColor(Color::NONE),
                        ))
                        // filled in once there's a player's hotbar to draw
                        .with_child((
                            SlotIcon(slot),
                            icons.icon(BlockType::default()),
                            Visibility::Hidden,
                        ));
                }
            });
        });
}

fn select_slot(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut hotbar: Query<&mut Hotbar, With<Player>>,
    // how far the wheel's turned towards the next notch
    mut notches: Local<f32>,
) {
    let Ok(mut hotbar) = hotbar.single_mut() else {
        return;
    };
    if let Some(slot) = SLOT_KEYS.iter().position(|&key| keys.just_pressed(key)) {
        hotbar.selected = slot;
    }
    *notches += match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_NOTCH,
    };
    let turned = notches.trunc();
    if turned != 0.0 {
        *notches -= turned;
        // scrolling down moves to the right, as in Minecraft
        hotbar.scroll(-turned as i32);
    }
}

fn draw_hotbar(
    icons: Res<BlockIcons>,
    hotbar: Query<&Hotbar, (With<Player>, Changed<Hotbar>)>,
    mut slots: Query<(&Slot, &mut BorderColor, &mut BackgroundColor)>,
    mut slot_icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility)>,
) {
    let Ok(hotbar) = hotbar.single() else {
        return;
    };
    for (slot, mut border, mut background) in &mut slots {
        let selected = slot.0 == hotbar.selected;
        *border = BorderColor::all(if selected {
            SELECTED_BORDER_COLOR
        } else {
            SLOT_BORDER_COLOR
        });
        background.0 = if selected {
            SELECTED_COLOR
        } else {
            Color::NONE
        };
    }
    for (icon, mut image, mut visibility) in &mut slot_icons {
        *visibility = match hotbar.slots[icon.0] {
            Some(block) => {
                icons.paint(&mut image, block);
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use bevy::input::touch::TouchPhase;
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;

    use super::{Hotbar, HotbarPlugin, SLOTS};
    use crate::block::BlockType;
    use crate::commands::CommandPlugin;
    use crate::menu::GameState;
    use crate::player::Player;

    #[test]
    fn test_scrolling_wraps_round() {
        let mut hotbar = Hotbar::default();
        hotbar.scroll(-1);
        assert_eq!(SLOTS - 1, hotbar.selected);
        hotbar.scroll(2);
        assert_eq!(1, hotbar.selected);
        hotbar.scroll(SLOTS as i32 * 3);
        assert_eq!(1, hotbar.selected);
    }

    #[test]
    fn test_picking_selects_a_block_held_or_fills_the_selected_slot() {
        let mut hotbar = Hotbar::default();
        hotbar.pick(BlockType::Bricks, true);
        assert_eq!(Some(BlockType::Bricks), hotbar.held());
        assert_eq!(Hotbar::default().slots, hotbar.slots);

        hotbar.pick(BlockType::Furnace, false);
        assert_eq!(Some(BlockType::Bricks), hotbar.held());
        hotbar.pick(BlockType::Furnace, true);
        assert_eq!(Some(BlockType::Furnace), hotbar.held());
        assert_eq!(
            1,
            hotbar
                .slots
                .iter()
                .filter(|&&s| s == Some(BlockType::Furnace))
                .count()
        );
    }

    // The plugin, headless, with a player.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            StatesPlugin,
            AssetPlugin::default(),
            CommandPlugin,
            HotbarPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .init_state::<GameState>();
        app.world_mut().spawn(Player::default());
        app.update();
        app
    }

    fn selected(app: &mut App) -> usize {
        app.world_mut()
            .query::<&Hotbar>()
            .single(app.world())
            .unwrap()
            .selected
    }

    fn press(app: &mut App, key_code: KeyCode) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    fn scroll(app: &mut App, y: f32, unit: MouseScrollUnit) {
        app.world_mut().write_message(MouseWheel {
            unit,
            x: 0.0,
            y,
            window: Entity::PLACEHOLDER,
            phase: TouchPhase::Moved,
        });
        app.update();
    }

    #[test]
    fn test_number_keys_select_a_slot() {
        let mut app = app();
        press(&mut app, KeyCode::Digit5);
        assert_eq!(4, selected(&mut app));
        press(&mut app, KeyCode::Digit1);
        assert_eq!(0, selected(&mut app));
    }

    #[test]
    fn test_the_wheel_selects_the_next_slot_wrapping_round() {
        let mut app = app();
        scroll(&mut app, 1.0, MouseScrollUnit::Line);
        assert_eq!(SLOTS - 1, selected(&mut app));
        scroll(&mut app, -2.0, MouseScrollUnit::Line);
        assert_eq!(1, selected(&mut app));
        // a touchpad's pixels add up to a notch
        scroll(&mut app, -25.0, MouseScrollUnit::Pixel);
        assert_eq!(1, selected(&mut app));
        scroll(&mut app, -25.0, MouseScrollUnit::Pixel);
        assert_eq!(2, selected(&mut app));
    }

    #[test]
    fn test_nothing_is_selected_while_paused() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update();
        scroll(&mut app, -1.0, MouseScrollUnit::Line);
        assert_eq!(0, selected(&mut app));
    }
}
