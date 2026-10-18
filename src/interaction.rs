use bevy::ecs::system::SystemParam;
use bevy::math::I64Vec3;
use bevy::prelude::*;

use crate::block::shape::Use;
use crate::block::{Block, BlockType};
use crate::breaking::{break_time, crack_stage};
use crate::building::{placed, toggled};
use crate::client::command_bar::command_bar_closed;
use crate::client::creatures::{creature_hit, SeenCreature};
use crate::client::player::LocalPlayer;
use crate::collision::{boxes_at, looked_at, Hit};
use crate::containers::Container;
use crate::controls::{Action, Bindings, Controls};
use crate::crafting::Station;
use crate::creatures::Creature;
use crate::farming::tilled;
use crate::game_mode::GameMode;
use crate::inventory::furnace::ContainerView;
use crate::inventory::recipes::CraftingStation;
use crate::inventory::Hotbar;
use crate::inventory::Inventory;
use crate::items::{Item, ToolKind};
use crate::menu::GameState;
use crate::player::input::{player_look, player_move};
use crate::player::{PlayerLook, EYE_HEIGHT, REACH};
use crate::protocol::{HitCreature, PickBlock, RequestEdit, StartBreaking, UseBlock};
use crate::world::World;

// Just over a block wide, so the outline isn't hidden in the block's own faces.
const OUTLINE_SIZE: f32 = 1.004;
const OUTLINE_COLOUR: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
// How long after breaking a block, in seconds, the next starts to break with the mouse still
// held, as in Minecraft, so a held click doesn't mow through plants a block a frame.
const BREAK_REST: f32 = 0.25;
// In pixels. Bevy's default 2 reads as a faint hairline once TAA's softened it, so the
// outline was easy to miss against grass or in shade.
const OUTLINE_WIDTH: f32 = 3.0;

/// The outline's own gizmo settings, so it's drawn a little thicker than other gizmos.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct OutlineGizmos;

/// Looking at blocks, outlining the one looked at, and breaking, placing, picking and using
/// blocks with the mouse (§5.1), breaking and placing as requests to the server. Creative breaks
/// a block at a click, and survival when the mouse has been held on it for as long as it takes.
/// None of it happens while the game's paused, the block picker's up or the command bar's open.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .init_resource::<Target>()
            .init_resource::<TargetCreature>()
            .init_resource::<CraftingStation>()
            .init_resource::<ContainerView>()
            .init_resource::<Breaking>()
            .init_resource::<EditSeq>()
            .add_systems(OnExit(GameState::Playing), stop_breaking)
            .add_systems(
                Update,
                (
                    target_block,
                    (outline_target, break_target, use_target, use_station),
                )
                    .chain()
                    // from where the player is this frame
                    .after(player_move)
                    .after(player_look)
                    .run_if(in_state(GameState::Playing))
                    .run_if(command_bar_closed)
                    // a client joining a server has nothing to reach for until it's welcomed
                    .run_if(resource_exists::<World>),
            );
        let outline = GizmoConfig {
            line: GizmoLineConfig {
                width: OUTLINE_WIDTH,
                ..default()
            },
            ..default()
        };
        app.insert_gizmo_config(OutlineGizmos, outline);
    }
}

/// The block looked at, if there's one in reach.
#[derive(Resource, Default, Debug)]
pub struct Target(pub Option<Hit>);

/// The block the player's holding the mouse on to break in survival, if any, and how it's
/// getting on.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Breaking {
    on: Option<BreakingBlock>,
    // seconds before the next block starts to break, after one has
    rest: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BreakingBlock {
    block: I64Vec3,
    // how long it's been held, from the frame it was started on, and how long it takes, in
    // seconds
    held: f32,
    takes: f32,
}

/// What breaking a block with the mouse held tells the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakStep {
    Start(I64Vec3),
    Break(I64Vec3),
}

impl Breaking {
    /// The block being broken and which of its cracks it shows, if there's one.
    pub fn cracks(&self) -> Option<(I64Vec3, usize)> {
        self.on.map(|on| (on.block, crack_stage(on.held, on.takes)))
    }

