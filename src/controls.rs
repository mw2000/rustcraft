//! What the keys and mouse buttons do: each action the player can take, the key or button bound
//! to it, which the Controls settings change, and reading them.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::de::value::{Error as ValueError, StrDeserializer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// A key or a mouse button, as an action's bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

use Binding::{Key, Mouse};

macro_rules! actions {
    ($($action:ident, $name:literal, $label:literal, $default:expr;)*) => {
        /// Something the player does with a key or a mouse button.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Action {
            $($action,)*
        }

        impl Action {
            /// Every action, in the order the Controls settings list them.
            pub const ALL: &[Action] = &[$(Action::$action,)*];

            /// Its name in the settings file.
            pub fn name(self) -> &'static str {
                match self {
                    $(Action::$action => $name,)*
                }
            }

            /// What it's called in the Controls settings.
            pub fn label(self) -> &'static str {
                match self {
                    $(Action::$action => $label,)*
                }
            }

            /// What it's bound to until the player changes it.
            pub fn default_binding(self) -> Binding {
                match self {
                    $(Action::$action => $default,)*
                }
            }
        }
    };
}

actions! {
    Forward, "forward", "Walk forward", Key(KeyCode::KeyW);
    Back, "back", "Walk back", Key(KeyCode::KeyS);
    Left, "left", "Step left", Key(KeyCode::KeyA);
    Right, "right", "Step right", Key(KeyCode::KeyD);
    Jump, "jump", "Jump, or fly up", Key(KeyCode::Space);
    Descend, "descend", "Fly down", Key(KeyCode::ControlLeft);
    Sprint, "sprint", "Sprint", Key(KeyCode::ShiftLeft);
    Fly, "fly", "Fly or walk (creative)", Key(KeyCode::KeyF);
    Break, "break", "Break a block", Mouse(MouseButton::Left);
    Place, "place", "Place, use or eat", Mouse(MouseButton::Right);
    Pick, "pick", "Pick a block", Mouse(MouseButton::Middle);
    Inventory, "inventory", "Inventory", Key(KeyCode::KeyE);
    Drop, "drop", "Drop the held item", Key(KeyCode::KeyQ);
    Map, "map", "Map", Key(KeyCode::KeyM);
    Chat, "chat", "Chat", Key(KeyCode::KeyT);
    Command, "command", "Type a command", Key(KeyCode::Slash);
    Slot1, "slot_1", "Hotbar slot 1", Key(KeyCode::Digit1);
    Slot2, "slot_2", "Hotbar slot 2", Key(KeyCode::Digit2);
    Slot3, "slot_3", "Hotbar slot 3", Key(KeyCode::Digit3);
    Slot4, "slot_4", "Hotbar slot 4", Key(KeyCode::Digit4);
    Slot5, "slot_5", "Hotbar slot 5", Key(KeyCode::Digit5);
    Slot6, "slot_6", "Hotbar slot 6", Key(KeyCode::Digit6);
    Slot7, "slot_7", "Hotbar slot 7", Key(KeyCode::Digit7);
    Slot8, "slot_8", "Hotbar slot 8", Key(KeyCode::Digit8);
    Slot9, "slot_9", "Hotbar slot 9", Key(KeyCode::Digit9);
    WindBack, "wind_back", "Wind the clock back", Key(KeyCode::BracketLeft);
    WindForward, "wind_forward", "Wind the clock on", Key(KeyCode::BracketRight);
    DebugOverlay, "debug_overlay", "Debug overlay", Key(KeyCode::F1);
}

impl Action {
    /// The hotbar's slots' actions, from the first.
    pub const SLOTS: [Action; 9] = [
        Action::Slot1,
        Action::Slot2,
        Action::Slot3,
        Action::Slot4,
        Action::Slot5,
        Action::Slot6,
        Action::Slot7,
        Action::Slot8,
        Action::Slot9,
    ];
}

/// What each action's bound to.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct Bindings(Vec<Binding>);

impl Default for Bindings {
    fn default() -> Self {
        Self(Action::ALL.iter().map(|a| a.default_binding()).collect())
    }
}

impl Bindings {
    pub fn get(&self, action: Action) -> Binding {
        self.0[action as usize]
    }

    pub fn set(&mut self, action: Action, binding: Binding) {
        self.0[action as usize] = binding;
    }

    /// The other actions bound to the same as `action`, which can't both happen.
    pub fn clashes(&self, action: Action) -> impl Iterator<Item = Action> + '_ {
        let binding = self.get(action);
        Action::ALL
            .iter()
            .copied()
            .filter(move |&other| other != action && self.get(other) == binding)
    }
}

// In the settings file as a table of action names, each bound to a key's name, such as "KeyW",
// or a mouse button's, such as "MouseLeft". One missing, or naming what this game doesn't know,
// keeps its default.
impl Serialize for Bindings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let table: BTreeMap<&str, String> = Action::ALL
            .iter()
            .map(|&action| (action.name(), self.get(action).to_string()))
            .collect();
        table.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Bindings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let table = BTreeMap::<String, String>::deserialize(deserializer)?;
        let mut bindings = Bindings::default();
        for &action in Action::ALL {
            if let Some(binding) = table.get(action.name()).and_then(|s| s.parse().ok()) {
                bindings.set(action, binding);
            }
        }
        Ok(bindings)
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Key(key) => write!(f, "{key:?}"),
            Mouse(MouseButton::Other(n)) => write!(f, "Mouse{n}"),
            Mouse(button) => write!(f, "Mouse{button:?}"),
        }
    }
}

