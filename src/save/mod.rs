mod creatures;
pub(crate) mod region;
mod world_file;
pub mod worlds;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::{App, AppExit, Last, OnAppExitSystems, Plugin};
use bevy::ecs::{
    message::MessageReader,
    resource::Resource,
    schedule::{common_conditions::resource_exists, IntoScheduleConfigs},
    system::{Commands, Query, Res, ResMut, SystemParam},
};
use bevy::log::{error, info, warn};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::tasks::{block_on, futures, IoTaskPool, Task};
use bevy::time::{Real, Time};
use bevy::transform::components::Transform;
use bevy_replicon::prelude::ClientId;

use crate::chunks::chunk::ChunkCoordinate;
use crate::clock::TimeOfDay;
use crate::containers::Containers;
use crate::creatures::{Creature, SavedCreature};
use crate::edits::Edits;
use crate::farming::Crops;
use crate::game_mode::GameMode;
use crate::inventory::Hotbar;
use crate::inventory::Inventory;
use crate::player::name::PlayerName;
use crate::player::{Player, PlayerLook, PlayerOwner};
use crate::server::creatures::{saved, Dormant, Health, Life, Settled, Townsfolk};
use crate::server::Operators;
use crate::skin::Skin;
use crate::survival::{SpawnBed, Vitals};
use crate::world::{order_leaving, LeaveWorld, LeaveWorldSystems, World};
use region::{ChunkEdits, Region};
pub use world_file::{PlayerState, SavedSkin, WorldFile};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);
const WORLD_FILE: &str = "world.toml";
const EDITS_DIR: &str = "edits";
const PLAYERS_DIR: &str = "players";
const SKINS_DIR: &str = "skins";

/// Saves the world (§4.3): its edits, containers and crops, `world.toml` with the host's player,
/// each player who's joined from elsewhere as `players/<name>.toml`, the skins they wear in
/// `skins/`, and its creatures by region in `creatures/`, every so often if they've changed, as such a player leaves, and as the game
/// quits.
pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        order_leaving(app);
        // with a world open, which the game's menus are without
        app.add_systems(
            Last,
            (autosave, save_on_exit.in_set(OnAppExitSystems))
                .chain()
                .run_if(resource_exists::<WorldSave>),
        )
        .add_systems(
            LeaveWorld,
            save_on_leaving
                .in_set(LeaveWorldSystems::Save)
                .run_if(resource_exists::<WorldSave>),
        );
    }
}

/// Where the game keeps what it saves, under the platform's data directory, such as
/// `~/.local/share/rustcraft` on Linux.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .expect("there's no data directory to save in")
        .join("rustcraft")
}

/// Where worlds are saved, a directory each.
pub fn worlds_dir() -> PathBuf {
    data_dir().join("worlds")
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
    // the host's player as it was opened, kept as it is by a dedicated server, which has none
    host: Option<PlayerState>,
    // see `WorldFile::new_player_mode`
    new_player_mode: GameMode,
    // the players who've joined from elsewhere, by name, as they were saved or as they left
    players: HashMap<String, PlayerState>,
    // those of them changed since they were last written
    unsaved_players: HashSet<String>,
    // the skins players wear, by their files' names, and those not yet written
    skins: HashMap<String, Skin>,
    unsaved_skins: HashSet<String>,
    // the creatures read as the world opened, until the server takes them
    opened_creatures: Vec<SavedCreature>,
    // each region's creatures as they were last written, and the regions to write whatever
    // they hold, as writing them failed
    written_creatures: HashMap<Region, Vec<SavedCreature>>,
    unsaved_creatures: HashSet<Region>,
    writing: Option<Writing>,
    next_autosave: Duration,
}

// A save being written in the background, and the regions and players in it, to write again if
// it fails.
struct Writing {
    task: Task<io::Result<()>>,
    contents: Contents,
}

// What a save writes: the regions of edits, the players, the regions of creatures and the skins.
type Contents = (Vec<Region>, Vec<String>, Vec<Region>, Vec<String>);

/// What's changed since the last save, copied from the game to be written apart from it.
struct Snapshot {
    dir: PathBuf,
    world_file: Option<WorldFile>,
    regions: HashMap<Region, Vec<ChunkEdits>>,
    players: Vec<(String, PlayerState)>,
    // each region's creatures, none to remove its file
    creatures: Vec<(Region, Vec<SavedCreature>)>,
    skins: Vec<(String, Skin)>,
}

