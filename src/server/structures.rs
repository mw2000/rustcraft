//! Authoring structures (§5.6): the operators' commands that select a box, save it as a
//! template in the world's `structures/` directory, build templates into the world as edits to
//! try them out, and list them. Templates are a few kilobytes, read and written as the command
//! runs, as rarely as an operator types one.

use std::borrow::Cow;
use std::fs;
use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::drops::DroppedItem;
use super::sync::BlockChanges;
use crate::authoring::{Selection, MARKERS, MAX_SIZE};
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::generate::structures::{Placement, Structure, Template, STRUCTURES};
use crate::collision::looked_at;
use crate::containers::Containers;
use crate::farming::Crops;
use crate::player::{PlayerLook, PlayerOwner};
use crate::protocol::{BlockChanged, SelectionChanged};
use crate::world::World;

// Where in a world's directory its structures are saved.
const STRUCTURES_DIR: &str = "structures";

/// What the structure commands read and change besides the world.
#[derive(SystemParam)]
pub(super) struct Authoring<'w, 's> {
    players: Query<'w, 's, (&'static PlayerLook, &'static mut Selection)>,
    containers: ResMut<'w, Containers>,
    crops: ResMut<'w, Crops>,
    changed: BlockChanges<'w, 's>,
    commands: Commands<'w, 's>,
}

impl Authoring<'_, '_> {
    /// Moves corner `corner` of the selection of `player`, whose feet are at `feet`, to the
    /// block at `at`, or else to the one they look at, or else the one they stand in.
    pub(super) fn corner(
        &mut self,
        world: &World,
        player: Entity,
        feet: Vec3,
        corner: usize,
        at: Option<Vec3>,
    ) -> String {
        let (&look, mut selection) = self.players.get_mut(player).unwrap();
        let block = match at {
            Some(at) => at.floor().as_i64vec3(),
            None => looked_at(world, feet, look).map_or(feet.floor().as_i64vec3(), |hit| hit.block),
        };
        selection.set(corner, block);
        let set = format!("Corner {} at {}", corner + 1, coordinates(block));
        match selection.bounds() {
            Some((_, size)) if size.cmpgt(MAX_SIZE).any() => format!(
                "{set}, selecting {}, too big to save: a structure is at most {}",
                dimensions(size),
                dimensions(MAX_SIZE)
            ),
            Some((_, size)) => format!("{set}, selecting {}", dimensions(size)),
            None => format!("{set}; mark the other with /pos{}", 2 - corner),
        }
    }

    /// Saves the selection of `player` as the structure `name` in the world's directory `dir`,
    /// with the layer `floor` from its bottom standing at ground level, or else the floor of the
    /// structure placed into it, or else its bottom layer. Structure voids are saved as cells left
    /// as the world has them and markers as markers. Its placement is the structure's of that
    /// name if there is one, so a structure reworked keeps it, and otherwise a start to edit.
    pub(super) fn save(
        &self,
        world: &World,
        dir: Option<&Path>,
        player: Entity,
        name: &str,
        floor: Option<i64>,
    ) -> Result<String, String> {
        let (_, selection) = self.players.get(player).unwrap();
        let (min, size) = selection
            .bounds()
            .ok_or("Select a box first, with /pos1 and /pos2")?;
        if size.cmpgt(MAX_SIZE).any() {
            return Err(format!(
                "The selection is {}, and a structure is at most {}",
                dimensions(size),
                dimensions(MAX_SIZE)
            ));
        }
        let (first, last) = (
            ChunkCoordinate::containing(min).0,
            ChunkCoordinate::containing(min + size - 1).0,
        );
        for x in first.x..=last.x {
            for y in first.y..=last.y {
                for z in first.z..=last.z {
                    if !world.is_chunk_generated(ChunkCoordinate(I64Vec3::new(x, y, z))) {
                        return Err("Part of the selection isn't loaded: come closer to it".into());
                    }
                }
            }
        }
        let floor = floor
            .or(selection.floor.map(|floor| floor - min.y))
            .unwrap_or(0);

        let mut cells = Vec::with_capacity((size.x * size.y * size.z) as usize);
        let mut markers = Vec::new();
        for y in 0..size.y {
            for z in 0..size.z {
                for x in 0..size.x {
                    let at = I64Vec3::new(x, y, z);
                    let block = world.get_block_at(min + at);
                    if let Some(&(_, kind)) = MARKERS.iter().find(|(marker, _)| *marker == block) {
                        markers.push((kind.to_string(), at));
                    }
                    cells.push((!block.is_for_authoring()).then_some(block));
                }
            }
        }
        // what it's built on, which most likely suits the ground under it too
        let layer = (size.x * size.z) as usize;
        let foundation = commonest(&cells[..layer])
            .or_else(|| commonest(&cells))
            .ok_or("There's nothing in the selection to save")?;
        let placement =
            load(dir, name).map_or_else(|_| Placement::default(), |s| s.placement.clone());
        let structure = Structure {
            name: name.into(),
            placement,
            template: Template::new(size, floor, foundation, cells, markers),
        };
        let text = structure.to_toml();
        if let Err(e) = toml::from_str::<Structure>(&text) {
            return Err(format!("It wouldn't load as a structure: {}", e.message()));
        }
        let dir = dir
            .ok_or("This world isn't saved, so there's nowhere to save it")?
            .join(STRUCTURES_DIR);
        let path = dir.join(format!("{name}.toml"));
        fs::create_dir_all(&dir)
            .and_then(|_| fs::write(&path, text))
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
        Ok(format!(
            "Saved {name}, {} with its floor on layer {floor}, to {}",
            dimensions(size),
            path.display()
        ))
    }

