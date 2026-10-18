use bevy::ecs::component::Component;
use serde::{Deserialize, Serialize};

/// The longest a name can be, in characters: long enough for anyone's, short enough for a label
/// over their head.
pub const MAX_NAME_LENGTH: usize = 16;

/// A player's name, as others see it over their head and their save is named by: up to 16
/// letters, digits, `_` and `-`, so it makes a file name anywhere.
#[derive(Component, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PlayerName(String);

impl PlayerName {
    /// `name` with anything a name can't have left out, or "Player" if that leaves nothing.
    pub fn new(name: &str) -> Self {
        let kept: String = name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(MAX_NAME_LENGTH)
            .collect();
        Self(if kept.is_empty() {
            "Player".into()
        } else {
            kept
        })
    }

    /// The name this game plays as: `RC_NAME`, or else the user's on this computer.
    pub fn own() -> Self {
        let name = ["RC_NAME", "USER", "USERNAME"]
            .into_iter()
            .find_map(|variable| std::env::var(variable).ok());
        Self::new(name.as_deref().unwrap_or_default())
    }

    /// This name, or if one of `taken` has it already, the first with a number after it that
    /// none has. Names differing only in case are taken to be the same, as saves are named by
    /// them and some file systems don't tell them apart.
    pub fn unique_among<'a>(self, taken: impl Iterator<Item = &'a PlayerName> + Clone) -> Self {
        let free = |name: &str| {
            !taken
                .clone()
                .any(|other| other.0.eq_ignore_ascii_case(name))
        };
        if free(&self.0) {
            return self;
        }
        let mut numbered = (2..).map(|n| format!("{}{n}", self.0));
        Self(numbered.find(|name| free(name)).unwrap())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerName;

    #[test]
    fn test_names_keep_only_what_a_file_name_can_have() {
        assert_eq!("Mihir", PlayerName::new("Mihir").as_str());
        assert_eq!("etcpasswd", PlayerName::new("../etc/passwd").as_str());
        assert_eq!("Player", PlayerName::new(" /.\\ ").as_str());
        assert_eq!("Zoë_2-b", PlayerName::new("Zoë_2-b").as_str());
        assert_eq!(16, PlayerName::new(&"a".repeat(40)).as_str().len());
    }

    #[test]
    fn test_a_name_taken_gets_the_first_number_free() {
        let taken = [PlayerName::new("alex"), PlayerName::new("Alex2")];
        let unique = |name| PlayerName::new(name).unique_among(taken.iter());
        assert_eq!("sam", unique("sam").as_str());
        assert_eq!("ALEX3", unique("ALEX").as_str());
    }
}