impl WorldSave {
    /// Opens the world saved in `dir`, or makes a new one there, with `requested_seed` or else a
    /// random one. Returns it with its `world.toml`, every edit made in it, every container and
    /// every crop.
    ///
    /// All the edits are loaded at once, which is quick while worlds are small. A world with a
    /// great many would load its regions as chunks near them load instead. So are the saves
    /// of the players who've joined it, so none is read as they join.
    pub fn open(
        dir: PathBuf,
        requested_seed: Option<u32>,
    ) -> io::Result<(Self, WorldFile, Edits, Containers, Crops)> {
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
                let file = WorldFile::new(
                    requested_seed.unwrap_or_else(rand::random),
                    TimeOfDay::default(),
                );
                fs::create_dir_all(&dir)?;
                // straight away, so the world keeps its seed whatever happens next
                write_atomically(&path, file.to_toml().as_bytes())?;
                info!("made a new world in {}", dir.display());
                file
            }
            Err(error) => return Err(error),
        };

        let (mut edits, mut containers, mut crops) =
            (Edits::default(), Containers::default(), Crops::default());
        match fs::read_dir(dir.join(EDITS_DIR)) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if path.extension() == Some(region::EXTENSION.as_ref()) {
                        let bytes = fs::read(&path)?;
                        region::decode(&bytes, &mut edits, &mut containers, &mut crops).map_err(
                            |error| {
                                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                            },
                        )?;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mut players = HashMap::new();
        match fs::read_dir(dir.join(PLAYERS_DIR)) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    let name = path.file_stem().and_then(|name| name.to_str());
                    // only those a name could have written
                    let Some(name) = name.filter(|&name| PlayerName::new(name).as_str() == name)
                    else {
                        continue;
                    };
                    if path.extension() == Some("toml".as_ref()) {
                        let state = toml::from_str(&fs::read_to_string(&path)?).map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("{}: {e}", path.display()),
                            )
                        })?;
                        players.insert(name.to_owned(), state);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mut written_creatures: HashMap<Region, Vec<SavedCreature>> = HashMap::new();
        match fs::read_dir(dir.join(creatures::DIR)) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    let region = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(Region::from_file_name);
                    if path.extension() != Some(creatures::EXTENSION.as_ref()) {
                        continue;
                    }
                    let Some(region) = region else {
                        continue;
                    };
                    let saved = creatures::decode(&fs::read(&path)?).map_err(|error| {
                        io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                    })?;
                    written_creatures.insert(region, saved);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        // the skins of those who wear their own, any that can't be read left off
        let mut skins = HashMap::new();
        for saved in players
            .values()
            .chain(&file.player)
            .filter_map(|p| p.skin.as_ref())
        {
            if !saved.is_named_by_hash() || skins.contains_key(&saved.file) {
                continue;
            }
            let path = dir.join(SKINS_DIR).join(&saved.file);
            match fs::read(&path).map(|png| Skin::new(png, saved.arms)) {
                Ok(Ok(skin)) => {
                    skins.insert(saved.file.clone(), skin);
                }
                Ok(Err(error)) => warn!("{} isn't a skin: {error}", path.display()),
                Err(error) => warn!("couldn't read the skin {}: {error}", path.display()),
            }
        }

        let save = Self {
            dir,
            unsaved: HashSet::new(),
            written: Some(file.clone()),
            host: file.player.clone(),
            new_player_mode: file.new_player_mode,
            players,
            unsaved_players: HashSet::new(),
            skins,
            unsaved_skins: HashSet::new(),
            opened_creatures: written_creatures.values().flatten().copied().collect(),
            written_creatures,
            unsaved_creatures: HashSet::new(),
            writing: None,
            next_autosave: AUTOSAVE_INTERVAL,
        };
        Ok((save, file, edits, containers, crops))
    }

    /// The creatures saved in the world, once, as it opens.
    pub fn take_creatures(&mut self) -> Vec<SavedCreature> {
        std::mem::take(&mut self.opened_creatures)
    }

    /// The saved state of the player from elsewhere called `name`, if they've played here.
    pub fn player(&self, name: &PlayerName) -> Option<PlayerState> {
        self.players.get(name.as_str()).cloned()
    }

    /// The skin `state` wears, if it's their own and it could be read.
    pub fn skin(&self, state: &PlayerState) -> Option<Skin> {
        let saved = state.skin.as_ref()?;
        Some(self.skins.get(&saved.file)?.with_arms(saved.arms))
    }

    /// Keeps the state of the player from elsewhere called `name` as they leave, and the skin
    /// they wear, to save soon.
    pub fn left(&mut self, name: &PlayerName, state: PlayerState, skin: Option<&Skin>) {
        self.keep_player(name, state);
        self.keep_skin(skin);
        self.save_soon();
    }

    /// The world's directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The game mode players new to the world start in.
    pub fn new_player_mode(&self) -> GameMode {
        self.new_player_mode
    }

    /// Saves what's changed at the next frame's autosave rather than waiting for its time.
    pub fn save_soon(&mut self) {
        self.next_autosave = Duration::ZERO;
    }

    // Keeps a player's state, to be written if it's changed.
    fn keep_player(&mut self, name: &PlayerName, state: PlayerState) {
        if self.players.get(name.as_str()) != Some(&state) {
            self.unsaved_players.insert(name.as_str().to_owned());
            self.players.insert(name.as_str().to_owned(), state);
        }
    }

    // Keeps a skin worn, to be written if it's new.
    fn keep_skin(&mut self, skin: Option<&Skin>) {
        let Some(skin) = skin else {
            return;
        };
        let file = SavedSkin::of(skin).file;
        if !self.skins.contains_key(&file) {
            self.unsaved_skins.insert(file.clone());
            self.skins.insert(file, skin.clone());
        }
    }

    // Takes what's changed since the last snapshot: the regions with new edits or changed
    // containers or crops, all of whose edits, containers and crops are copied to be written
    // again, `file` if it's not what was last written, and the players changed.
    fn snapshot(
        &mut self,
        edits: &mut Edits,
        containers: &mut Containers,
        crops: &mut Crops,
        file: WorldFile,
    ) -> Snapshot {
        let changed = edits
            .take_changed()
            .into_iter()
            .chain(containers.take_changed())
            .chain(crops.take_changed());
        self.unsaved.extend(changed.map(Region::containing));
        let mut regions: HashMap<_, HashMap<_, _>> =
            self.unsaved.drain().map(|r| (r, HashMap::new())).collect();
        if !regions.is_empty() {
            // with those of chunks with no edits, as chests in structures (M2.3) will be
            // and every crop's chunk has an edit, the crop itself
            for coord in edits.chunks().chain(containers.chunks()) {
                if let Some(chunks) = regions.get_mut(&Region::containing(coord)) {
                    chunks.entry(coord).or_insert_with(|| ChunkEdits {
                        coord,
                        blocks: edits.in_chunk(coord).collect(),
                        containers: containers
                            .in_chunk(coord)
                            .map(|(local, container)| (local, container.clone()))
                            .collect(),
                        crops: crops.in_chunk(coord).collect(),
                    });
                }
            }
        }
        let regions = regions
            .into_iter()
            .map(|(region, chunks)| (region, chunks.into_values().collect()))
            .collect();
        let world_file = (self.written.as_ref() != Some(&file)).then(|| file.clone());
        self.written = Some(file);
        let players = self
            .unsaved_players
            .drain()
            .map(|name| {
                let state = self.players[&name].clone();
                (name, state)
            })
            .collect();
        let skins = self
            .unsaved_skins
            .drain()
            .map(|file| {
                let skin = self.skins[&file].clone();
                (file, skin)
            })
            .collect();
        Snapshot {
            dir: self.dir.clone(),
            world_file,
            regions,
            players,
            creatures: Vec::new(),
            skins,
        }
    }

    // The regions whose creatures aren't as they were last written, each with all of its own,
    // in an order of their own so the same creatures are always written the same.
    fn changed_creatures(
        &mut self,
        creatures: impl IntoIterator<Item = SavedCreature>,
    ) -> Vec<(Region, Vec<SavedCreature>)> {
        let mut now: HashMap<Region, Vec<SavedCreature>> = HashMap::new();
        for creature in creatures {
            let at = creature.position.floor().as_i64vec3();
            let region = Region::containing(ChunkCoordinate::containing(at));
            now.entry(region).or_default().push(creature);
        }
        for creatures in now.values_mut() {
            creatures.sort_unstable_by_key(|creature| {
                let [x, y, z] = creature.position.to_array().map(f32::to_bits);
                (creature.kind.name(), x, y, z, creature.health)
            });
        }
        let gone: Vec<Region> = self
            .written_creatures
            .keys()
            .filter(|&region| !now.contains_key(region))
            .copied()
            .collect();
        let mut changed = Vec::new();
        for region in gone {
            self.written_creatures.remove(&region);
            changed.push((region, Vec::new()));
        }
        for (region, creatures) in now {
            let unsaved = self.unsaved_creatures.remove(&region);
            if unsaved || self.written_creatures.get(&region) != Some(&creatures) {
                self.written_creatures.insert(region, creatures.clone());
                changed.push((region, creatures));
            }
        }
        // those that failed and have none now, to remove
        for region in self.unsaved_creatures.drain() {
            changed.push((region, Vec::new()));
        }
        changed
    }

    // A failed save leaves what it was writing to be written next time.
    fn finished(&mut self, result: io::Result<()>, contents: Contents) {
        let (regions, players, creatures, skins) = contents;
        if let Err(error) = result {
            error!("couldn't save the world in {}: {error}", self.dir.display());
            self.unsaved.extend(regions);
            self.unsaved_players.extend(players);
            self.unsaved_creatures.extend(creatures);
            self.unsaved_skins.extend(skins);
            self.written = None;
        }
    }
}