    // The mouse held on `target`, a block and how long it takes to break, or let go if none,
    // for `dt` seconds. A new block starts from nothing, so looking away or letting go loses
    // the progress, and the frame it starts on counts no time, as the server's timing of it
    // starts from that frame too. A block that breaks at a touch needs no start.
    fn hold(&mut self, target: Option<(I64Vec3, f32)>, dt: f32) -> Option<BreakStep> {
        let Some((block, takes)) = target else {
            *self = Self::default();
            return None;
        };
        if self.rest > 0.0 {
            self.rest -= dt;
            return None;
        }
        let on = match &mut self.on {
            Some(on) if on.block == block => {
                on.held += dt;
                // the tool held may have changed
                on.takes = takes;
                *on
            }
            _ => {
                let on = BreakingBlock {
                    block,
                    held: 0.0,
                    takes,
                };
                self.on = Some(on);
                if takes > 0.0 {
                    return Some(BreakStep::Start(block));
                }
                on
            }
        };
        if on.held < on.takes {
            return None;
        }
        self.on = None;
        self.rest = BREAK_REST;
        Some(BreakStep::Break(block))
    }
}

// Each edit request a client makes is numbered, so it can tell which an answer is to.
#[derive(Resource, Default)]
struct EditSeq(u32);

// What's asked of the server about blocks.
#[derive(SystemParam)]
struct EditRequests<'w> {
    edits: MessageWriter<'w, RequestEdit>,
    starts: MessageWriter<'w, StartBreaking>,
    seq: ResMut<'w, EditSeq>,
}

impl EditRequests<'_> {
    fn edit(&mut self, block: I64Vec3, to: impl Into<Block>, slot: u8) {
        self.seq.0 = self.seq.0.wrapping_add(1);
        let seq = self.seq.0;
        self.edits.write(RequestEdit {
            block,
            to: to.into(),
            slot,
            seq,
        });
    }
}

/// The creature looked at, if there's one in reach nearer than any block, which a left click
/// hits rather than the block behind it.
#[derive(Resource, Default, Debug)]
pub struct TargetCreature(pub Option<Entity>);

// The block looked at, or the creature in front of it.
fn target_block(
    world: Res<World>,
    mut target: ResMut<Target>,
    mut creature: ResMut<TargetCreature>,
    player: Query<(&Transform, &PlayerLook), With<LocalPlayer>>,
    seen: Query<(&SeenCreature, &Transform)>,
    creatures: Query<&Creature>,
) {
    let (feet, look) = player.single().expect("player does not exist");
    let block = looked_at(&world, feet.translation, *look);
    let eye = feet.translation + Vec3::Y * EYE_HEIGHT;
    let block_distance = block.map_or(REACH, |hit| eye.distance(hit.point));
    let seen = seen.iter().filter_map(|(seen, body)| {
        creatures
            .get(seen.0)
            .ok()
            .map(|creature| (seen, body, creature.0))
    });
    creature.0 = creature_hit(eye, look.forward(), block_distance, seen).map(|(hit, _)| hit);
    target.0 = block.filter(|_| creature.0.is_none());
}

// Each box of the shape looked at, so the outline fits a slab or a fence as it does a cube.
fn outline_target(world: Res<World>, target: Res<Target>, mut gizmos: Gizmos<OutlineGizmos>) {
    if let Some(hit) = target.0 {
        let corner = hit.block.as_vec3();
        for part in boxes_at(hit.block, Use::Outline, |at| world.get(at)).iter() {
            let centre = corner + (part.min + part.max) / 2.0;
            let size = part.max - part.min + (OUTLINE_SIZE - 1.0);
            gizmos.cube(
                Transform::from_translation(centre).with_scale(size),
                OUTLINE_COLOUR,
            );
        }
    }
}

// Left click breaks the block looked at in creative. In survival the mouse is held on it, the
// server told when it starts, and it breaks once it's been held as long as it takes with the
// tool in the selected slot, which the server checks. On a creature, it hits it.
#[allow(clippy::too_many_arguments)]
fn break_target(
    world: Res<World>,
    target: Res<Target>,
    creature: Res<TargetCreature>,
    mut hits: MessageWriter<HitCreature>,
    controls: Controls,
    time: Res<Time>,
    player: Query<(&GameMode, &Hotbar, &Inventory), With<LocalPlayer>>,
    mut breaking: ResMut<Breaking>,
    mut requests: EditRequests,
) {
    let (mode, hotbar, inventory) = player.single().expect("player does not exist");
    let slot = hotbar.selected as u8;
    if let Some(creature) = creature.0.filter(|_| controls.just_pressed(Action::Break)) {
        hits.write(HitCreature { creature });
    }
    if mode.breaks_at_once() {
        if let Some(hit) = target.0.filter(|_| controls.just_pressed(Action::Break)) {
            requests.edit(hit.block, BlockType::Air, slot);
        }
        return;
    }
    let tool = hotbar.held(inventory).and_then(|stack| stack.item.tool());
    let held = target.0.filter(|_| controls.pressed(Action::Break));
    let held = held.map(|hit| (hit.block, break_time(world.get_block_at(hit.block), tool)));
    match breaking.hold(held, time.delta_secs()) {
        Some(BreakStep::Start(block)) => {
            requests.starts.write(StartBreaking { block });
        }
        Some(BreakStep::Break(block)) => requests.edit(block, BlockType::Air, slot),
        None => {}
    }
}