    /// Builds the structure `name` turned `rotation` quarter turns with its floor on the block
    /// that `player`, whose feet are at `feet`, looks at, or else where they stand, centred on
    /// it as generation centres one on its spot, and selects it. Cells it leaves as the world has
    /// them become structure voids and its markers marker blocks, so saving it again gives the
    /// same template back.
    pub(super) fn place(
        &mut self,
        world: &mut World,
        dir: Option<&Path>,
        player: Entity,
        feet: Vec3,
        name: &str,
        rotation: u8,
    ) -> Result<String, String> {
        let structure = load(dir, name)?;
        let template = &structure.template;
        let (&look, _) = self.players.get(player).unwrap();
        let ground = looked_at(world, feet, look)
            .map_or(feet.floor().as_i64vec3(), |hit| hit.block + I64Vec3::Y);
        let footprint = template.footprint(rotation);
        let size = I64Vec3::new(footprint.x, template.size.y, footprint.y);
        let min = ground - I64Vec3::new(footprint.x / 2, template.floor, footprint.y / 2);
        let marked: Vec<(I64Vec3, BlockType)> = template
            .markers
            .iter()
            .filter_map(|(kind, at)| {
                let (block, _) = MARKERS.iter().find(|(_, marker)| marker == kind)?;
                Some((template.turn(rotation, *at), *block))
            })
            .collect();
        for y in 0..size.y {
            for z in 0..size.z {
                for x in 0..size.x {
                    let at = I64Vec3::new(x, y, z);
                    let block = match marked.iter().find(|(marker, _)| *marker == at) {
                        Some(&(_, marker)) => marker,
                        None => template
                            .cell(rotation, at)
                            .unwrap_or(BlockType::StructureVoid),
                    };
                    self.edit(world, min + at, block);
                }
            }
        }
        let (_, mut selection) = self.players.get_mut(player).unwrap();
        *selection = Selection {
            corners: [Some(min), Some(min + size - 1)],
            floor: Some(ground.y),
        };
        Ok(format!(
            "Placed {name}, {}, from {}, turned {} degrees, and selected it",
            dimensions(size),
            coordinates(min),
            u32::from(rotation) * 90
        ))
    }

    // Changes `at` to `to` as a player's edit does, telling the clients near it, and letting go
    // of what was there: a container leaves what was in it, as items are never lost with it.
    fn edit(&mut self, world: &mut World, at: I64Vec3, to: BlockType) {
        if !world.edit(at, to) {
            return;
        }
        self.changed.write(BlockChanged {
            block: at,
            to,
            by: None,
        });
        let held = self.containers.remove(at).into_iter().flat_map(|c| c.slots);
        for stack in held.flatten() {
            self.commands
                .spawn((DroppedItem::from_block(stack, at), Replicated));
        }
        self.crops.remove(at);
    }
}

/// The structures there are to place: those built in and those saved in the world's
/// directory `dir`.
pub(super) fn list(dir: Option<&Path>) -> String {
    let built_in: Vec<&str> = STRUCTURES.iter().map(|s| s.name.as_str()).collect();
    let mut saved: Vec<String> = dir
        .and_then(|dir| fs::read_dir(dir.join(STRUCTURES_DIR)).ok())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "toml" {
                return None;
            }
            Some(path.file_stem()?.to_str()?.to_owned())
        })
        .collect();
    saved.sort_unstable();
    let saved = if saved.is_empty() {
        "none yet".into()
    } else {
        saved.join(", ")
    };
    format!(
        "Built in: {}\nSaved in this world: {saved}",
        built_in.join(", ")
    )
}