impl Snapshot {
    fn is_empty(&self) -> bool {
        self.world_file.is_none()
            && self.regions.is_empty()
            && self.players.is_empty()
            && self.creatures.is_empty()
            && self.skins.is_empty()
    }

    // What to write again if writing it fails.
    fn contents(&self) -> Contents {
        let regions = self.regions.keys().copied().collect();
        let players = self.players.iter().map(|(name, _)| name.clone()).collect();
        let creatures = self.creatures.iter().map(|(region, _)| *region).collect();
        let skins = self.skins.iter().map(|(file, _)| file.clone()).collect();
        (regions, players, creatures, skins)
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
        if !self.players.is_empty() {
            let players_dir = self.dir.join(PLAYERS_DIR);
            fs::create_dir_all(&players_dir)?;
            for (name, state) in self.players {
                let text = toml::to_string(&state).expect("players always encode");
                write_atomically(&players_dir.join(name + ".toml"), text.as_bytes())?;
            }
        }
        if !self.skins.is_empty() {
            let skins_dir = self.dir.join(SKINS_DIR);
            fs::create_dir_all(&skins_dir)?;
            for (file, skin) in self.skins {
                write_atomically(&skins_dir.join(file), skin.png())?;
            }
        }
        if !self.creatures.is_empty() {
            let creatures_dir = self.dir.join(creatures::DIR);
            fs::create_dir_all(&creatures_dir)?;
            for (region, saved) in self.creatures {
                let path = creatures_dir.join(region.file_name_for(creatures::EXTENSION));
                if saved.is_empty() {
                    match fs::remove_file(&path) {
                        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                        _ => {}
                    }
                } else {
                    write_atomically(&path, &creatures::encode(&saved))?;
                }
            }
        }
        Ok(())
    }
}