// Breaking starts again from nothing once the game's back from a menu or the inventory.
fn stop_breaking(mut breaking: ResMut<Breaking>) {
    *breaking = Breaking::default();
}

// Right click places the selected block in the hotbar, or plants the seeds there, against the
// face looked at, or in place of a block that's replaceable, such as tall grass, and with a hoe
// tills the dirt or grass looked at. Middle click selects the hotbar slot holding the block
// looked at, or in creative asks for it in the selected slot. The server decides whether a block
// can go where it's placed: into a replaceable block, never into a player, and in survival only
// if it's held, and whether what's tilled has room above it.
fn use_target(
    world: Res<World>,
    target: Res<Target>,
    controls: Controls,
    mut player: Query<(&GameMode, &mut Hotbar, &Inventory, &PlayerLook), With<LocalPlayer>>,
    mut requests: EditRequests,
    mut picks: MessageWriter<PickBlock>,
) {
    let Some(hit) = target.0 else {
        return;
    };
    let (mode, mut hotbar, inventory, look) = player.single_mut().expect("player does not exist");
    let slot = hotbar.selected as u8;
    let replaceable = world.get_block_at(hit.block).is_replaceable();
    // and right click on a crafting table, a furnace or a bed uses it instead, see
    // `use_station`
    let places = (replaceable || hit.face != I64Vec3::ZERO) && !is_used(&world, hit, *mode);
    if controls.just_pressed(Action::Place) && places {
        let held = hotbar.held(inventory).map(|stack| stack.item);
        let hoe = held
            .and_then(Item::tool)
            .is_some_and(|tool| tool.kind == ToolKind::Hoe);
        if let Some(farmland) = tilled(world.get_block_at(hit.block)).filter(|_| hoe) {
            requests.edit(hit.block, farmland, slot);
        } else if let Some(kind) = held.and_then(Item::places) {
            let at = if replaceable {
                hit.block
            } else {
                hit.block + hit.face
            };
            let point = hit.point - hit.block.as_vec3();
            requests.edit(at, placed(kind, hit.face, point, look.forward()), slot);
        }
    }
    if controls.just_pressed(Action::Pick) {
        let block = world.get_block_at(hit.block);
        if !hotbar.pick(block, inventory) && mode.has_every_block() {
            picks.write(PickBlock {
                slot,
                item: Item::Block(block),
            });
        }
    }
}

/// Whether right-clicking `hit` uses it, rather than building against it or eating: a bed,
/// a door or a trapdoor always, and a station or a container in survival.
pub fn is_used(world: &World, hit: Hit, mode: GameMode) -> bool {
    world.get_block_at(hit.block) == BlockType::Bed
        || toggled(world.get(hit.block)).is_some()
        || opens_screen(world, hit, mode)
}

// Whether right-clicking `hit` opens the inventory screen at it, as a station or a container
// does in survival: in creative, where there's no crafting, smelting or inventory screen, it's
// built against like any other block.
fn opens_screen(world: &World, hit: Hit, mode: GameMode) -> bool {
    let block = world.get_block_at(hit.block);
    let opened = Station::of_block(block).is_some() || Container::new(block).is_some();
    opened && !mode.has_every_block()
}

