//! The commands typed into the command bar (`client::command_bar`), which the server runs
//! (`server::commands`).

use bevy::math::Vec3;

use crate::game_mode::GameMode;

// Each way to type a command as `/help` lists it, what it does, and whether only operators may
// run it: those that change the world or other players. The clock's keys are listed too, as
// they wind everyone's.
const USAGES: [(&str, &str, bool); 17] = [
    (
        "/tp <x> <y> <z>",
        "teleport there; ~ means where you are, ~10 ten blocks on from it",
        true,
    ),
    ("/tp <x> <z>", "teleport above the ground there", true),
    ("/tp <player>", "teleport to a player", true),
    ("/tp <player> <x> <y> <z>", "teleport a player there", true),
    (
        "/gamemode survival|creative [player]",
        "switch your game mode, or a player's; s or c for short",
        true,
    ),
    ("/op <player>", "let a player run these commands too", true),
    ("/deop <player>", "stop a player running them", true),
    (
        "/pos1 [<x> <y> <z>]",
        "select a box from this corner: the block there, looked at or stood in",
        true,
    ),
    ("/pos2 [<x> <y> <z>]", "select a box to this corner", true),
    (
        "/structure save <name> [floor]",
        "save the box as a structure, the layer from its bottom at ground level its floor",
        true,
    ),
    (
        "/structure place <name> [0|90|180|270]",
        "build a structure on the block looked at, turned, and select it",
        true,
    ),
    (
        "/structure list",
        "list the structures there are to place",
        true,
    ),
    (
        "[ and ]",
        "wind the clock back and forward, for everyone",
        true,
    ),
    ("/seed", "print the world's seed", false),
    ("/list", "list the players here", false),
    (
        "/msg <player> <text>",
        "say something only that player sees",
        false,
    ),
    ("/help", "print this", false),
];

/// What `/help` prints, for an operator if `op`, or else only the commands anyone may run.
pub(crate) fn help(op: bool) -> String {
    usages(op) + "\nAnything typed without a / is said to everyone"
}

/// The commands an operator may run if `op`, or else those anyone may, each with what it does.
pub(crate) fn usages(op: bool) -> String {
    let mut usages = String::from("Commands:");
    for (usage, does, ops_only) in USAGES {
        if op || !ops_only {
            usages += &format!("\n{usage}  {does}");
        }
    }
    usages
}

// Whether only operators may run the command called `name`.
fn ops_only(name: &str) -> bool {
    USAGES.iter().any(|&(usage, _, ops_only)| {
        let command = usage
            .strip_prefix('/')
            .and_then(|usage| usage.split(' ').next());
        ops_only && command == Some(name)
    })
}

/// A command, as `parse` reads it. A player it names is as typed, to be found among those here.
#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    /// Sends `who`, or else whoever ran it, `to` somewhere.
    Teleport {
        who: Option<String>,
        to: Destination,
    },
    Seed,
    GameMode {
        mode: GameMode,
        who: Option<String>,
    },
    Help,
    List,
    Message {
        to: String,
        text: String,
    },
    Op(String),
    Deop(String),
    /// Moves corner `corner`, 0 or 1, of the selection to the block at `at`, or else to the one
    /// looked at or stood in.
    Corner {
        corner: usize,
        at: Option<Vec3>,
    },
    /// Saves the selection as the structure `name`, with the layer `floor` from its bottom at
    /// ground level, or else the floor of what was placed in it, or its bottom.
    SaveStructure {
        name: String,
        floor: Option<i64>,
    },
    /// Builds the structure `name` turned `rotation` quarter turns.
    PlaceStructure {
        name: String,
        rotation: u8,
    },
    ListStructures,
}

/// Where `/tp` goes.
#[derive(Debug, PartialEq)]
pub(crate) enum Destination {
    Exact(Vec3),
    // above the ground at x, z
    Surface { x: f32, z: f32 },
    // where the player named is
    Player(String),
}