impl std::str::FromStr for Binding {
    type Err = ValueError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let button = match name.strip_prefix("Mouse") {
            Some("Left") => Some(MouseButton::Left),
            Some("Right") => Some(MouseButton::Right),
            Some("Middle") => Some(MouseButton::Middle),
            Some("Back") => Some(MouseButton::Back),
            Some("Forward") => Some(MouseButton::Forward),
            Some(n) => n.parse().ok().map(MouseButton::Other),
            None => None,
        };
        match button {
            Some(button) => Ok(Mouse(button)),
            // as serde names keys, which is how they're written too
            None => KeyCode::deserialize(StrDeserializer::<ValueError>::new(name)).map(Key),
        }
    }
}

impl Binding {
    /// What it's called on screen, such as "W", "Left Shift" or "Right Click".
    pub fn describe(self) -> String {
        let key = match self {
            Mouse(MouseButton::Left) => return "Left Click".into(),
            Mouse(MouseButton::Right) => return "Right Click".into(),
            Mouse(MouseButton::Middle) => return "Middle Click".into(),
            Mouse(MouseButton::Back) => return "Mouse Back".into(),
            Mouse(MouseButton::Forward) => return "Mouse Forward".into(),
            Mouse(MouseButton::Other(n)) => return format!("Mouse {n}"),
            Key(key) => key,
        };
        let symbol = match key {
            KeyCode::Space => "Space",
            KeyCode::Slash => "/",
            KeyCode::Backslash => "\\",
            KeyCode::BracketLeft => "[",
            KeyCode::BracketRight => "]",
            KeyCode::Comma => ",",
            KeyCode::Period => ".",
            KeyCode::Semicolon => ";",
            KeyCode::Quote => "'",
            KeyCode::Backquote => "`",
            KeyCode::Minus => "-",
            KeyCode::Equal => "=",
            KeyCode::ArrowUp => "Up",
            KeyCode::ArrowDown => "Down",
            KeyCode::ArrowLeft => "Left",
            KeyCode::ArrowRight => "Right",
            _ => "",
        };
        if !symbol.is_empty() {
            return symbol.into();
        }
        let name = format!("{key:?}");
        for (prefix, shown) in [("Key", ""), ("Digit", ""), ("Numpad", "Numpad ")] {
            if let Some(rest) = name.strip_prefix(prefix) {
                return format!("{shown}{rest}");
            }
        }
        for (side, shown) in [("Left", "Left "), ("Right", "Right ")] {
            if let Some(rest) = name.strip_suffix(side) {
                let rest = if rest == "Control" { "Ctrl" } else { rest };
                return format!("{shown}{rest}");
            }
        }
        name
    }

    fn pressed(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Key(key) => keys.pressed(key),
            Mouse(button) => mouse.pressed(button),
        }
    }

    fn just_pressed(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Key(key) => keys.just_pressed(key),
            Mouse(button) => mouse.just_pressed(button),
        }
    }
}

/// The keys and mouse buttons held, read by action through what they're bound to.
#[derive(SystemParam)]
pub struct Controls<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    bindings: Res<'w, Bindings>,
}

impl Controls<'_> {
    pub fn pressed(&self, action: Action) -> bool {
        self.bindings.get(action).pressed(&self.keys, &self.mouse)
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        self.bindings
            .get(action)
            .just_pressed(&self.keys, &self.mouse)
    }
}

/// Whether `action`'s binding was pressed this frame, for systems that take the keys mutably.
pub fn just_pressed(
    bindings: &Bindings,
    action: Action,
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
) -> bool {
    bindings.get(action).just_pressed(keys, mouse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bindings_round_trip_by_name_and_keep_defaults_for_what_they_dont_know() {
        let mut bindings = Bindings::default();
        bindings.set(Action::Jump, Key(KeyCode::KeyJ));
        bindings.set(Action::Inventory, Mouse(MouseButton::Other(4)));
        bindings.set(Action::Break, Mouse(MouseButton::Right));
        let text = toml::to_string(&bindings).unwrap();
        assert!(text.contains("jump = \"KeyJ\""), "{text}");
        assert!(text.contains("break = \"MouseRight\""), "{text}");
        assert_eq!(bindings, toml::from_str(&text).unwrap());

        let read: Bindings =
            toml::from_str("jump = \"KeyJ\"\nfly = \"NoSuchKey\"\nteleport = \"KeyX\"").unwrap();
        assert_eq!(Key(KeyCode::KeyJ), read.get(Action::Jump));
        assert_eq!(Action::Fly.default_binding(), read.get(Action::Fly));
    }

    #[test]
    fn test_bindings_are_named_as_a_player_reads_them() {
        assert_eq!("W", Key(KeyCode::KeyW).describe());
        assert_eq!("1", Key(KeyCode::Digit1).describe());
        assert_eq!("Left Shift", Key(KeyCode::ShiftLeft).describe());
        assert_eq!("Left Ctrl", Key(KeyCode::ControlLeft).describe());
        assert_eq!("[", Key(KeyCode::BracketLeft).describe());
        assert_eq!("F1", Key(KeyCode::F1).describe());
        assert_eq!("Right Click", Mouse(MouseButton::Right).describe());
    }

    #[test]
    fn test_actions_bound_alike_clash() {
        let mut bindings = Bindings::default();
        assert_eq!(0, bindings.clashes(Action::Jump).count());
        bindings.set(Action::Map, Key(KeyCode::KeyE));
        assert_eq!(
            vec![Action::Map],
            bindings.clashes(Action::Inventory).collect::<Vec<_>>()
        );
    }
}
