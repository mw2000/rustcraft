//! The worlds saved in a directory of them, as the Singleplayer screen lists and manages them: a
//! world's name is its directory's.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::ecs::resource::Resource;

use super::{write_atomically, WorldFile, WORLD_FILE};
use crate::clock::TimeOfDay;
use crate::game_mode::GameMode;

/// Where the worlds are saved, a directory each, as the menus list them.
#[derive(Resource, Clone)]
pub struct WorldsDir(pub PathBuf);

/// The longest a world's name can be.
pub const MAX_NAME: usize = 32;

/// A world saved in the worlds directory.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedWorld {
    pub name: String,
    pub dir: PathBuf,
    /// Its `world.toml`, or why it couldn't be read.
    pub file: Result<WorldFile, String>,
    /// When it was last saved, so last played.
    pub last_played: Option<SystemTime>,
}

impl SavedWorld {
    /// The game mode it's played in: the host's, if they've played it, or else new players'.
    pub fn game_mode(&self) -> Option<GameMode> {
        let file = self.file.as_ref().ok()?;
        Some(
            file.player
                .as_ref()
                .map_or(file.new_player_mode, |player| player.game_mode),
        )
    }
}

/// Every world in `worlds`, the last played first.
pub fn list(worlds: &Path) -> Vec<SavedWorld> {
    let Ok(entries) = fs::read_dir(worlds) else {
        return Vec::new();
    };
    let mut found: Vec<SavedWorld> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(WORLD_FILE).is_file())
        .map(|entry| read(entry.path()))
        .collect();
    found.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.name.cmp(&b.name))
    });
    found
}

fn read(dir: PathBuf) -> SavedWorld {
    let path = dir.join(WORLD_FILE);
    let file = fs::read_to_string(&path)
        .and_then(|text| WorldFile::from_toml(&text))
        .map_err(|error| error.to_string());
    SavedWorld {
        name: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        last_played: fs::metadata(&path).and_then(|m| m.modified()).ok(),
        file,
        dir,
    }
}

/// Whether `c` can be in a world's name: anything but what some platform won't have in a
/// directory's name.
pub fn allowed_in_name(c: char) -> bool {
    !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

/// `name` made fit to name a world, or none if nothing's left of it.
pub fn tidy_name(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|&c| allowed_in_name(c))
        .take(MAX_NAME)
        .collect();
    // nor a name a directory can't have on Windows, nor one that's a directory already
    let name = name.trim().trim_end_matches('.').trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// `wanted`, or if a world in `worlds` has it already, it with the first number after it that
/// none has, such as "New World 2".
pub fn unused_name(worlds: &Path, wanted: &str) -> String {
    let taken = |name: &str| worlds.join(name).exists();
    if !taken(wanted) {
        return wanted.to_owned();
    }
    (2..)
        .map(|n| format!("{wanted} {n}"))
        .find(|name| !taken(name))
        .expect("there's always a number free")
}

/// Makes a new world in `worlds` called `name`, or the first free name like it, with `seed` or
/// a random one, for new players to start in `mode`. Returns its directory.
pub fn create(worlds: &Path, name: &str, seed: Option<u32>, mode: GameMode) -> io::Result<PathBuf> {
    let name = tidy_name(name).ok_or_else(|| invalid("a world needs a name"))?;
    let dir = worlds.join(unused_name(worlds, &name));
    let mut file = WorldFile::new(seed.unwrap_or_else(rand::random), TimeOfDay::default());
    file.new_player_mode = mode;
    fs::create_dir_all(&dir)?;
    write_atomically(&dir.join(WORLD_FILE), file.to_toml().as_bytes())?;
    Ok(dir)
}

/// Renames the world in `dir` to `name`, refusing a name another world has. Returns its new
/// directory.
pub fn rename(dir: &Path, name: &str) -> io::Result<PathBuf> {
    let name = tidy_name(name).ok_or_else(|| invalid("a world needs a name"))?;
    let renamed = dir.with_file_name(&name);
    if renamed == dir {
        return Ok(renamed);
    }
    // a name differing only in case is the same directory on some platforms
    let clash = fs::read_dir(dir.parent().unwrap_or(dir))?
        .filter_map(Result::ok)
        .any(|entry| entry.path() != dir && entry.file_name().eq_ignore_ascii_case(&name));
    if clash {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("there's a world called {name} already"),
        ));
    }
    fs::rename(dir, &renamed)?;
    Ok(renamed)
}