// Written beside it and renamed over it, so a crash part way through leaves the old file whole.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_added_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    // on disk before it takes the name, or a crash could leave the name on an empty file
    file.sync_all()?;
    fs::rename(&temporary, path)
}

/// The game state that's saved.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
struct Savable<'w, 's> {
    world: ResMut<'w, World>,
    containers: ResMut<'w, Containers>,
    crops: ResMut<'w, Crops>,
    clock: Res<'w, TimeOfDay>,
    operators: Res<'w, Operators>,
    dormant: Option<Res<'w, Dormant>>,
    creatures: Query<
        'w,
        's,
        (
            &'static Creature,
            &'static Transform,
            &'static Health,
            &'static Life,
            Option<&'static Townsfolk>,
        ),
    >,
    settled: Option<Res<'w, Settled>>,
    players: Query<
        'w,
        's,
        (
            &'static Transform,
            &'static Player,
            &'static PlayerLook,
            &'static GameMode,
            &'static Hotbar,
            &'static Inventory,
            &'static PlayerOwner,
            Option<&'static PlayerName>,
            &'static Vitals,
            &'static SpawnBed,
            Option<&'static Skin>,
        ),
    >,
}

impl Savable<'_, '_> {
    fn snapshot(&mut self, save: &mut WorldSave) -> Snapshot {
        let mut file = WorldFile::new(self.world.seed(), *self.clock);
        file.ops = self.operators.names().to_vec();
        file.new_player_mode = save.new_player_mode;
        file.settled = self
            .settled
            .iter()
            .flat_map(|settled| settled.0.iter().map(|&(x, z)| [x, z]))
            .collect();
        file.player = save.host.clone();
        for (body, player, look, mode, hotbar, inventory, owner, name, vitals, bed, skin) in
            &self.players
        {
            let state = PlayerState::of(body, *look, player.movement, *mode, *hotbar, inventory)
                .surviving(*vitals, *bed)
                .wearing(skin);
            save.keep_skin(skin);
            match (owner.0, name) {
                (ClientId::Server, _) => file.player = Some(state),
                (_, Some(name)) => save.keep_player(name, state),
                _ => {}
            }
        }
        // those near players as they are, and those asleep
        // monsters never are, as they despawn
        let awake = self
            .creatures
            .iter()
            .filter(|(creature, ..)| !creature.0.properties().despawns)
            .map(|(creature, body, health, life, townsfolk)| {
                saved(creature, body, health, life, townsfolk)
            });
        let asleep = self
            .dormant
            .iter()
            .flat_map(|dormant| dormant.0.values().flatten());
        let creatures = awake.chain(asleep.copied());
        let mut snapshot = save.snapshot(
            self.world.edits_mut(),
            &mut self.containers,
            &mut self.crops,
            file,
        );
        snapshot.creatures = save.changed_creatures(creatures);
        snapshot
    }
}