// Right click on a crafting table, a furnace or a chest opens the inventory screen at it, as the
// server's told, so what's made there is checked against it, and a container's sent, and on a
// bed tells the server, which sleeps in it.
#[allow(clippy::too_many_arguments)]
fn use_station(
    world: Res<World>,
    target: Res<Target>,
    controls: Controls,
    player: Query<&GameMode, With<LocalPlayer>>,
    mut station: ResMut<CraftingStation>,
    mut view: ResMut<ContainerView>,
    mut state: ResMut<NextState<GameState>>,
    mut uses: MessageWriter<UseBlock>,
) {
    let (Some(hit), Ok(&mode)) = (target.0, player.single()) else {
        return;
    };
    if !controls.just_pressed(Action::Place) {
        return;
    }
    if world.get_block_at(hit.block) == BlockType::Bed || toggled(world.get(hit.block)).is_some() {
        uses.write(UseBlock { block: hit.block });
    } else if opens_screen(&world, hit, mode) {
        let block = world.get_block_at(hit.block);
        uses.write(UseBlock { block: hit.block });
        *station = CraftingStation(Station::of_block(block).unwrap_or_default());
        *view = ContainerView::opening(block, hit.block);
        state.set(GameState::Inventory);
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, Vec3};

    use super::{BreakStep, Breaking, BREAK_REST};
    use crate::breaking::CRACK_STAGES;
    use crate::collision::overlaps;
    use crate::collision::raycast;
    use crate::player::BODY;
    use crate::player::REACH;

    #[test]
    fn test_holding_on_a_block_starts_it_cracks_it_and_breaks_it_once_its_taken_long_enough() {
        let mut breaking = Breaking::default();
        let block = I64Vec3::new(1, 2, 3);
        // a tenth of a second a stage
        let target = Some((block, CRACK_STAGES as f32 * 0.1));
        assert_eq!(Some(BreakStep::Start(block)), breaking.hold(target, 0.1));
        // the frame it starts on counts no time
        assert_eq!(Some((block, 0)), breaking.cracks());
        let mut stages = vec![];
        for _ in 1..CRACK_STAGES {
            assert_eq!(None, breaking.hold(target, 0.1));
            stages.push(breaking.cracks().unwrap().1);
        }
        assert_eq!((1..CRACK_STAGES).collect::<Vec<_>>(), stages);
        assert_eq!(Some(BreakStep::Break(block)), breaking.hold(target, 0.1));
        assert_eq!(None, breaking.cracks());
        // and the next waits a moment before it starts
        let next = Some((block + I64Vec3::Y, 0.8));
        let rest = (BREAK_REST / 0.1).ceil() as usize;
        for _ in 0..rest {
            assert_eq!(None, breaking.hold(next, 0.1));
        }
        assert_eq!(
            Some(BreakStep::Start(block + I64Vec3::Y)),
            breaking.hold(next, 0.1)
        );
    }

    #[test]
    fn test_letting_go_or_looking_away_starts_breaking_again_from_nothing() {
        let mut breaking = Breaking::default();
        let (block, other) = (I64Vec3::ZERO, I64Vec3::X);
        breaking.hold(Some((block, 1.0)), 0.1);
        for _ in 0..5 {
            breaking.hold(Some((block, 1.0)), 0.1);
        }
        assert_eq!(Some((block, 4)), breaking.cracks());
        assert_eq!(None, breaking.hold(None, 0.1));
        assert_eq!(None, breaking.cracks());
        assert_eq!(
            Some(BreakStep::Start(block)),
            breaking.hold(Some((block, 1.0)), 0.1)
        );
        breaking.hold(Some((block, 1.0)), 0.5);
        assert_eq!(
            Some(BreakStep::Start(other)),
            breaking.hold(Some((other, 1.0)), 0.1)
        );
        assert_eq!(Some((other, 0)), breaking.cracks());
    }

    #[test]
    fn test_what_breaks_at_a_touch_breaks_without_starting() {
        let mut breaking = Breaking::default();
        let block = I64Vec3::ZERO;
        assert_eq!(
            Some(BreakStep::Break(block)),
            breaking.hold(Some((block, 0.0)), 0.1)
        );
    }

    // a floor from y 0 down, and a pillar at x 3, z 0 standing on it
    fn stops(block: I64Vec3) -> bool {
        block.y <= 0 || (block.x == 3 && block.z == 0)
    }

    #[test]
    fn test_hits_the_block_looked_at_through_the_face_facing_the_eye() {
        let eye = Vec3::new(0.5, 1.5, 0.5);
        let hit = raycast(eye, Vec3::X, REACH, stops).unwrap();
        assert_eq!(
            (I64Vec3::new(3, 1, 0), I64Vec3::NEG_X),
            (hit.block, hit.face)
        );
        // where it meets the pillar's face
        assert_eq!(Vec3::new(3.0, 1.5, 0.5), hit.point);
        let hit = raycast(eye, Vec3::NEG_Y, REACH, stops).unwrap();
        assert_eq!((I64Vec3::ZERO, I64Vec3::Y), (hit.block, hit.face));
        // at a slant, onto the floor two blocks off
        let hit = raycast(eye, Vec3::new(-4.0, -1.0, 0.0), REACH, stops).unwrap();
        assert_eq!((I64Vec3::new(-2, 0, 0), I64Vec3::Y), (hit.block, hit.face));
    }

    #[test]
    fn test_nothing_past_reach_is_hit() {
        let eye = Vec3::new(-2.5, 1.5, 0.5);
        assert!(raycast(eye, Vec3::X, REACH, stops).is_none());
        assert!(raycast(eye + Vec3::X, Vec3::X, REACH, stops).is_some());
        assert!(raycast(eye, Vec3::Y, REACH, stops).is_none());
    }

    #[test]
    fn test_air_and_water_are_looked_through() {
        use crate::block::BlockType;
        // a pool two deep over the floor at y 0, with a stone in it at x 2
        let block = |b: I64Vec3| match b {
            b if b.y <= 0 => BlockType::Sand,
            b if b == I64Vec3::new(2, 1, 0) => BlockType::Stone,
            b if b.y <= 2 => BlockType::Water,
            _ => BlockType::Air,
        };
        let stops = |b| block(b).is_solid();
        let hit = raycast(Vec3::new(-1.5, 1.5, 0.5), Vec3::X, REACH, stops).unwrap();
        assert_eq!(I64Vec3::new(2, 1, 0), hit.block);
        // and from above the water, down through it to the bottom
        let hit = raycast(Vec3::new(0.5, 3.5, 0.5), Vec3::NEG_Y, REACH, stops).unwrap();
        assert_eq!((I64Vec3::ZERO, I64Vec3::Y), (hit.block, hit.face));
    }

    #[test]
    fn test_plants_are_hit_but_air_and_water_looked_through() {
        use crate::block::BlockType;
        // a poppy on the floor at x 2, with water beyond it
        let block = |b: I64Vec3| match b {
            b if b.y <= 0 => BlockType::Grass,
            b if b == I64Vec3::new(2, 1, 0) => BlockType::Poppy,
            b if b.x > 2 && b.y == 1 => BlockType::Water,
            _ => BlockType::Air,
        };
        let stops = |b| block(b).is_tangible();
        let hit = raycast(Vec3::new(-0.5, 1.5, 0.5), Vec3::X, REACH, stops).unwrap();
        assert_eq!(
            (I64Vec3::new(2, 1, 0), I64Vec3::NEG_X),
            (hit.block, hit.face)
        );
        let hit = raycast(Vec3::new(3.5, 1.5, 0.5), Vec3::X, REACH, stops);
        assert!(hit.is_none());
    }

    #[test]
    fn test_a_ray_starting_in_a_block_hits_it_with_no_face() {
        let hit = raycast(Vec3::new(0.5, -0.5, 0.5), Vec3::X, REACH, stops).unwrap();
        assert_eq!(
            (I64Vec3::new(0, -1, 0), I64Vec3::ZERO),
            (hit.block, hit.face)
        );
    }

    #[test]
    fn test_blocks_can_go_beside_and_under_the_player_but_not_in_them() {
        let feet = Vec3::new(0.5, 1.0, 0.5);
        assert!(overlaps(BODY, feet, I64Vec3::new(0, 1, 0)));
        assert!(overlaps(BODY, feet, I64Vec3::new(0, 2, 0)));
        // under the feet, over the head and beside the body only touch it
        assert!(!overlaps(BODY, feet, I64Vec3::new(0, 0, 0)));
        assert!(!overlaps(BODY, feet, I64Vec3::new(0, 3, 0)));
        assert!(!overlaps(BODY, feet, I64Vec3::new(1, 1, 0)));
        // straddling two columns, the body is in both
        let between = Vec3::new(1.0, 1.0, 0.5);
        assert!(overlaps(BODY, between, I64Vec3::new(0, 1, 0)));
        assert!(overlaps(BODY, between, I64Vec3::new(1, 1, 0)));
    }

    mod clicks {
        use bevy::asset::Handle;
        use bevy::ecs::message::Messages;
        use bevy::math::I64Vec3;
        use bevy::prelude::*;
        use bevy::state::app::StatesPlugin;
        use bevy_replicon::prelude::ClientId;
        use bevy_replicon::RepliconPlugins;

        use super::super::{
            break_target, use_station, use_target, Breaking, EditSeq, Hit, Target, TargetCreature,
        };
        use crate::block::BlockType;
        use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
        use crate::chunks::chunk_loader::ChunkLoader;
        use crate::chunks::group::BuildChunkGroup;
        use crate::chunks::lod::{LodKey, LodLayout};
        use crate::client::player::LocalPlayer;
        use crate::client::ClientPlugin;
        use crate::crafting::Station;
        use crate::game_mode::GameMode;
        use crate::inventory::furnace::ContainerView;
        use crate::inventory::recipes::CraftingStation;
        use crate::inventory::Hotbar;
        use crate::inventory::Inventory;
        use crate::items::{Item, ItemStack};
        use crate::menu::GameState;
        use crate::player::{Player, PlayerOwner};
        use crate::protocol::{
            EditRejected, HitCreature, PickBlock, ProtocolPlugin, RequestEdit, StartBreaking,
            UseBlock,
        };
        use crate::server::ServerPlugin;

        // Looking down at the top of a block of sand at the origin, from `feet`.
        fn app(feet: Vec3, mode: GameMode) -> App {
            let mut world = crate::world::World::new(16);
            world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
            world.edit(I64Vec3::ZERO, BlockType::Sand);
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, StatesPlugin))
                .insert_state(GameState::Playing)
                .add_message::<RequestEdit>()
                .add_message::<PickBlock>()
                .add_message::<UseBlock>()
                .add_message::<StartBreaking>()
                .add_message::<HitCreature>()
                .init_resource::<TargetCreature>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<crate::controls::Bindings>()
                .init_resource::<CraftingStation>()
                .init_resource::<ContainerView>()
                .init_resource::<Breaking>()
                .init_resource::<EditSeq>()
                .insert_resource(world)
                .insert_resource(Target(Some(Hit {
                    block: I64Vec3::ZERO,
                    face: I64Vec3::Y,
                    point: Vec3::new(0.5, 1.0, 0.5),
                })))
                .add_systems(Update, (break_target, use_target, use_station));
            app.world_mut().spawn((
                Player::default(),
                PlayerOwner(ClientId::Server),
                LocalPlayer,
                mode,
                Inventory::starting(mode),
                Transform::from_translation(feet),
            ));
            app
        }

        // The same, played in singleplayer, with the server and client in the App and the
        // group of chunks at the origin drawn.
        fn played(feet: Vec3, mode: GameMode) -> App {
            let mut app = app(feet, mode);
            app.add_plugins((RepliconPlugins, ProtocolPlugin, ServerPlugin, ClientPlugin));
            let height = app.world().resource::<crate::world::World>().height;
            let mut loader = ChunkLoader::new(
                LodLayout::new(32, 128, 2),
                height,
                false,
                Handle::default(),
                Handle::default(),
            );
            // all of them, as a group is only rebuilt once every chunk in it has loaded
            for chunk in LodKey::containing(ChunkCoordinate(I64Vec3::ZERO), 1).chunks() {
                loader.track(chunk, app.world_mut().spawn_empty().id());
            }
            app.insert_resource(loader).finish();
            app
        }

        const AWAY: Vec3 = Vec3::new(3.5, 1.0, 0.5);

        fn press(app: &mut App, button: MouseButton) {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release_all();
            mouse.clear();
            mouse.press(button);
        }

        fn click(app: &mut App, button: MouseButton) -> Vec<RequestEdit> {
            press(app, button);
            app.update();
            let mut messages = app.world_mut().resource_mut::<Messages<RequestEdit>>();
            messages.drain().collect()
        }

        fn hotbar(app: &mut App) -> Mut<'_, Hotbar> {
            app.world_mut()
                .query::<&mut Hotbar>()
                .single_mut(app.world_mut())
                .unwrap()
        }

        fn inventory(app: &mut App) -> Mut<'_, Inventory> {
            app.world_mut()
                .query::<&mut Inventory>()
                .single_mut(app.world_mut())
                .unwrap()
        }

        #[test]
        fn test_left_click_asks_to_break_and_right_click_to_place_against_the_face() {
            let mut app = app(AWAY, GameMode::Creative);
            let at = |block, to: BlockType, seq| {
                vec![RequestEdit {
                    block,
                    to: to.into(),
                    slot: 0,
                    seq,
                }]
            };
            assert_eq!(
                at(I64Vec3::ZERO, BlockType::Air, 1),
                click(&mut app, MouseButton::Left)
            );
            assert_eq!(
                at(I64Vec3::Y, BlockType::Stone, 2),
                click(&mut app, MouseButton::Right)
            );
        }

        #[test]
        fn test_placing_onto_tall_grass_replaces_it() {
            let mut app = app(AWAY, GameMode::Creative);
            app.world_mut()
                .resource_mut::<crate::world::World>()
                .edit(I64Vec3::ZERO, BlockType::TallGrass);
            let requests = click(&mut app, MouseButton::Right);
            assert_eq!(
                vec![I64Vec3::ZERO],
                requests.iter().map(|r| r.block).collect::<Vec<_>>()
            );
        }

        #[test]
        fn test_placing_takes_the_selected_slots_block() {
            let mut app = app(AWAY, GameMode::Creative);
            hotbar(&mut app).selected = 3;
            let to =
                |requests: Vec<RequestEdit>| requests.iter().map(|r| r.to.kind).collect::<Vec<_>>();
            assert_eq!(
                vec![BlockType::Bricks],
                to(click(&mut app, MouseButton::Right))
            );
            // and an empty slot places nothing
            inventory(&mut app).slots[3] = None;
            assert!(click(&mut app, MouseButton::Right).is_empty());
        }

        #[test]
        fn test_right_click_with_a_hoe_tills_and_with_seeds_plants() {
            let mut app = app(AWAY, GameMode::Survival);
            let to = |requests: Vec<RequestEdit>| {
                let edits = requests.iter().map(|r| (r.block, r.to.kind));
                edits.collect::<Vec<_>>()
            };
            inventory(&mut app).slots[0] = Some(ItemStack::new(Item::StoneHoe, 1));
            // sand can't be tilled, and a hoe places nothing
            assert!(click(&mut app, MouseButton::Right).is_empty());
            app.world_mut()
                .resource_mut::<crate::world::World>()
                .edit(I64Vec3::ZERO, BlockType::Grass);
            assert_eq!(
                vec![(I64Vec3::ZERO, BlockType::Farmland)],
                to(click(&mut app, MouseButton::Right))
            );
            inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Seeds, 5));
            assert_eq!(
                vec![(I64Vec3::Y, BlockType::Wheat0)],
                to(click(&mut app, MouseButton::Right))
            );
        }

        #[test]
        fn test_a_click_changes_the_world_and_queues_its_redraw_in_the_same_frame() {
            let mut app = played(AWAY, GameMode::Creative);
            press(&mut app, MouseButton::Right);
            // the request, the server's checks and the start of rebuilding the meshes all
            // happen in the click's frame
            app.update();
            let world = app.world().resource::<crate::world::World>();
            assert_eq!(BlockType::Stone, world.get_block_at(I64Vec3::Y));
            let drawn = LodKey::containing(ChunkCoordinate(I64Vec3::ZERO), 1);
            let loader = app.world().resource::<ChunkLoader>();
            assert!(loader.rebuilding.is_empty());
            let group = loader
                .group_entity(drawn)
                .expect("the group isn't being built");
            assert!(app.world().entity(group).contains::<BuildChunkGroup>());
        }

        #[test]
        fn test_nothing_is_placed_where_the_player_stands() {
            let mut app = played(Vec3::new(0.5, 1.0, 0.5), GameMode::Creative);
            press(&mut app, MouseButton::Right);
            app.update();
            let world = app.world().resource::<crate::world::World>();
            assert_eq!(BlockType::Air, world.get_block_at(I64Vec3::Y));
            assert!(app.world().resource::<ChunkLoader>().rebuilding.is_empty());
            let rejected = app.world().resource::<Messages<EditRejected>>();
            assert_eq!(1, rejected.len());
        }

        #[test]
        fn test_survival_placing_uses_up_the_block_held() {
            let mut app = played(AWAY, GameMode::Survival);
            let dirt = ItemStack::new(Item::Block(BlockType::Dirt), 1);
            inventory(&mut app).slots[0] = Some(dirt);
            press(&mut app, MouseButton::Right);
            app.update();
            let world = app.world().resource::<crate::world::World>();
            assert_eq!(BlockType::Dirt, world.get_block_at(I64Vec3::Y));
            assert_eq!(None, inventory(&mut app).slots[0]);
        }

        #[test]
        fn test_middle_click_in_creative_picks_the_block_into_the_selected_slot_or_selects_it() {
            let mut app = played(AWAY, GameMode::Creative);
            hotbar(&mut app).selected = 2;
            press(&mut app, MouseButton::Middle);
            app.update();
            let sand = Some(ItemStack::full(Item::Block(BlockType::Sand)));
            assert_eq!(sand, inventory(&mut app).slots[2]);
            // already in the hotbar, it's selected rather than put in a second slot
            hotbar(&mut app).selected = 6;
            press(&mut app, MouseButton::Middle);
            app.update();
            assert_eq!(2, hotbar(&mut app).selected);
            let dirt = Some(ItemStack::full(Item::Block(BlockType::Dirt)));
            assert_eq!(dirt, inventory(&mut app).slots[6]);
        }

        #[test]
        fn test_middle_click_in_survival_only_selects() {
            let mut app = app(AWAY, GameMode::Survival);
            click(&mut app, MouseButton::Middle);
            assert_eq!(Hotbar::default(), *hotbar(&mut app));
            assert!(app.world().resource::<Messages<PickBlock>>().is_empty());
            let sand = ItemStack::new(Item::Block(BlockType::Sand), 1);
            inventory(&mut app).slots[4] = Some(sand);
            click(&mut app, MouseButton::Middle);
            assert_eq!(4, hotbar(&mut app).selected);
        }

        #[test]
        fn test_right_clicking_a_crafting_table_in_survival_opens_it_rather_than_building_on_it() {
            let mut app = app_in(GameMode::Survival);
            inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Dirt), 1));
            assert!(click(&mut app, MouseButton::Right).is_empty());
            let uses: Vec<_> = app
                .world_mut()
                .resource_mut::<Messages<UseBlock>>()
                .drain()
                .collect();
            assert_eq!(
                vec![UseBlock {
                    block: I64Vec3::ZERO
                }],
                uses
            );
            assert_eq!(
                CraftingStation(Station::CraftingTable),
                *app.world().resource::<CraftingStation>()
            );
            app.update();
            let state = app.world().resource::<State<GameState>>();
            assert_eq!(GameState::Inventory, *state.get());

            // while in creative, it's built on
            let mut app = app_in(GameMode::Creative);
            assert_eq!(1, click(&mut app, MouseButton::Right).len());
            assert_eq!(
                CraftingStation(Station::Hand),
                *app.world().resource::<CraftingStation>()
            );
        }

        // Looking at a crafting table, in `mode`.
        fn app_in(mode: GameMode) -> App {
            let mut app = app(AWAY, mode);
            app.world_mut()
                .resource_mut::<crate::world::World>()
                .edit(I64Vec3::ZERO, BlockType::CraftingTable);
            app
        }

        #[test]
        fn test_survival_breaks_a_block_once_the_mouse_has_been_held_on_it_long_enough() {
            use std::time::Duration;

            use bevy::time::TimeUpdateStrategy;

            let mut app = played(AWAY, GameMode::Survival);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )));
            // sand takes 0.75 seconds by hand
            press(&mut app, MouseButton::Left);
            app.update();
            let sand = |app: &App| {
                let world = app.world().resource::<crate::world::World>();
                world.get_block_at(I64Vec3::ZERO) == BlockType::Sand
            };
            for _ in 0..7 {
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .clear();
                app.update();
                assert!(sand(&app));
            }
            let cracks = app.world().resource::<Breaking>().cracks();
            assert_eq!(Some((I64Vec3::ZERO, 7)), cracks);
            app.update();
            assert!(!sand(&app));
            assert_eq!(None, app.world().resource::<Breaking>().cracks());
        }
    }

    mod outline {
        use std::any::TypeId;

        use bevy::gizmos::{GizmoAsset, GizmoHandles};
        use bevy::prelude::*;

        use super::super::{outline_target, Hit, OutlineGizmos, Target, OUTLINE_SIZE};

        // the lines the outline drew last frame
        fn outline(target: Option<Hit>) -> Vec<Vec3> {
            let mut app = App::new();
            // just the gizmos' own parts of `GizmoPlugin`, which wants a renderer's worth more
            app.add_plugins((MinimalPlugins, AssetPlugin::default()))
                .init_asset::<GizmoAsset>()
                .init_resource::<GizmoHandles>()
                .init_gizmo_group::<OutlineGizmos>()
                .insert_resource(crate::world::World::new(16))
                .insert_resource(Target(target))
                .add_systems(Update, outline_target);
            app.update();
            let handles = app.world().resource::<GizmoHandles>().handles();
            let Some(Some(handle)) = handles.get(&TypeId::of::<OutlineGizmos>()) else {
                return Vec::new();
            };
            let assets = app.world().resource::<Assets<GizmoAsset>>();
            let buffer = assets.get(handle).unwrap().buffer();
            let view = buffer.buffer();
            [view.list_positions.clone(), view.strip_positions.clone()]
                .concat()
                .into_iter()
                .filter(|position| position.is_finite())
                .collect()
        }

        #[test]
        fn test_outlines_the_block_looked_at_from_just_outside() {
            let block = IVec3::new(3, -2, 7).as_i64vec3();
            let lines = outline(Some(Hit {
                block,
                face: IVec3::Y.as_i64vec3(),
                point: block.as_vec3() + Vec3::new(0.5, 1.0, 0.5),
            }));
            assert!(!lines.is_empty());
            let margin = (OUTLINE_SIZE - 1.0) / 2.0;
            for corner in lines {
                let offset = corner - block.as_vec3();
                for d in offset.to_array() {
                    assert!(
                        (d + margin).abs() < 1e-4 || (d - 1.0 - margin).abs() < 1e-4,
                        "{corner} isn't a corner of the outline"
                    );
                }
            }
        }

        #[test]
        fn test_outlines_nothing_when_nothing_is_looked_at() {
            assert!(outline(None).is_empty());
        }
    }
}
