//! Which of the world the local player has seen, as tiles revealed around them as they go, kept
//! by the client in a small file of its own.
//!
//! It's kept by the client rather than in the server's save of the player: the map shows nothing
//! the seed doesn't, which every client has (§4.4), so there's nothing for the server to guard
//! (INV-4), and it needs no messages or save format of its own. A player joining a server keeps
//! their map of it under the server's address, and the host theirs in the world's directory, so
//! it goes with the world.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::tasks::{block_on, futures, IoTaskPool, Task};
use serde::{Deserialize, Serialize};

use crate::client::player::LocalPlayer;
use crate::save::write_atomically;
use crate::world::World;

/// How wide a map tile is, in blocks.
pub const TILE_BLOCKS: i32 = 64;

/// How far around the player the map reveals: as far as the land's drawn at full detail with
/// the default settings, so what's on the map is what was seen up close, block by block. Out
/// to the horizon would reveal kilometres from wherever the player stands, and the map would no
/// longer show where they've been.
pub const REVEAL_RADIUS: f32 = 256.0;

const SAVE_INTERVAL: Duration = Duration::from_secs(30);

/// The map tiles the local player has seen, by tile coordinate: the tile at (x, z) covers
/// blocks `x * TILE_BLOCKS` to `(x + 1) * TILE_BLOCKS` across, and the same down.
#[derive(Resource, Default)]
pub struct Explored {
    tiles: HashSet<IVec2>,
    // the tile the player was last revealed from, as nothing new is revealed until they leave it
    last_tile: Option<IVec2>,
    unsaved: bool,
}

impl Explored {
    /// The tile that block column (`x`, `z`) is in.
    pub fn tile_of(at: Vec2) -> IVec2 {
        (at / TILE_BLOCKS as f32).floor().as_ivec2()
    }