// Every so often, writes what's changed on the IO pool, one save at a time so they land in order.
fn autosave(time: Res<Time<Real>>, mut save: ResMut<WorldSave>, mut savable: Savable) {
    if let Some(writing) = &mut save.writing {
        let Some(result) = futures::check_ready(&mut writing.task) else {
            return;
        };
        let contents = save.writing.take().unwrap().contents;
        save.finished(result, contents);
    }
    if time.elapsed() < save.next_autosave {
        return;
    }
    save.next_autosave = time.elapsed() + AUTOSAVE_INTERVAL;
    let snapshot = savable.snapshot(&mut save);
    if snapshot.is_empty() {
        return;
    }
    let contents = snapshot.contents();
    let task = IoTaskPool::get().spawn(async move { snapshot.write() });
    save.writing = Some(Writing { task, contents });
}

// The last save is written here on the main thread, after any still being written, rather than
// on the IO pool, whose threads end with the process: the app only exits once this returns.
fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    mut save: ResMut<WorldSave>,
    mut savable: Savable,
) {
    if exits.read().count() > 0 {
        write_last_save(&mut save, &mut savable);
    }
}

// The same as the world's left for another, which is done with its save once it's written.
fn save_on_leaving(mut save: ResMut<WorldSave>, mut savable: Savable, mut commands: Commands) {
    write_last_save(&mut save, &mut savable);
    commands.remove_resource::<WorldSave>();
}

