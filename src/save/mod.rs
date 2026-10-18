mod region;
mod world_file;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::{App, AppExit, Last, OnAppExitSystems, Plugin};
use bevy::ecs::{
    message::MessageReader,
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Query, Res, ResMut, SystemParam},
};
use bevy::log::{error, info};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::tasks::{block_on, futures, IoTaskPool, Task};
use bevy::time::{Real, Time};
use bevy::transform::components::Transform;
use bevy_replicon::prelude::ClientId;

use crate::clock::{TimeOfDay, START_TIME};
use crate::edits::Edits;
use crate::game_mode::GameMode;
use crate::hotbar::Hotbar;
use crate::player::{Player, PlayerLook, PlayerOwner};
use crate::world::World;
use region::{ChunkEdits, Region};
pub use world_file::{PlayerState, WorldFile};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);
const WORLD_FILE: &str = "world.toml";
const EDITS_DIR: &str = "edits";

/// Saves the world (§4.3): its edits and `world.toml`, every so often if they've changed, and
/// as the game quits.
pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Last,
            (autosave, save_on_exit.in_set(OnAppExitSystems)).chain(),
        );
    }
}

/// Where worlds are saved, a directory each: under the platform's data directory, such as
/// `~/.local/share/rustcraft/worlds` on Linux.
pub fn worlds_dir() -> PathBuf {
    dirs::data_dir()
        .expect("there's no data directory to save worlds in")
        .join("rustcraft")
        .join("worlds")
}

/// The name of the world to play: `RC_WORLD`, or `world`.
pub fn world_name() -> String {
    std::env::var("RC_WORLD").unwrap_or_else(|_| "world".into())
}

/// A world's directory, and what's still to be written to it.
#[derive(Resource)]
pub struct WorldSave {
    dir: PathBuf,
    // regions edited since they were last written
    unsaved: HashSet<Region>,
    // what `world.toml` was last written with, so it's only written again when it changes
    written: Option<WorldFile>,
    writing: Option<Writing>,
    next_autosave: Duration,
}

// A save being written in the background, and the regions in it, to write again if it fails.
struct Writing {
    task: Task<io::Result<()>>,
    regions: Vec<Region>,
}

/// What's changed since the last save, copied from the game to be written apart from it.
struct Snapshot {
    dir: PathBuf,
    world_file: Option<WorldFile>,
    regions: HashMap<Region, Vec<ChunkEdits>>,
}

impl WorldSave {
    /// Opens the world saved in `dir`, or makes a new one there, with `requested_seed` or else a
    /// random one. Returns it with its `world.toml` and every edit made in it.
    ///
    /// All the edits are loaded at once, which is quick while worlds are small. A world with a
    /// great many would load its regions as chunks near them load instead.
    pub fn open(dir: PathBuf, requested_seed: Option<u32>) -> io::Result<(Self, WorldFile, Edits)> {
        let path = dir.join(WORLD_FILE);
        let file = match fs::read_to_string(&path) {
            Ok(text) => {
                let file = WorldFile::from_toml(&text)?;
                if let Some(seed) = requested_seed.filter(|&seed| seed != file.seed) {
                    info!(
                        "ignoring RC_SEED {seed}, as this world has seed {}",
                        file.seed
                    );
                }
                file
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let file = WorldFile::new(requested_seed.unwrap_or_else(rand::random), START_TIME);
                fs::create_dir_all(&dir)?;
                // straight away, so the world keeps its seed whatever happens next
                write_atomically(&path, file.to_toml().as_bytes())?;
                info!("made a new world in {}", dir.display());
                file
            }
            Err(error) => return Err(error),
        };

        let mut edits = Edits::default();
        match fs::read_dir(dir.join(EDITS_DIR)) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if path.extension() == Some(region::EXTENSION.as_ref()) {
                        region::decode(&fs::read(&path)?, &mut edits).map_err(|error| {
                            io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                        })?;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let save = Self {
            dir,
            unsaved: HashSet::new(),
            written: Some(file.clone()),
            writing: None,
            next_autosave: AUTOSAVE_INTERVAL,
        };
        Ok((save, file, edits))
    }

    // Takes what's changed since the last snapshot: the regions with new edits, all of whose
    // edits are copied to be written again, and `file` if it's not what was last written.
    fn snapshot(&mut self, edits: &mut Edits, file: WorldFile) -> Snapshot {
        let changed = edits.take_changed().into_iter().map(Region::containing);
        self.unsaved.extend(changed);
        let mut regions: HashMap<_, _> = self.unsaved.drain().map(|r| (r, Vec::new())).collect();
        if !regions.is_empty() {
            for coord in edits.chunks() {
                if let Some(chunks) = regions.get_mut(&Region::containing(coord)) {
                    chunks.push((coord, edits.in_chunk(coord).collect()));
                }
            }
        }
        let world_file = (self.written.as_ref() != Some(&file)).then(|| file.clone());
        self.written = Some(file);
        Snapshot {
            dir: self.dir.clone(),
            world_file,
            regions,
        }
    }

    // A failed save leaves what it was writing to be written next time.
    fn finished(&mut self, result: io::Result<()>, regions: Vec<Region>) {
        if let Err(error) = result {
            error!("couldn't save the world in {}: {error}", self.dir.display());
            self.unsaved.extend(regions);
            self.written = None;
        }
    }
}

impl Snapshot {
    fn is_empty(&self) -> bool {
        self.world_file.is_none() && self.regions.is_empty()
    }