/// Tells each client whose player's selection has changed what it is now, to draw.
pub(super) fn send_selections(
    players: Query<(&PlayerOwner, &Selection), Changed<Selection>>,
    mut changes: MessageWriter<ToClients<SelectionChanged>>,
) {
    for (owner, selection) in &players {
        if owner.0 != ClientId::Server {
            changes.write(ToClients {
                targets: SendTargets::Single(owner.0),
                message: SelectionChanged(*selection),
            });
        }
    }
}

// The structure called `name`: the one saved in the world's directory `dir` if there is one,
// so a built-in one can be reworked there, or else the built-in one.
fn load(dir: Option<&Path>, name: &str) -> Result<Cow<'static, Structure>, String> {
    if let Some(path) = dir.map(|dir| dir.join(STRUCTURES_DIR).join(format!("{name}.toml"))) {
        if let Ok(text) = fs::read_to_string(&path) {
            return toml::from_str(&text)
                .map(Cow::Owned)
                .map_err(|e| format!("{} won't load: {}", path.display(), e.message()));
        }
    }
    STRUCTURES
        .iter()
        .find(|structure| structure.name == name)
        .map(Cow::Borrowed)
        .ok_or_else(|| format!("There's no structure called {name}, see /structure list"))
}

// The commonest block among `cells` other than air, if there's one.
fn commonest(cells: &[Option<BlockType>]) -> Option<BlockType> {
    let mut counts = [0; BlockType::ALL.len()];
    for &block in cells.iter().flatten() {
        counts[block as usize] += 1;
    }
    BlockType::ALL
        .into_iter()
        .filter(|&block| block != BlockType::Air && counts[block as usize] > 0)
        .max_by_key(|&block| counts[block as usize])
}

fn coordinates(block: I64Vec3) -> String {
    format!("{}, {}, {}", block.x, block.y, block.z)
}