    pub fn contains(&self, tile: IVec2) -> bool {
        self.tiles.contains(&tile)
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Reveals each tile whose middle is within `REVEAL_RADIUS` of `at` (x, z), unless that's
    /// been done from the tile `at` is in already. Returns whether any were new.
    pub fn reveal_around(&mut self, at: Vec2) -> bool {
        let here = Self::tile_of(at);
        if self.last_tile == Some(here) {
            return false;
        }
        self.last_tile = Some(here);
        let reach = (REVEAL_RADIUS / TILE_BLOCKS as f32).ceil() as i32 + 1;
        let before = self.tiles.len();
        for z in here.y - reach..=here.y + reach {
            for x in here.x - reach..=here.x + reach {
                let tile = IVec2::new(x, z);
                let middle = (tile.as_vec2() + 0.5) * TILE_BLOCKS as f32;
                if middle.distance(at) <= REVEAL_RADIUS {
                    self.tiles.insert(tile);
                }
            }
        }
        let new = self.tiles.len() > before;
        self.unsaved |= new;
        new
    }

    // A map that can't be read is carried on from empty, and saving writes over it.
    fn add_read(&mut self, read: io::Result<HashSet<IVec2>>, path: &Path) {
        match read {
            Ok(tiles) => {
                info!(
                    "read a map of {} tiles from {}",
                    tiles.len(),
                    path.display()
                );
                self.tiles.extend(tiles);
            }
            Err(error) => error!("couldn't read the map in {}: {error}", path.display()),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut tiles: Vec<_> = self.tiles.iter().map(|tile| tile.to_array()).collect();
        // so the same map always writes the same file
        tiles.sort_unstable();
        postcard::to_stdvec(&SavedMap { tiles }).expect("a map always encodes")
    }

    fn decode(bytes: &[u8]) -> io::Result<HashSet<IVec2>> {
        let map: SavedMap = postcard::from_bytes(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(map.tiles.into_iter().map(IVec2::from_array).collect())
    }
}

#[derive(Serialize, Deserialize)]
struct SavedMap {
    tiles: Vec<[i32; 2]>,
}

/// The directory this client keeps its map of the world being played in: the world's own, or
/// one for the server joined. Each seed played there gets a file.
#[derive(Resource, Clone)]
pub struct MapDir(pub PathBuf);

impl MapDir {
    /// For the server at `address`, as it was given to join it.
    pub fn joined(address: &str) -> Self {
        let name: String = address
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        Self(
            dirs::data_dir()
                .expect("there's no data directory to keep maps in")
                .join("rustcraft")
                .join("servers")
                .join(name),
        )
    }

    fn file(&self, seed: u32) -> PathBuf {
        self.0.join(format!("map-{seed}.bin"))
    }
}

/// The file the map's kept in, and its reading and writing in the background (INV-7).
#[derive(Resource)]
pub(super) struct MapFile {
    path: PathBuf,
    loading: Option<Task<io::Result<HashSet<IVec2>>>>,
    writing: Option<Task<io::Result<()>>>,
    next_save: Duration,
}

/// Starts reading the map of the world being played, once there's a world and so a seed.
pub(super) fn open_map_file(dir: Res<MapDir>, world: Res<World>, mut commands: Commands) {
    let path = dir.file(world.seed());
    let reading = path.clone();
    commands.insert_resource(MapFile {
        path,
        loading: Some(IoTaskPool::get().spawn(async move { read(&reading) })),
        writing: None,
        next_save: SAVE_INTERVAL,
    });
}

fn read(path: &Path) -> io::Result<HashSet<IVec2>> {
    match fs::read(path) {
        Ok(bytes) => Explored::decode(&bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(error),
    }
}

fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomically(path, bytes)
}

/// Adds what's been read to what's been explored since the game started.
pub(super) fn finish_loading(mut file: ResMut<MapFile>, mut explored: ResMut<Explored>) {
    if let Some(read) = file.loading.as_mut().and_then(futures::check_ready) {
        file.loading = None;
        explored.add_read(read, &file.path);
    }
}

/// Reveals the map around the local player, changing `Explored` only if that's revealed more.
pub(super) fn explore(
    player: Query<&Transform, With<LocalPlayer>>,
    mut explored: ResMut<Explored>,
) {
    if let Ok(body) = player.single() {
        if explored
            .bypass_change_detection()
            .reveal_around(body.translation.xz())
        {
            explored.set_changed();
        }
    }
}

/// Writes the map every so often if more has been explored, once it's been read.
pub(super) fn autosave(
    time: Res<Time<Real>>,
    mut file: ResMut<MapFile>,
    mut explored: ResMut<Explored>,
) {
    if let Some(written) = file.writing.as_mut().and_then(futures::check_ready) {
        file.writing = None;
        if let Err(error) = written {
            error!("couldn't save the map in {}: {error}", file.path.display());
            explored.unsaved = true;
        }
    }
    if file.loading.is_some()
        || file.writing.is_some()
        || !explored.unsaved
        || time.elapsed() < file.next_save
    {
        return;
    }
    file.next_save = time.elapsed() + SAVE_INTERVAL;
    explored.unsaved = false;
    let (path, bytes) = (file.path.clone(), explored.encode());
    file.writing = Some(IoTaskPool::get().spawn(async move { write(&path, &bytes) }));
}

/// Writes the map as the game quits, if more has been explored since it was last written.
pub(super) fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    file: Option<ResMut<MapFile>>,
    mut explored: ResMut<Explored>,
) {
    if let Some(mut file) = file.filter(|_| exits.read().count() > 0) {
        write_last(&mut file, &mut explored);
    }
}

/// The same as the world's left, forgetting it, ready for the next.
pub(super) fn save_on_leaving(
    file: Option<ResMut<MapFile>>,
    mut explored: ResMut<Explored>,
    mut commands: Commands,
) {
    if let Some(mut file) = file {
        write_last(&mut file, &mut explored);
    }
    *explored = Explored::default();
    commands.remove_resource::<MapFile>();
    commands.remove_resource::<MapDir>();
}

// Waits for what's being read and written, and writes what's still to be.
fn write_last(file: &mut MapFile, explored: &mut Explored) {
    if let Some(loading) = file.loading.take() {
        explored.add_read(block_on(loading), &file.path);
    }
    let written = file
        .writing
        .take()
        .is_none_or(|task| block_on(task).is_ok());
    if explored.unsaved || !written {
        if let Err(error) = write(&file.path, &explored.encode()) {
            error!("couldn't save the map in {}: {error}", file.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiles_are_revealed_in_a_circle_round_the_player() {
        let mut explored = Explored::default();
        let at = Vec2::new(10.0, -20.0);
        explored.reveal_around(at);
        // about the circle's area in tiles
        let tiles = std::f32::consts::PI * (REVEAL_RADIUS / TILE_BLOCKS as f32).powi(2);
        assert!((explored.len() as f32 - tiles).abs() < tiles * 0.15);
        assert!(explored.contains(Explored::tile_of(at)));
        let edge = REVEAL_RADIUS - TILE_BLOCKS as f32;
        assert!(explored.contains(Explored::tile_of(at + Vec2::X * edge)));
        assert!(explored.contains(Explored::tile_of(at - Vec2::Y * edge)));
        let beyond = REVEAL_RADIUS + TILE_BLOCKS as f32;
        assert!(!explored.contains(Explored::tile_of(at + Vec2::X * beyond)));
        assert!(!explored.contains(Explored::tile_of(at + Vec2::splat(REVEAL_RADIUS))));
    }

    #[test]
    fn test_a_tile_is_its_blocks_from_the_corner_down() {
        assert_eq!(IVec2::ZERO, Explored::tile_of(Vec2::new(0.0, 63.9)));
        assert_eq!(IVec2::new(-1, 1), Explored::tile_of(Vec2::new(-0.1, 64.0)));
    }

    #[test]
    fn test_walking_on_reveals_more_and_saving_keeps_it() {
        let mut explored = Explored::default();
        explored.reveal_around(Vec2::ZERO);
        let around_spawn = explored.len();
        // nothing new until the player leaves the tile
        assert!(!explored.reveal_around(Vec2::splat(30.0)));
        assert!(explored.reveal_around(Vec2::new(1000.0, 0.0)));
        assert!(explored.len() > around_spawn);

        let dir = std::env::temp_dir().join(format!("rustcraft-map-test-{}", std::process::id()));
        let file = MapDir(dir.clone()).file(7);
        write(&file, &explored.encode()).unwrap();
        assert_eq!(explored.tiles, read(&file).unwrap());
        // the same map writes the same bytes
        assert_eq!(fs::read(&file).unwrap(), explored.encode());
        fs::remove_dir_all(dir).unwrap();
        assert!(read(&file).unwrap().is_empty());
    }

    #[test]
    fn test_a_map_is_kept_per_seed_under_a_name_safe_for_any_platform() {
        let dir = MapDir::joined("[::1]:25600");
        assert!(dir.0.ends_with("servers/___1__25600"));
        assert_ne!(dir.file(1), dir.file(2));
    }
}