    fn write(self) -> io::Result<()> {
        if !self.regions.is_empty() {
            let edits_dir = self.dir.join(EDITS_DIR);
            fs::create_dir_all(&edits_dir)?;
            for (region, chunks) in self.regions {
                write_atomically(&edits_dir.join(region.file_name()), &region::encode(chunks))?;
            }
        }
        if let Some(file) = self.world_file {
            write_atomically(&self.dir.join(WORLD_FILE), file.to_toml().as_bytes())?;
        }
        Ok(())
    }
}

// Written beside it and renamed over it, so a crash part way through leaves the old file whole.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_added_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    // on disk before it takes the name, or a crash could leave the name on an empty file
    file.sync_all()?;
    fs::rename(&temporary, path)
}

/// The game state that's saved.
#[derive(SystemParam)]
struct Savable<'w, 's> {
    world: ResMut<'w, World>,
    clock: Res<'w, TimeOfDay>,
    players: Query<
        'w,
        's,
        (
            &'static Transform,
            &'static Player,
            &'static PlayerLook,
            &'static GameMode,
            &'static Hotbar,
            &'static PlayerOwner,
        ),
    >,
}

impl Savable<'_, '_> {
    fn snapshot(&mut self, save: &mut WorldSave) -> Snapshot {
        let mut file = WorldFile::new(self.world.seed(), self.clock.0);
        // the host's, until M0.6 saves each player's
        file.player = self
            .players
            .iter()
            .find(|(.., owner)| owner.0 == ClientId::Server)
            .map(|(body, player, look, mode, hotbar, _)| {
                PlayerState::of(body, *look, player.movement, *mode, *hotbar)
            });
        save.snapshot(self.world.edits_mut(), file)
    }
}

// Every so often, writes what's changed on the IO pool, one save at a time so they land in order.
fn autosave(time: Res<Time<Real>>, mut save: ResMut<WorldSave>, mut savable: Savable) {
    if let Some(writing) = &mut save.writing {
        let Some(result) = futures::check_ready(&mut writing.task) else {
            return;
        };
        let regions = save.writing.take().unwrap().regions;
        save.finished(result, regions);
    }
    if time.elapsed() < save.next_autosave {
        return;
    }
    save.next_autosave = time.elapsed() + AUTOSAVE_INTERVAL;
    let snapshot = savable.snapshot(&mut save);
    if snapshot.is_empty() {
        return;
    }
    let regions = snapshot.regions.keys().copied().collect();
    let task = IoTaskPool::get().spawn(async move { snapshot.write() });
    save.writing = Some(Writing { task, regions });
}

