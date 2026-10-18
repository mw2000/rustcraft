//! The servers the player's saved to join, as the Multiplayer screen lists them, kept in a file
//! of their own under the data directory.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::save::write_atomically;

/// The file under the data directory the servers are kept in.
pub const SERVERS_FILE: &str = "servers.toml";

/// A server saved to join: what the player calls it and where it is, `host:port` or `host`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
}

/// The servers saved, in the order the player added them, and the file they're kept in.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ServerList {
    pub servers: Vec<SavedServer>,
    file: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct ServersFile {
    #[serde(default, rename = "server")]
    servers: Vec<SavedServer>,
}

impl ServerList {
    /// The servers saved in `file`, none if there's no file or it can't be read.
    pub fn read(file: PathBuf) -> Self {
        let servers = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| {
                toml::from_str::<ServersFile>(&text)
                    .inspect_err(|error| {
                        warn!("couldn't read the servers in {}: {error}", file.display())
                    })
                    .ok()
            })
            .unwrap_or_default()
            .servers;
        Self { servers, file }
    }

    /// Writes the servers to their file, as each change is made.
    pub fn save(&self) {
        if let Err(error) = write(&self.file, &self.servers) {
            error!(
                "couldn't save the servers in {}: {error}",
                self.file.display()
            );
        }
    }
}

fn write(file: &Path, servers: &[SavedServer]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(&ServersFile {
        servers: servers.to_vec(),
    })
    .expect("servers always encode");
    write_atomically(file, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_servers_are_kept_in_their_file_in_order() {
        let dir = std::env::temp_dir().join(format!("rustcraft-servers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join(SERVERS_FILE);
        let mut list = ServerList::read(file.clone());
        assert!(list.servers.is_empty());
        for (name, address) in [("Home", "192.168.1.20"), ("Ada's", "ada.example:4000")] {
            list.servers.push(SavedServer {
                name: name.into(),
                address: address.into(),
            });
        }
        list.save();
        assert_eq!(list, ServerList::read(file.clone()));

        list.servers.remove(0);
        list.servers[0].name = "Ada's world".into();
        list.save();
        let read = ServerList::read(file);
        assert_eq!(1, read.servers.len());
        assert_eq!("Ada's world", read.servers[0].name);
        assert_eq!("ada.example:4000", read.servers[0].address);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