/// Copies the world in `dir` beside it, named as it is with a number after it. Returns the
/// copy's directory.
pub fn duplicate(dir: &Path) -> io::Result<PathBuf> {
    let worlds = dir.parent().unwrap_or(dir);
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let copy = worlds.join(unused_name(worlds, &name));
    copy_dir(dir, &copy)?;
    Ok(copy)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()))?;
        } else {
            fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Deletes the world in `dir`, for good.
pub fn delete(dir: &Path) -> io::Result<()> {
    fs::remove_dir_all(dir)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustcraft-worlds-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_worlds_are_made_listed_renamed_duplicated_and_deleted() {
        let worlds = temp_dir("library");
        assert!(list(&worlds).is_empty());

        let first = create(&worlds, "  My World  ", Some(7), GameMode::Creative).unwrap();
        assert_eq!(worlds.join("My World"), first);
        // the same name again gets a number
        let second = create(&worlds, "My World", None, GameMode::Survival).unwrap();
        assert_eq!(worlds.join("My World 2"), second);
        let listed = list(&worlds);
        assert_eq!(2, listed.len());
        let mine = listed.iter().find(|w| w.dir == first).unwrap();
        assert_eq!("My World", mine.name);
        assert_eq!(7, mine.file.as_ref().unwrap().seed);
        assert_eq!(Some(GameMode::Creative), mine.game_mode());
        assert!(mine.last_played.is_some());

        let renamed = rename(&first, "Castle").unwrap();
        assert_eq!(worlds.join("Castle"), renamed);
        assert!(!first.exists());
        // nor onto another world, whatever its case
        assert!(rename(&renamed, "my world 2").is_err());
        assert!(rename(&renamed, "///").is_err());

        let copy = duplicate(&renamed).unwrap();
        assert_eq!(worlds.join("Castle 2"), copy);
        let copied = list(&worlds).into_iter().find(|w| w.dir == copy).unwrap();
        assert_eq!(7, copied.file.unwrap().seed);

        delete(&renamed).unwrap();
        let names: Vec<_> = list(&worlds).into_iter().map(|w| w.name).collect();
        assert_eq!(2, names.len());
        assert!(!names.contains(&"Castle".to_owned()));
        fs::remove_dir_all(&worlds).unwrap();
    }

    #[test]
    fn test_a_world_that_cant_be_read_is_listed_with_why() {
        let worlds = temp_dir("unreadable");
        fs::create_dir_all(worlds.join("Broken")).unwrap();
        fs::write(
            worlds.join("Broken").join(WORLD_FILE),
            "format = 99\nseed = 1",
        )
        .unwrap();
        let listed = list(&worlds);
        assert_eq!(1, listed.len());
        assert!(listed[0].file.as_ref().unwrap_err().contains("newer"));
        assert_eq!(None, listed[0].game_mode());
        fs::remove_dir_all(&worlds).unwrap();
    }

    #[test]
    fn test_names_keep_only_what_a_directory_can_have() {
        assert_eq!(Some("ab".to_owned()), tidy_name("a/b"));
        assert_eq!(Some("Home".to_owned()), tidy_name(" Home. "));
        assert_eq!(None, tidy_name(" :: "));
        assert_eq!(MAX_NAME, tidy_name(&"x".repeat(50)).unwrap().len());
    }
}