fn dimensions(size: I64Vec3) -> String {
    format!("{} by {} by {}", size.x, size.y, size.z)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use bevy::ecs::message::Messages;
    use bevy::math::I64Vec3;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::STRUCTURES_DIR;
    use crate::authoring::Selection;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::chunks::generate::structures::{Placement, Structure, STRUCTURES};
    use crate::game_mode::GameMode;
    use crate::player::name::PlayerName;
    use crate::player::{Player, PlayerLook, PlayerOwner};
    use crate::protocol::{CommandOutput, ProtocolPlugin, RunCommand};
    use crate::save::WorldSave;
    use crate::server::{Operators, ServerPlugin};
    use crate::world::World;

    // Standing on a floor of stone at y 0, over the middle of the chunks loaded, from -32 to 31
    // across and 0 to 31 up, looking straight down at the block under their feet.
    const FEET: Vec3 = Vec3::new(0.5, 1.0, 0.5);
    const DOWN: PlayerLook = PlayerLook {
        yaw: 0.0,
        pitch: -1.5,
    };

    // A server alone, as a dedicated one runs, with its world saved in a directory of its own,
    // removed afterwards, and its host's player in creative.
    struct Server {
        app: App,
        dir: PathBuf,
        host: Entity,
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Server {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rustcraft-test-{}-structures-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            let (save, ..) = WorldSave::open(dir.clone(), Some(3)).unwrap();
            let mut world = World::new(16);
            for x in -2..=1 {
                for y in 0..=1 {
                    for z in -2..=1 {
                        let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                        world.insert_chunk(coord, ChunkData::default());
                    }
                }
            }
            for x in -16..16 {
                for z in -16..16 {
                    world.edit(I64Vec3::new(x, 0, z), BlockType::Stone);
                }
            }
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                StatesPlugin,
                RepliconPlugins,
                ProtocolPlugin,
                ServerPlugin,
            ))
            .insert_resource(world)
            .insert_resource(save)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            let host = app
                .world_mut()
                .spawn((
                    Player::default(),
                    PlayerOwner(ClientId::Server),
                    PlayerName::new("host"),
                    GameMode::Creative,
                    Transform::from_translation(FEET),
                    DOWN,
                ))
                .id();
            app.finish();
            Self { app, dir, host }
        }

        // Runs `line` as the client's.
        fn send(&mut self, client: ClientId, line: &str) {
            self.app.world_mut().write_message(FromClient {
                client_id: client,
                message: RunCommand(line.into()),
            });
            self.app.update();
        }

        // Runs `line` as the host's, returning what it answered.
        fn run(&mut self, line: &str) -> Result<String, String> {
            self.send(ClientId::Server, line);
            let mut output = self
                .app
                .world_mut()
                .resource_mut::<Messages<CommandOutput>>();
            let CommandOutput { text, error } = output.drain().last().expect("no answer");
            if error {
                Err(text)
            } else {
                Ok(text)
            }
        }

        fn block_at(&self, at: I64Vec3) -> BlockType {
            self.app.world().resource::<World>().get_block_at(at)
        }

        fn edit(&mut self, at: I64Vec3, to: BlockType) {
            self.app.world_mut().resource_mut::<World>().edit(at, to);
        }

        fn selection(&self) -> Selection {
            *self.app.world().get::<Selection>(self.host).unwrap()
        }

        fn saved(&self, name: &str) -> Structure {
            let path = self.dir.join(STRUCTURES_DIR).join(format!("{name}.toml"));
            toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
        }
    }

    fn watchtower() -> &'static Structure {
        STRUCTURES.iter().find(|s| s.name == "watchtower").unwrap()
    }

    #[test]
    fn test_corners_are_set_where_typed_looked_at_or_stood_in_and_say_what_they_select() {
        let mut server = Server::new("corners");
        assert_eq!(
            Ok("Corner 1 at 1, 2, 3; mark the other with /pos2".into()),
            server.run("/pos1 1 ~1 3.5")
        );
        assert_eq!(
            Ok("Corner 2 at 0, 0, 0, selecting 2 by 3 by 4".into()),
            server.run("/pos2")
        );
        let corners = [Some(I64Vec3::new(1, 2, 3)), Some(I64Vec3::ZERO)];
        assert_eq!(corners, server.selection().corners);
        // looking up at nothing, the block stood in
        let host = server.host;
        server.app.world_mut().entity_mut(host).insert(PlayerLook {
            yaw: 0.0,
            pitch: 1.5,
        });
        server.run("/pos2").unwrap();
        assert_eq!(Some(I64Vec3::new(0, 1, 0)), server.selection().corners[1]);
    }

    #[test]
    fn test_a_box_too_big_or_not_all_loaded_is_refused() {
        let mut server = Server::new("too-big");
        assert!(server
            .run("/structure save hut")
            .unwrap_err()
            .contains("Select a box"));
        server.run("/pos1 -20 0 -20").unwrap();
        let too_big = server.run("/pos2 44 5 0").unwrap();
        assert!(
            too_big.contains("65 by 6 by 21, too big to save"),
            "{too_big}"
        );
        let refused = server.run("/structure save hut").unwrap_err();
        assert!(refused.contains("at most 64 by 31 by 64"), "{refused}");
        server.run("/pos2 ~ 31 ~").unwrap();
        let refused = server.run("/structure save hut").unwrap_err();
        assert!(refused.contains("at most"), "{refused}");
        // one wide enough, but reaching out of the chunks loaded
        server.run("/pos2 40 5 0").unwrap();
        let refused = server.run("/structure save hut").unwrap_err();
        assert!(refused.contains("isn't loaded"), "{refused}");
        // and a floor deeper than structures sink isn't saved
        server.run("/pos2 0 5 0").unwrap();
        let refused = server.run("/structure save hut 5").unwrap_err();
        assert!(refused.contains("wouldn't load"), "{refused}");
    }

    #[test]
    fn test_saving_writes_a_template_with_voids_left_alone_and_markers_marked() {
        let mut server = Server::new("save");
        server.edit(I64Vec3::new(2, 1, 2), BlockType::StructureVoid);
        server.edit(I64Vec3::new(3, 1, 3), BlockType::LootMarker);
        server.edit(I64Vec3::new(4, 1, 4), BlockType::Cobblestone);
        server.run("/pos1 4 2 4").unwrap();
        server.run("/pos2 2 0 2").unwrap();
        let saved = server.run("/structure save hut 1").unwrap();
        assert!(saved.starts_with("Saved hut, 3 by 3 by 3 with its floor on layer 1, to"));
        assert!(saved.ends_with("hut.toml"), "{saved}");

        let hut = server.saved("hut");
        assert_eq!("hut", hut.name);
        assert_eq!(Placement::default(), hut.placement);
        let template = &hut.template;
        assert_eq!((I64Vec3::splat(3), 1), (template.size, template.floor));
        // what most of its bottom layer is
        assert_eq!(BlockType::Stone, template.foundation);
        let cell = |x, y, z| template.cell(0, I64Vec3::new(x, y, z));
        assert_eq!(Some(BlockType::Stone), cell(1, 0, 1));
        assert_eq!(None, cell(0, 1, 0));
        assert_eq!(Some(BlockType::Cobblestone), cell(2, 1, 2));
        assert_eq!(Some(BlockType::Air), cell(0, 2, 2));
        let loot = vec![("loot".to_string(), I64Vec3::new(1, 1, 1))];
        assert_eq!(loot, template.markers);

        let list = server.run("/structure list").unwrap();
        assert_eq!("Built in: watchtower\nSaved in this world: hut", list);
    }

    #[test]
    fn test_placing_builds_a_structure_turned_on_the_block_looked_at_and_selects_it() {
        let mut server = Server::new("place");
        let placed = server.run("/structure place watchtower 90").unwrap();
        assert_eq!(
            "Placed watchtower, 7 by 20 by 7, from -3, 0, -3, turned 90 degrees, and selected it",
            placed
        );
        let template = &watchtower().template;
        let (_, loot) = template.markers[0];
        let min = I64Vec3::new(-3, 0, -3);
        for y in 0..20 {
            for z in 0..7 {
                for x in 0..7 {
                    let at = I64Vec3::new(x, y, z);
                    let wanted = if at == template.turn(1, loot) {
                        BlockType::LootMarker
                    } else {
                        template.cell(1, at).unwrap_or(BlockType::StructureVoid)
                    };
                    assert_eq!(wanted, server.block_at(min + at), "{at}");
                }
            }
        }
        // its floor on the block looked at, over its footing sunk into it
        assert_eq!(
            Selection {
                corners: [Some(min), Some(I64Vec3::new(3, 19, 3))],
                floor: Some(1),
            },
            server.selection()
        );
        assert!(server
            .run("/structure place castle")
            .unwrap_err()
            .contains("no structure called castle"));
    }

    #[test]
    fn test_the_watchtower_placed_and_saved_again_is_the_same_turned_or_not() {
        let mut server = Server::new("round-trip");
        server.run("/structure place watchtower").unwrap();
        server.run("/structure save watchtower").unwrap();
        // placement and all, as it's kept by name
        assert_eq!(*watchtower(), server.saved("watchtower"));

        server.run("/structure place watchtower 270").unwrap();
        server.run("/structure save turned").unwrap();
        let (turned, template) = (server.saved("turned").template, &watchtower().template);
        assert_eq!((template.size.zyx(), 1), (turned.size, turned.floor));
        for y in 0..20 {
            for z in 0..7 {
                for x in 0..7 {
                    let at = I64Vec3::new(x, y, z);
                    assert_eq!(template.cell(3, at), turned.cell(0, at), "{at}");
                }
            }
        }
        let (kind, loot) = &template.markers[0];
        assert_eq!(
            vec![(kind.clone(), template.turn(3, *loot))],
            turned.markers
        );
    }

    #[test]
    fn test_only_operators_author_structures() {
        let mut server = Server::new("operators");
        let client = ClientId::Client(server.app.world_mut().spawn_empty().id());
        let guest = server
            .app
            .world_mut()
            .spawn((
                Player::default(),
                PlayerOwner(client),
                PlayerName::new("guest"),
                Transform::from_translation(FEET),
                DOWN,
            ))
            .id();
        let selection = |server: &Server| *server.app.world().get::<Selection>(guest).unwrap();
        for line in ["/pos1", "/pos2 1 2 3", "/structure place watchtower"] {
            server.send(client, line);
        }
        assert_eq!(Selection::default(), selection(&server));
        assert_eq!(BlockType::Stone, server.block_at(I64Vec3::ZERO));
        server
            .app
            .insert_resource(Operators::new(vec!["guest".into()]));
        server.send(client, "/pos1 1 2 3");
        assert_eq!(Some(I64Vec3::new(1, 2, 3)), selection(&server).corners[0]);
        server.send(client, "/structure place watchtower");
        assert_eq!(BlockType::Cobblestone, server.block_at(I64Vec3::ZERO));
    }
}