/// Reads a line typed into the bar, by a player standing at `from`, who's an operator if `op`.
/// A command only operators may run is refused to anyone else before its arguments are read.
pub(crate) fn parse(line: &str, from: Vec3, op: bool) -> Result<Command, String> {
    let line = line.trim();
    let Some(line) = line.strip_prefix('/') else {
        return Err("Commands start with /, try /help".into());
    };
    let (name, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let name = if name == "teleport" { "tp" } else { name };
    let args: Vec<&str> = rest.split_whitespace().collect();
    if ops_only(name) && !op {
        return Err(format!(
            "You don't have permission to use /{name}, only operators do"
        ));
    }
    let exact = |x, y, z| -> Result<_, String> {
        Ok(Destination::Exact(Vec3::new(
            coordinate(x, from.x)?,
            coordinate(y, from.y)?,
            coordinate(z, from.z)?,
        )))
    };
    match name {
        "tp" => match args[..] {
            [x, y, z] => Ok(Command::Teleport {
                who: None,
                to: exact(x, y, z)?,
            }),
            [x, z] => Ok(Command::Teleport {
                who: None,
                to: Destination::Surface {
                    x: coordinate(x, from.x)?,
                    z: coordinate(z, from.z)?,
                },
            }),
            [player] => Ok(Command::Teleport {
                who: None,
                to: Destination::Player(player.into()),
            }),
            // ~ is still where whoever ran it is, as it is in Minecraft
            [player, x, y, z] => Ok(Command::Teleport {
                who: Some(player.into()),
                to: exact(x, y, z)?,
            }),
            _ => Err(
                "Usage: /tp <x> <y> <z>, /tp <x> <z>, /tp <player> or /tp <player> <x> <y> <z>"
                    .into(),
            ),
        },
        "seed" => Ok(Command::Seed),
        "gamemode" => {
            let mode = match args.first() {
                Some(&("survival" | "s")) => Some(GameMode::Survival),
                Some(&("creative" | "c")) => Some(GameMode::Creative),
                _ => None,
            };
            match (mode, &args[..]) {
                (Some(mode), [_]) => Ok(Command::GameMode { mode, who: None }),
                (Some(mode), [_, player]) => Ok(Command::GameMode {
                    mode,
                    who: Some((*player).into()),
                }),
                _ => Err("Usage: /gamemode survival|creative [player]".into()),
            }
        }
        "op" => match args[..] {
            [player] => Ok(Command::Op(player.into())),
            _ => Err("Usage: /op <player>".into()),
        },
        "deop" => match args[..] {
            [player] => Ok(Command::Deop(player.into())),
            _ => Err("Usage: /deop <player>".into()),
        },
        "list" => Ok(Command::List),
        "msg" => {
            // the text as typed, spaces and all
            let (to, text) = rest
                .trim()
                .split_once(char::is_whitespace)
                .unwrap_or(("", ""));
            match text.trim() {
                "" => Err("Usage: /msg <player> <text>".into()),
                text => Ok(Command::Message {
                    to: to.into(),
                    text: text.into(),
                }),
            }
        }
        "pos1" | "pos2" => {
            let corner = usize::from(name == "pos2");
            match args[..] {
                [] => Ok(Command::Corner { corner, at: None }),
                [x, y, z] => {
                    let at = Vec3::new(
                        coordinate(x, from.x)?,
                        coordinate(y, from.y)?,
                        coordinate(z, from.z)?,
                    );
                    Ok(Command::Corner {
                        corner,
                        at: Some(at),
                    })
                }
                _ => Err(format!("Usage: /{name} [<x> <y> <z>]")),
            }
        }
        "structure" => match args[..] {
            ["save", name] => Ok(Command::SaveStructure {
                name: structure_name(name)?,
                floor: None,
            }),
            ["save", name, floor] => {
                let floor = floor.parse().ok().filter(|&floor: &i64| floor >= 0);
                Ok(Command::SaveStructure {
                    name: structure_name(name)?,
                    floor: Some(floor.ok_or("The floor is a layer, from 0 at the bottom")?),
                })
            }
            ["place", name] => Ok(Command::PlaceStructure {
                name: structure_name(name)?,
                rotation: 0,
            }),
            ["place", name, turn] => {
                let rotation = ["0", "90", "180", "270"].iter().position(|&t| t == turn);
                Ok(Command::PlaceStructure {
                    name: structure_name(name)?,
                    rotation: rotation.ok_or("A structure turns 0, 90, 180 or 270 degrees")? as u8,
                })
            }
            ["list"] => Ok(Command::ListStructures),
            _ => Err(
                "Usage: /structure save <name> [floor], /structure place <name> [0|90|180|270] \
                 or /structure list"
                    .into(),
            ),
        },
        "help" => Ok(Command::Help),
        "" => Err("Type a command after the /, try /help".into()),
        other => Err(format!("Unknown command /{other}, try /help")),
    }
}

// The longest a structure's name can be, which is plenty.
const MAX_STRUCTURE_NAME: usize = 32;

// A structure's name as typed, which names its file, so it's kept to what a block's name can be.
fn structure_name(name: &str) -> Result<String, String> {
    let fits = name.len() <= MAX_STRUCTURE_NAME
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !fits {
        return Err(format!(
            "A structure's name is up to {MAX_STRUCTURE_NAME} lower-case letters, digits and \
             underscores, starting with a letter, not {name}"
        ));
    }
    Ok(name.into())
}

// A number, or ~ for `here`, or ~n for n on from it.
fn coordinate(word: &str, here: f32) -> Result<f32, String> {
    let number = |text: &str| {
        text.parse::<f32>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("{word} isn't a number"))
    };
    match word.strip_prefix('~') {
        Some("") => Ok(here),
        Some(offset) => Ok(here + number(offset)?),
        None => number(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: Vec3 = Vec3::new(10.0, 64.0, -20.0);

    // As an operator reads it.
    fn parse(line: &str, from: Vec3) -> Result<Command, String> {
        super::parse(line, from, true)
    }

    fn exact(x: f32, y: f32, z: f32) -> Command {
        Command::Teleport {
            who: None,
            to: Destination::Exact(Vec3::new(x, y, z)),
        }
    }

    #[test]
    fn test_teleports_to_exact_coordinates() {
        assert_eq!(
            Ok(exact(100.0, 80.5, -3000.0)),
            parse("/tp 100 80.5 -3000", HERE)
        );
        assert_eq!(
            Ok(exact(0.0, 0.0, 0.0)),
            parse("  /teleport   0 0 0  ", HERE)
        );
    }

    #[test]
    fn test_tilde_is_relative_to_where_you_are() {
        assert_eq!(Ok(exact(10.0, 74.0, -25.5)), parse("/tp ~ ~10 ~-5.5", HERE));
    }

    #[test]
    fn test_two_coordinates_teleport_above_the_ground() {
        assert_eq!(
            Ok(Command::Teleport {
                who: None,
                to: Destination::Surface { x: 500.0, z: -20.0 }
            }),
            parse("/tp 500 ~", HERE)
        );
    }

    #[test]
    fn test_teleports_to_and_of_other_players() {
        assert_eq!(
            Ok(Command::Teleport {
                who: None,
                to: Destination::Player("alex".into())
            }),
            parse("/tp alex", HERE)
        );
        assert_eq!(
            Ok(Command::Teleport {
                who: Some("alex".into()),
                to: Destination::Exact(Vec3::new(1.0, 74.0, 3.0))
            }),
            parse("/tp alex 1 ~10 3", HERE)
        );
    }

    #[test]
    fn test_bad_teleports_are_errors() {
        assert!(parse("/tp", HERE).is_err());
        assert!(parse("/tp 1 2 3 4 5", HERE).is_err());
        assert!(parse("/tp a 2 3", HERE).is_err());
        assert!(parse("/tp alex 1 2 z", HERE).is_err());
        assert!(parse("/tp ~x 2 3", HERE).is_err());
        assert!(parse("/tp NaN 2 3", HERE).is_err());
        assert!(parse("/tp inf 2 3", HERE).is_err());
    }

    #[test]
    fn test_other_commands() {
        assert_eq!(Ok(Command::Seed), parse("/seed", HERE));
        assert_eq!(Ok(Command::Help), parse("/help", HERE));
        assert_eq!(Ok(Command::List), parse("/list", HERE));
        assert_eq!(Ok(Command::Op("sam".into())), parse("/op sam", HERE));
        assert_eq!(Ok(Command::Deop("sam".into())), parse("/deop sam", HERE));
        assert!(parse("/op", HERE).is_err());
        assert!(parse("/deop sam alex", HERE).is_err());
        assert!(parse("/fly", HERE)
            .unwrap_err()
            .contains("Unknown command /fly"));
        assert!(parse("tp 1 2 3", HERE).is_err());
        assert!(parse("/", HERE).is_err());
    }

    #[test]
    fn test_a_message_keeps_its_text_as_typed() {
        assert_eq!(
            Ok(Command::Message {
                to: "sam".into(),
                text: "meet  at the tower".into()
            }),
            parse("/msg sam   meet  at the tower ", HERE)
        );
        assert!(parse("/msg sam", HERE).is_err());
        assert!(parse("/msg", HERE).is_err());
    }

    #[test]
    fn test_gamemode_takes_a_mode_by_name_or_initial_and_a_player() {
        for (word, mode) in [
            ("survival", GameMode::Survival),
            ("s", GameMode::Survival),
            ("creative", GameMode::Creative),
            ("c", GameMode::Creative),
        ] {
            assert_eq!(
                Ok(Command::GameMode { mode, who: None }),
                parse(&format!("/gamemode {word}"), HERE)
            );
        }
        assert_eq!(
            Ok(Command::GameMode {
                mode: GameMode::Creative,
                who: Some("sam".into())
            }),
            parse("/gamemode c sam", HERE)
        );
        assert!(parse("/gamemode", HERE).is_err());
        assert!(parse("/gamemode adventure", HERE).is_err());
        assert!(parse("/gamemode s sam alex", HERE).is_err());
    }

    #[test]
    fn test_structure_authoring_commands() {
        let corner = |corner, at| Ok(Command::Corner { corner, at });
        assert_eq!(corner(0, None), parse("/pos1", HERE));
        assert_eq!(
            corner(1, Some(Vec3::new(1.0, 66.0, -20.0))),
            parse("/pos2 1 ~2 ~", HERE)
        );
        assert!(parse("/pos1 1 2", HERE).is_err());
        assert!(parse("/pos3", HERE).is_err());
        let save = |name: &str, floor| {
            let name = name.into();
            Ok(Command::SaveStructure { name, floor })
        };
        assert_eq!(
            save("tower_2", None),
            parse("/structure save tower_2", HERE)
        );
        assert_eq!(save("hut", Some(1)), parse("/structure save hut 1", HERE));
        assert!(parse("/structure save hut -1", HERE).is_err());
        let place = |rotation| {
            let name = "hut".into();
            Ok(Command::PlaceStructure { name, rotation })
        };
        assert_eq!(place(0), parse("/structure place hut", HERE));
        assert_eq!(place(3), parse("/structure place hut 270", HERE));
        assert!(parse("/structure place hut 45", HERE).is_err());
        assert_eq!(Ok(Command::ListStructures), parse("/structure list", HERE));
        assert!(parse("/structure", HERE).is_err());
        // names name files, so they're kept to what's safe in a path
        for name in ["../hut", "Hut", "_hut", "h.t", &"a".repeat(33)] {
            let line = format!("/structure save {name}");
            assert!(parse(&line, HERE).is_err(), "{name}");
        }
    }

    #[test]
    fn test_only_operators_may_change_things_and_are_told_of_them() {
        let refused = super::parse("/gamemode", HERE, false).unwrap_err();
        assert!(refused.contains("permission to use /gamemode"), "{refused}");
        for line in [
            "/tp 1 2 3",
            "/teleport 1 2 3",
            "/op sam",
            "/deop sam",
            "/pos1",
            "/structure list",
        ] {
            assert!(super::parse(line, HERE, false).is_err(), "{line}");
        }
        for line in ["/seed", "/list", "/msg sam hi", "/help"] {
            assert!(super::parse(line, HERE, false).is_ok(), "{line}");
        }
        let (anyone, op) = (help(false), help(true));
        assert!(anyone.contains("/msg") && !anyone.contains("/tp") && !anyone.contains("/op"));
        assert!(op.contains("/msg") && op.contains("/tp") && op.contains("/deop"));
        assert!(op.contains("[ and ]") && !anyone.contains("[ and ]"));
        let empty = super::parse("/", HERE, false).unwrap_err();
        assert!(empty.contains("Type a command"), "{empty}");
    }
}