// The last save is written here on the main thread, after any still being written, rather than
// on the IO pool, whose threads end with the process: the app only exits once this returns.
fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    mut save: ResMut<WorldSave>,
    mut savable: Savable,
) {
    if exits.read().count() == 0 {
        return;
    }
    if let Some(writing) = save.writing.take() {
        save.finished(block_on(writing.task), writing.regions);
    }
    let snapshot = savable.snapshot(&mut save);
    let regions = snapshot.regions.keys().copied().collect();
    let result = snapshot.write();
    if result.is_ok() {
        info!("saved the world in {}", save.dir.display());
    }
    save.finished(result, regions);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use bevy::math::{I64Vec3, U16Vec3, Vec3};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::{region, Region, SavePlugin, WorldFile, WorldSave, EDITS_DIR, WORLD_FILE};
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE};
    use crate::clock::TimeOfDay;
    use crate::edits::Edits;
    use crate::game_mode::GameMode;
    use crate::hotbar::Hotbar;
    use crate::player::{Movement, Player, PlayerLook};
    use crate::protocol::ProtocolPlugin;
    use crate::server::{ServerPlugin, WorldToOpen};

    // A directory of its own under the system's temporary one, removed afterwards.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("rustcraft-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn world(&self) -> PathBuf {
            self.0.join("world")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn region_file(dir: &TempDir, region: Region) -> PathBuf {
        dir.world().join(EDITS_DIR).join(region.file_name())
    }

    #[test]
    fn test_a_new_world_takes_the_seed_asked_for_and_keeps_it() {
        let dir = TempDir::new("seed");
        let (_, file, edits) = WorldSave::open(dir.world(), Some(7)).unwrap();
        assert_eq!(7, file.seed);
        assert!(edits.chunks().next().is_none());
        // a never edited world is just its world.toml
        let names: Vec<_> = fs::read_dir(dir.world())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(vec![WORLD_FILE], names);
        assert!(fs::metadata(dir.world().join(WORLD_FILE)).unwrap().len() < 100);

        for asked in [Some(8), None] {
            let (_, file, _) = WorldSave::open(dir.world(), asked).unwrap();
            assert_eq!(7, file.seed);
        }

        let random = TempDir::new("random-seed");
        let (_, file, _) = WorldSave::open(random.world(), None).unwrap();
        assert_eq!(
            file.seed,
            WorldSave::open(random.world(), None).unwrap().1.seed
        );
    }

    #[test]
    fn test_a_world_from_a_newer_version_is_refused() {
        let dir = TempDir::new("newer");
        let mut file = WorldFile::new(1, 0.4);
        file.format += 1;
        fs::create_dir_all(dir.world()).unwrap();
        fs::write(dir.world().join(WORLD_FILE), file.to_toml()).unwrap();
        let Err(error) = WorldSave::open(dir.world(), None) else {
            panic!("opened a newer world");
        };
        assert!(error.to_string().contains("newer version"), "{error}");
    }

    #[test]
    fn test_only_regions_with_new_edits_are_written() {
        let dir = TempDir::new("regions");
        let (mut save, file, mut edits) = WorldSave::open(dir.world(), Some(1)).unwrap();
        let (here, there) = (I64Vec3::new(5, 500, 5), I64Vec3::new(-600, 470, 9000));
        edits.set(here, BlockType::Air);
        edits.set(there, BlockType::Bricks);
        let snapshot = save.snapshot(&mut edits, file.clone());
        // world.toml hasn't changed since it was written
        assert!(snapshot.world_file.is_none());
        assert_eq!(2, snapshot.regions.len());
        snapshot.write().unwrap();
        assert!(save.snapshot(&mut edits, file.clone()).is_empty());

        let (near, far) = (
            Region::containing(ChunkCoordinate::containing(here)),
            Region::containing(ChunkCoordinate::containing(there)),
        );
        fs::write(region_file(&dir, far), "left alone").unwrap();
        edits.set(here + I64Vec3::X, BlockType::Planks);
        let snapshot = save.snapshot(&mut edits, file);
        assert_eq!(vec![&near], snapshot.regions.keys().collect::<Vec<_>>());
        snapshot.write().unwrap();
        assert_eq!(
            "left alone",
            fs::read_to_string(region_file(&dir, far)).unwrap()
        );

        // the rewritten region keeps its old edits as well as the new
        let mut loaded = Edits::default();
        region::decode(&fs::read(region_file(&dir, near)).unwrap(), &mut loaded).unwrap();
        let chunk = ChunkCoordinate::containing(here);
        let mut blocks: Vec<_> = loaded.in_chunk(chunk).collect();
        blocks.sort_unstable_by_key(|(local, _)| local.x);
        let local = ChunkCoordinate::local_block(here);
        assert_eq!(
            vec![
                (local, BlockType::Air),
                (local + U16Vec3::X, BlockType::Planks)
            ],
            blocks
        );
    }

    // The game's saving, headless: a world played in one app is there in the next. The
    // server opens it and spawns its player, with no camera, as a server's players have none.
    fn play(dir: &TempDir) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
            SavePlugin,
        ))
        .insert_resource(WorldToOpen {
            dir: dir.world(),
            seed: None,
            time_of_day: None,
            region_distance: 16,
        });
        app.finish();
        app.update();
        app
    }

    fn player(app: &mut App) -> (Transform, PlayerLook, Movement) {
        let (transform, look, player) = app
            .world_mut()
            .query::<(&Transform, &PlayerLook, &Player)>()
            .single(app.world())
            .unwrap();
        (*transform, *look, player.movement)
    }

    #[test]
    fn test_edits_and_the_player_are_there_after_a_restart() {
        let dir = TempDir::new("restart");
        let built = I64Vec3::new(3, 460, -7);
        let mut app = play(&dir);
        // no time passes, so the clock stays where it's set
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        let at = Vec3::new(100.0, 480.0, -50.0);
        let looking = PlayerLook {
            yaw: 4.0,
            pitch: -0.5,
        };
        {
            let world = app.world_mut();
            let mut game_world = world.resource_mut::<crate::world::World>();
            assert!(game_world.edit(built, BlockType::Furnace));
            world.resource_mut::<TimeOfDay>().0 = 3.5;
            let mut players = world.query::<(
                &mut Transform,
                &mut PlayerLook,
                &mut Player,
                &mut GameMode,
                &mut Hotbar,
            )>();
            let (mut transform, mut look, mut player, mut mode, mut hotbar) =
                players.single_mut(world).unwrap();
            transform.translation = at;
            *look = looking;
            player.movement = Movement::Walk;
            *mode = GameMode::Survival;
            hotbar.slots[2] = Some(BlockType::IronOre);
            hotbar.selected = 2;
        }
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);

        let mut app = play(&dir);
        let (transform, look, movement) = player(&mut app);
        assert_eq!(
            (at, looking, Movement::Walk),
            (transform.translation, look, movement)
        );
        assert_eq!(3.5, app.world().resource::<TimeOfDay>().0);
        let (mode, hotbar) = app
            .world_mut()
            .query::<(&GameMode, &Hotbar)>()
            .single(app.world())
            .unwrap();
        assert_eq!(GameMode::Survival, *mode);
        assert_eq!(Some(BlockType::IronOre), hotbar.held());
        let mut world = app.world_mut().resource_mut::<crate::world::World>();
        world.insert_chunk(ChunkCoordinate::containing(built), ChunkData::default());
        assert_eq!(BlockType::Furnace, world.get_block_at(built));
        assert_eq!(BlockType::Air, world.get_block_at(built + I64Vec3::Y));
    }

    #[test]
    fn test_autosaves_write_in_the_background() {
        let dir = TempDir::new("autosave");
        let mut app = play(&dir);
        app.world_mut().resource_mut::<WorldSave>().next_autosave = Duration::ZERO;
        let built = I64Vec3::new(0, 450, CHUNK_SIZE as i64 * 40);
        app.world_mut()
            .resource_mut::<crate::world::World>()
            .edit(built, BlockType::Cobblestone);
        app.update();
        let region = Region::containing(ChunkCoordinate::containing(built));
        while app.world().resource::<WorldSave>().writing.is_some() {
            app.update();
        }
        assert!(region_file(&dir, region).exists());
        let (_, file, edits) = WorldSave::open(dir.world(), None).unwrap();
        assert!(file.player.is_some());
        assert_eq!(
            1,
            edits.in_chunk(ChunkCoordinate::containing(built)).count()
        );
    }
}