fn write_last_save(save: &mut WorldSave, savable: &mut Savable) {
    if let Some(writing) = save.writing.take() {
        save.finished(block_on(writing.task), writing.contents);
    }
    let snapshot = savable.snapshot(save);
    let contents = snapshot.contents();
    let result = snapshot.write();
    if result.is_ok() {
        info!("saved the world in {}", save.dir.display());
    }
    save.finished(result, contents);
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

    use super::world_file::FORMAT_VERSION;
    use super::{
        creatures, region, Region, SavePlugin, WorldFile, WorldSave, EDITS_DIR, WORLD_FILE,
    };
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE};
    use crate::clock::TimeOfDay;
    use crate::containers::{Containers, FUEL, INPUT};
    use crate::crafting::SMELT_TIME;
    use crate::creatures::{CreatureKind, SavedCreature};
    use crate::edits::Edits;
    use crate::farming::Crops;
    use crate::game_mode::GameMode;
    use crate::inventory::Hotbar;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::player::name::PlayerName;
    use crate::player::{Movement, Player, PlayerLook};
    use crate::protocol::ProtocolPlugin;
    use crate::server::creatures::{creature, Dormant, SpawnRate};
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
        let (_, file, edits, ..) = WorldSave::open(dir.world(), Some(7)).unwrap();
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
            let (_, file, ..) = WorldSave::open(dir.world(), asked).unwrap();
            assert_eq!(7, file.seed);
        }

        let random = TempDir::new("random-seed");
        let (_, file, ..) = WorldSave::open(random.world(), None).unwrap();
        assert_eq!(
            file.seed,
            WorldSave::open(random.world(), None).unwrap().1.seed
        );
    }

    #[test]
    fn test_a_world_from_a_newer_version_is_refused() {
        let dir = TempDir::new("newer");
        let mut file = WorldFile::new(1, TimeOfDay::from_days(0.4));
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
        let (mut save, file, mut edits, mut containers, mut crops) =
            WorldSave::open(dir.world(), Some(1)).unwrap();
        let (here, there) = (I64Vec3::new(5, 500, 5), I64Vec3::new(-600, 470, 9000));
        edits.set(here, BlockType::Air);
        edits.set(there, BlockType::Bricks);
        let snapshot = save.snapshot(&mut edits, &mut containers, &mut crops, file.clone());
        // world.toml hasn't changed since it was written
        assert!(snapshot.world_file.is_none());
        assert_eq!(2, snapshot.regions.len());
        snapshot.write().unwrap();
        assert!(save
            .snapshot(&mut edits, &mut containers, &mut crops, file.clone())
            .is_empty());

        let (near, far) = (
            Region::containing(ChunkCoordinate::containing(here)),
            Region::containing(ChunkCoordinate::containing(there)),
        );
        fs::write(region_file(&dir, far), "left alone").unwrap();
        edits.set(here + I64Vec3::X, BlockType::Planks);
        let snapshot = save.snapshot(&mut edits, &mut containers, &mut crops, file);
        assert_eq!(vec![&near], snapshot.regions.keys().collect::<Vec<_>>());
        snapshot.write().unwrap();
        assert_eq!(
            "left alone",
            fs::read_to_string(region_file(&dir, far)).unwrap()
        );

        // the rewritten region keeps its old edits as well as the new
        let mut loaded = Edits::default();
        let bytes = fs::read(region_file(&dir, near)).unwrap();
        region::decode(
            &bytes,
            &mut loaded,
            &mut Containers::default(),
            &mut Crops::default(),
        )
        .unwrap();
        let chunk = ChunkCoordinate::containing(here);
        let mut blocks: Vec<_> = loaded.in_chunk(chunk).collect();
        blocks.sort_unstable_by_key(|(local, _)| local.x);
        let local = ChunkCoordinate::local_block(here);
        assert_eq!(
            vec![
                (local, BlockType::Air.into()),
                (local + U16Vec3::X, BlockType::Planks.into())
            ],
            blocks
        );
    }

    #[test]
    fn test_containers_are_saved_with_their_regions_and_alone_rewrite_them() {
        let dir = TempDir::new("containers");
        let (mut save, file, mut edits, mut containers, mut crops) =
            WorldSave::open(dir.world(), Some(1)).unwrap();
        let at = I64Vec3::new(-40, 460, 3);
        edits.set(at, BlockType::Furnace);
        let furnace = containers.open(at, BlockType::Furnace).unwrap();
        furnace.slots[INPUT] = Some(ItemStack::new(Item::Block(BlockType::IronOre), 4));
        furnace.slots[FUEL] = Some(ItemStack::new(Item::Coal, 2));
        containers.run(SMELT_TIME * 1.5);
        let saved = containers.get(at).cloned();
        save.snapshot(&mut edits, &mut containers, &mut crops, file.clone())
            .write()
            .unwrap();
        let (_, _, _, loaded, _) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!(saved, loaded.get(at).cloned());

        // a furnace burning changes nothing but itself, and that's saved too
        containers.run(1.0);
        let snapshot = save.snapshot(&mut edits, &mut containers, &mut crops, file);
        assert_eq!(1, snapshot.regions.len());
        snapshot.write().unwrap();
        let (_, _, _, loaded, _) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!(containers.get(at), loaded.get(at));
    }

    #[test]
    fn test_crops_are_saved_with_their_regions_and_alone_rewrite_them() {
        let dir = TempDir::new("crops");
        let (mut save, file, mut edits, mut containers, mut crops) =
            WorldSave::open(dir.world(), Some(1)).unwrap();
        let at = I64Vec3::new(70, 460, -3);
        edits.set(at, BlockType::Wheat0);
        crops.plant(at, 27_000);
        save.snapshot(&mut edits, &mut containers, &mut crops, file.clone())
            .write()
            .unwrap();
        let (.., loaded) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!(Some(27_000), loaded.planted(at));

        // a crop ripening and let go of changes no edit, and is saved all the same
        crops.grew(at, None);
        let snapshot = save.snapshot(&mut edits, &mut containers, &mut crops, file);
        assert_eq!(1, snapshot.regions.len());
        snapshot.write().unwrap();
        let (_, _, edits, _, loaded) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!(None, loaded.planted(at));
        assert_eq!(1, edits.chunks().count());
    }

    #[test]
    fn test_a_format_2_world_opens_and_is_written_in_the_latest_format() {
        type Format2Chunk = ([i64; 3], Vec<(u16, u16)>);
        #[derive(serde::Serialize)]
        struct Format2Region<'a> {
            palette: Vec<&'a str>,
            chunks: Vec<Format2Chunk>,
        }
        let dir = TempDir::new("format-2");
        let mut old = WorldFile::new(5, TimeOfDay::from_days(0.4));
        old.format = 2;
        fs::create_dir_all(dir.world().join(EDITS_DIR)).unwrap();
        // with its time in days, as it was then
        let old = old.to_toml().replace("time = 4800", "time_of_day = 0.4");
        fs::write(dir.world().join(WORLD_FILE), old).unwrap();
        let region = postcard::to_stdvec(&Format2Region {
            palette: vec!["furnace"],
            chunks: vec![([0, 28, 0], vec![(7, 0)])],
        })
        .unwrap();
        fs::write(region_file(&dir, Region { x: 0, z: 0 }), region).unwrap();

        let (mut save, file, mut edits, mut containers, mut crops) =
            WorldSave::open(dir.world(), None).unwrap();
        assert_eq!((5, 2), (file.seed, file.format));
        let chunk = ChunkCoordinate(I64Vec3::new(0, 28, 0));
        let blocks: Vec<_> = edits.in_chunk(chunk).collect();
        assert_eq!(
            vec![(U16Vec3::new(7, 0, 0), BlockType::Furnace.into())],
            blocks
        );
        // and its world.toml's written in the new format, its regions as they change
        let snapshot = save.snapshot(
            &mut edits,
            &mut containers,
            &mut crops,
            WorldFile::new(5, TimeOfDay::from_days(0.4)),
        );
        assert_eq!(FORMAT_VERSION, snapshot.world_file.as_ref().unwrap().format);
        snapshot.write().unwrap();
        let (.., crops) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!(0, crops.chunks().count());
        assert_eq!(
            FORMAT_VERSION,
            WorldSave::open(dir.world(), None).unwrap().1.format
        );
    }

    // The game's saving, headless: a world played in one app is there in the next. The
    // server opens it and spawns its player, with no camera, as a server's players have none.
    fn play(dir: &TempDir) -> App {
        play_hosted_by(dir, Some("host"))
    }

    // The same, with a host called `host`, or none as on a dedicated server.
    fn play_hosted_by(dir: &TempDir, host: Option<&str>) -> App {
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
            host: host.map(PlayerName::new),
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
        let ore = ItemStack::new(Item::Block(BlockType::IronOre), 7);
        let looking = PlayerLook {
            yaw: 4.0,
            pitch: -0.5,
        };
        {
            let world = app.world_mut();
            let mut game_world = world.resource_mut::<crate::world::World>();
            assert!(game_world.edit(built, BlockType::Furnace));
            world.resource_mut::<TimeOfDay>().0 = 42_000;
            let mut players = world.query::<(
                &mut Transform,
                &mut PlayerLook,
                &mut Player,
                &mut GameMode,
                &mut Hotbar,
                &mut Inventory,
            )>();
            let (mut transform, mut look, mut player, mut mode, mut hotbar, mut inventory) =
                players.single_mut(world).unwrap();
            transform.translation = at;
            *look = looking;
            player.movement = Movement::Walk;
            *mode = GameMode::Survival;
            inventory.slots[2] = Some(ore);
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
        assert_eq!(42_000, app.world().resource::<TimeOfDay>().0);
        let (mode, hotbar, inventory) = app
            .world_mut()
            .query::<(&GameMode, &Hotbar, &Inventory)>()
            .single(app.world())
            .unwrap();
        assert_eq!(GameMode::Survival, *mode);
        assert_eq!(Some(ore), hotbar.held(inventory));
        let mut world = app.world_mut().resource_mut::<crate::world::World>();
        world.insert_chunk(ChunkCoordinate::containing(built), ChunkData::default());
        assert_eq!(BlockType::Furnace, world.get_block_at(built));
        assert_eq!(BlockType::Air, world.get_block_at(built + I64Vec3::Y));
    }

    #[test]
    fn test_water_that_flowed_is_there_after_a_restart_and_stands_still() {
        use crate::fluids::{fluid_of, Fluid, Level};
        use crate::server::fluids::{Fluids, FLUID_TICK};
        let dir = TempDir::new("flowed");
        // high over land far from the player, in empty chunks of its own
        let origin = I64Vec3::new(5000, 470, 5000);
        let load = |app: &mut App| {
            let mut world = app.world_mut().resource_mut::<crate::world::World>();
            let middle = ChunkCoordinate::containing(origin).0;
            for x in -1..=1 {
                for y in -1..=1 {
                    for z in -1..=1 {
                        let coord = ChunkCoordinate(middle + I64Vec3::new(x, y, z));
                        world.insert_chunk(coord, ChunkData::default());
                    }
                }
            }
        };
        let cells: Vec<I64Vec3> = (-8..=8)
            .flat_map(|x| (-8..=8).map(move |z| origin + I64Vec3::new(x, 0, z)))
            .collect();
        let mut app = play(&dir);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(FLUID_TICK));
        load(&mut app);
        {
            let mut world = app.world_mut().resource_mut::<crate::world::World>();
            for &cell in &cells {
                world.edit(cell - I64Vec3::Y, BlockType::Stone);
            }
            world.edit(origin, Fluid::Water.source());
        }
        for _ in 0..60 {
            app.update();
        }
        let world = app.world().resource::<crate::world::World>();
        let flowed: Vec<_> = cells.iter().map(|&cell| world.get(cell)).collect();
        assert_eq!(
            Some((Fluid::Water, Level::Flowing(7))),
            fluid_of(world.get(origin + I64Vec3::new(4, 0, 3)))
        );
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);

        let mut app = play(&dir);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(FLUID_TICK));
        load(&mut app);
        for _ in 0..20 {
            app.update();
        }
        assert_eq!(0, app.world().resource::<Fluids>().pending());
        let world = app.world().resource::<crate::world::World>();
        let reloaded: Vec<_> = cells.iter().map(|&cell| world.get(cell)).collect();
        assert!(flowed == reloaded, "the flow isn't as it was saved");
    }

    #[test]
    fn test_creatures_are_there_after_a_restart_and_gone_once_theyve_gone() {
        let dir = TempDir::new("creatures");
        let mut app = play(&dir);
        app.insert_resource(SpawnRate(None));
        let near = Vec3::new(3.5, 470.0, -2.5);
        // a lamb, and a shorn sheep asleep
        let lamb = SavedCreature {
            health: 6,
            grows_up_in: 100.0,
            ..SavedCreature::grown(CreatureKind::Sheep, near)
        };
        let far = SavedCreature {
            health: 3,
            regrows_in: 50.0,
            ..SavedCreature::grown(CreatureKind::Sheep, Vec3::new(5000.5, 466.0, 20.25))
        };
        app.world_mut().spawn(creature(lamb, 0.0));
        // and a monster beside it, which isn't saved, as monsters despawn
        let slinger = SavedCreature::grown(CreatureKind::Slinger, near + Vec3::X);
        app.world_mut().spawn(creature(slinger, 0.0));
        app.world_mut().resource_mut::<Dormant>().0.insert(
            Region::containing(ChunkCoordinate::containing(far.position.as_i64vec3())),
            vec![far],
        );
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);
        assert_eq!(
            2,
            fs::read_dir(dir.world().join(creatures::DIR))
                .unwrap()
                .count()
        );

        let mut app = play(&dir);
        app.insert_resource(SpawnRate(None));
        let mut saved: Vec<_> = app
            .world()
            .resource::<Dormant>()
            .0
            .values()
            .flatten()
            .copied()
            .collect();
        saved.sort_by(|a, b| a.position.x.total_cmp(&b.position.x));
        // the lamb a frame older, as it grew while the world closed
        assert_eq!(2, saved.len());
        assert_eq!(
            (near.x, 6, 0.0),
            (saved[0].position.x, saved[0].health, saved[0].regrows_in)
        );
        assert!((95.0..=100.0).contains(&saved[0].grows_up_in), "{saved:?}");
        assert_eq!(far, saved[1]);
        // and gone from the save once they're gone from the world
        app.world_mut().resource_mut::<Dormant>().0.clear();
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(
            0,
            fs::read_dir(dir.world().join(creatures::DIR))
                .unwrap()
                .count()
        );
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
        let (_, file, edits, ..) = WorldSave::open(dir.world(), None).unwrap();
        assert!(file.player.is_some());
        assert_eq!(
            1,
            edits.in_chunk(ChunkCoordinate::containing(built)).count()
        );
    }

    #[test]
    fn test_a_dedicated_server_leaves_the_hosts_player_as_it_was() {
        let dir = TempDir::new("no-host");
        let mut app = play(&dir);
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);
        let hosts = WorldSave::open(dir.world(), None).unwrap().1.player;
        assert!(hosts.is_some());

        let mut app = play_hosted_by(&dir, None);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        let mut players = app.world_mut().query::<&Player>();
        assert_eq!(0, players.iter(app.world()).count());
        app.world_mut().resource_mut::<TimeOfDay>().0 = 24_000;
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);
        let (_, file, ..) = WorldSave::open(dir.world(), None).unwrap();
        assert_eq!((24_000, hosts), (file.time, file.player));
    }
}
