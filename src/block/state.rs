use std::fmt;
use std::str::FromStr;

use bevy::math::{I64Vec3, IVec3};

use super::BlockType;

/// A block's state beyond its type: which way it faces, which half of its cell it fills,
/// whether it's open. Each property has bits of its own in the byte, and every property's
/// first value is 0, so a block with no state, and everything generated, is 0.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BlockState(u8);

/// A property a block's state can have, which its render style declares (`RenderStyle`).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Property {
    /// Which of the four ways round it faces.
    Facing,
    /// Which of the six ways it faces, up and down too.
    FacingAny,
    /// Which axis it lies along, y first, so a log the world grows stands upright.
    Axis,
    /// Whether it's in the top half of its cell, or for a door, its upper block.
    Half,
    /// Whether it hangs from the block above rather than standing on the one below.
    Hanging,
    Open,
    /// Which side a door turns on, as it's faced.
    Hinge,
    /// How far a fluid has flowed from its source, 0 for the source itself (M4.3).
    Level,
    /// Whether a fluid is falling, fed from the fluid over it, which fills its cell.
    Falling,
}

impl Property {
    // Where its bits are in the byte: its lowest bit and how many values it takes.
    const fn bits(self) -> (u8, u8) {
        match self {
            Property::Facing => (0, 4),
            Property::FacingAny => (0, 6),
            Property::Axis => (3, 3),
            Property::Half | Property::Hanging => (5, 2),
            Property::Open => (6, 2),
            Property::Hinge => (7, 2),
            Property::Level => (0, 8),
            Property::Falling => (3, 2),
        }
    }

    const fn mask(self) -> u8 {
        let (shift, values) = self.bits();
        (values.next_power_of_two() - 1) << shift
    }

    /// Its name in a block's text, as in `stone_brick_stairs[facing=east,half=top]`.
    pub fn name(self) -> &'static str {
        match self {
            Property::Facing | Property::FacingAny => "facing",
            Property::Axis => "axis",
            Property::Half => "half",
            Property::Hanging => "hanging",
            Property::Open => "open",
            Property::Hinge => "hinge",
            Property::Level => "level",
            Property::Falling => "falling",
        }
    }

    /// Its values' names, in the order they're numbered.
    pub fn values(self) -> &'static [&'static str] {
        const DIRECTIONS: [&str; 6] = ["north", "east", "south", "west", "up", "down"];
        match self {
            Property::Facing => &DIRECTIONS[..4],
            Property::FacingAny => &DIRECTIONS,
            Property::Axis => &["y", "x", "z"],
            Property::Half => &["bottom", "top"],
            Property::Hanging | Property::Open | Property::Falling => &["false", "true"],
            Property::Hinge => &["left", "right"],
            Property::Level => &["0", "1", "2", "3", "4", "5", "6", "7"],
        }
    }
}

impl BlockState {
    pub fn get(self, property: Property) -> u8 {
        (self.0 & property.mask()) >> property.bits().0
    }

    pub fn with(self, property: Property, value: u8) -> Self {
        debug_assert!(
            value < property.bits().1,
            "{property:?} has no value {value}"
        );
        Self(self.0 & !property.mask() | value << property.bits().0)
    }

    pub fn facing(self) -> Direction {
        Direction::ALL[self.get(Property::FacingAny) as usize]
    }

    pub fn with_facing(self, facing: Direction) -> Self {
        self.with(Property::FacingAny, facing as u8)
    }

    /// The axis it lies along, 0 for x, 1 for y and 2 for z.
    pub fn axis(self) -> usize {
        [1, 0, 2][self.get(Property::Axis) as usize]
    }

    pub fn with_axis(self, axis: usize) -> Self {
        self.with(Property::Axis, [1, 0, 2][axis])
    }

    /// Whether it's in the top half of its cell, a door's upper block, or a lantern hanging.
    pub fn top(self) -> bool {
        self.get(Property::Half) == 1
    }

    pub fn with_top(self, top: bool) -> Self {
        self.with(Property::Half, top as u8)
    }

    pub fn open(self) -> bool {
        self.get(Property::Open) == 1
    }

    pub fn with_open(self, open: bool) -> Self {
        self.with(Property::Open, open as u8)
    }

    pub fn hinge_right(self) -> bool {
        self.get(Property::Hinge) == 1
    }

    pub fn with_hinge_right(self, right: bool) -> Self {
        self.with(Property::Hinge, right as u8)
    }

    /// Whether every bit set belongs to one of `properties`, with a value it has.
    fn fits(self, properties: &[Property]) -> bool {
        let mask = properties.iter().fold(0, |mask, p| mask | p.mask());
        self.0 & !mask == 0 && properties.iter().all(|&p| self.get(p) < p.bits().1)
    }
}

/// Which way a block faces: north is -z and east +x, as the map has them.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Direction {
    North,
    East,
    South,
    West,
    Up,
    Down,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
        Direction::Up,
        Direction::Down,
    ];

    pub fn offset(self) -> IVec3 {
        match self {
            Direction::North => IVec3::NEG_Z,
            Direction::East => IVec3::X,
            Direction::South => IVec3::Z,
            Direction::West => IVec3::NEG_X,
            Direction::Up => IVec3::Y,
            Direction::Down => IVec3::NEG_Y,
        }
    }

    /// The way a unit offset points, if it's one.
    pub fn of_offset(offset: I64Vec3) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|d| d.offset().as_i64vec3() == offset)
    }

    /// Which of the four ways round `forward` points most.
    pub fn horizontal(forward: bevy::math::Vec3) -> Self {
        if forward.x.abs() > forward.z.abs() {
            if forward.x > 0.0 {
                Direction::East
            } else {
                Direction::West
            }
        } else if forward.z > 0.0 {
            Direction::South
        } else {
            Direction::North
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            d => d.turned(2),
        }
    }

    /// Turned `quarters` quarter turns clockwise seen from above, as structures turn (north to
    /// east, +x to +z); up and down stay as they are.
    pub fn turned(self, quarters: u8) -> Self {
        match self {
            Direction::Up | Direction::Down => self,
            d => Direction::ALL[((d as u8 + quarters) % 4) as usize],
        }
    }

    /// The axis it runs along, 0 for x, 1 for y and 2 for z.
    pub fn axis(self) -> usize {
        match self {
            Direction::East | Direction::West => 0,
            Direction::Up | Direction::Down => 1,
            Direction::North | Direction::South => 2,
        }
    }
}

/// A block as it stands in a cell: its type and its state.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Block {
    pub kind: BlockType,
    pub state: BlockState,
}

impl From<BlockType> for Block {
    fn from(kind: BlockType) -> Self {
        Self {
            kind,
            state: BlockState::default(),
        }
    }
}

impl Block {
    pub const AIR: Block = Block {
        kind: BlockType::Air,
        state: BlockState(0),
    };

    /// `kind` in `state`, if it's a state `kind` can be in.
    pub fn new(kind: BlockType, state: BlockState) -> Option<Self> {
        state
            .fits(kind.state_properties())
            .then_some(Self { kind, state })
    }

    /// Turned `quarters` quarter turns clockwise seen from above, as a structure turns it.
    pub fn turned(self, quarters: u8) -> Self {
        let properties = self.kind.state_properties();
        let mut state = self.state;
        if properties.contains(&Property::Facing) || properties.contains(&Property::FacingAny) {
            state = state.with_facing(state.facing().turned(quarters));
        }
        if properties.contains(&Property::Axis) && quarters % 2 == 1 && state.axis() != 1 {
            state = state.with_axis(2 - state.axis());
        }
        Self { state, ..self }
    }
}

// As a template or a save writes it: its name, then the properties not at their first value in
// brackets, `stone_brick_stairs[facing=east,half=top]`, so a block with no state is its name.
impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.name())?;
        let mut set = self
            .kind
            .state_properties()
            .iter()
            .filter(|&&p| self.state.get(p) != 0)
            .peekable();
        if set.peek().is_none() {
            return Ok(());
        }
        let set: Vec<_> = set
            .map(|&p| format!("{}={}", p.name(), p.values()[self.state.get(p) as usize]))
            .collect();
        write!(f, "[{}]", set.join(","))
    }
}

impl FromStr for Block {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (name, properties) = match text.split_once('[') {
            Some((name, rest)) => (
                name,
                rest.strip_suffix(']')
                    .ok_or(format!("\"{text}\" has no closing ]"))?,
            ),
            None => (text, ""),
        };
        let kind = BlockType::from_name(name).ok_or(format!("there's no block called {name}"))?;
        let mut state = BlockState::default();
        for pair in properties.split(',').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair
                .split_once('=')
                .ok_or(format!("\"{pair}\" isn't a property=value"))?;
            let property = kind
                .state_properties()
                .iter()
                .find(|p| p.name() == key)
                .ok_or(format!("{name} has no {key}"))?;
            let value = property
                .values()
                .iter()
                .position(|&v| v == value)
                .ok_or(format!("{name}'s {key} can't be {value}"))?;
            state = state.with(*property, value as u8);
        }
        Ok(Self { kind, state })
    }
}

// By its text, so a block means the same to every save and every game version that knows it,
// as `BlockType`'s name does.
impl serde::Serialize for Block {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Block {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_properties_keep_to_their_own_bits() {
        let state = BlockState::default()
            .with_facing(Direction::West)
            .with_top(true)
            .with_open(true)
            .with_hinge_right(true);
        assert_eq!(Direction::West, state.facing());
        assert!(state.top() && state.open() && state.hinge_right());
        let state = state.with_open(false).with_facing(Direction::East);
        assert_eq!(Direction::East, state.facing());
        assert!(state.top() && !state.open() && state.hinge_right());
        for axis in 0..3 {
            assert_eq!(axis, BlockState::default().with_axis(axis).axis());
        }
        // what the world generates is upright, and has no state at all
        assert_eq!(1, BlockState::default().axis());
        assert_eq!(BlockState(0), BlockState::default().with_axis(1));
    }

    #[test]
    fn test_blocks_read_and_write_their_state_by_name() {
        let stairs: Block = "stone_brick_stairs[facing=east,half=top]".parse().unwrap();
        assert_eq!(BlockType::StoneBrickStairs, stairs.kind);
        assert_eq!(Direction::East, stairs.state.facing());
        assert!(stairs.state.top());
        assert_eq!(
            "stone_brick_stairs[facing=east,half=top]",
            stairs.to_string()
        );
        // first values aren't written, so a block with no state is just its name
        let north: Block = "stone_brick_stairs[facing=north]".parse().unwrap();
        assert_eq!("stone_brick_stairs", north.to_string());
        assert_eq!(Ok(Block::from(BlockType::Stone)), "stone".parse());
        let log: Block = "log[axis=x]".parse().unwrap();
        assert_eq!(0, log.state.axis());
        for bad in [
            "stone[facing=east]",
            "log[axis=w]",
            "stone_brick_stairs[facing=east",
            "mithril",
            "log[axis]",
        ] {
            assert!(bad.parse::<Block>().is_err(), "{bad}");
        }
        let bytes = postcard::to_stdvec(&stairs).unwrap();
        assert_eq!(stairs, postcard::from_bytes(&bytes).unwrap());
    }

    #[test]
    fn test_only_states_a_block_can_be_in_are_made() {
        let east = BlockState::default().with_facing(Direction::East);
        assert!(Block::new(BlockType::Stone, east).is_none());
        assert!(Block::new(BlockType::StoneBrickStairs, east).is_some());
        // four ways round only, for what faces a wall
        let up = BlockState::default().with_facing(Direction::Up);
        assert!(Block::new(BlockType::Ladder, up).is_none());
        assert!(Block::new(BlockType::Barrel, up).is_some());
        assert!(Block::new(
            BlockType::Log,
            BlockState::default().with(Property::Axis, 2)
        )
        .is_some());
        assert!(Block::new(BlockType::Log, BlockState(3 << 3)).is_none());
    }

    #[test]
    fn test_turning_turns_facing_and_axis_and_four_turns_go_all_the_way_round() {
        let stairs: Block = "planks_stairs[facing=north,half=top]".parse().unwrap();
        assert_eq!(
            "planks_stairs[facing=east,half=top]",
            stairs.turned(1).to_string()
        );
        assert_eq!(
            "planks_stairs[facing=west,half=top]",
            stairs.turned(3).to_string()
        );
        let log: Block = "log[axis=x]".parse().unwrap();
        assert_eq!("log[axis=z]", log.turned(1).to_string());
        assert_eq!("log[axis=x]", log.turned(2).to_string());
        let upright = Block::from(BlockType::Log);
        assert_eq!(upright, upright.turned(1));
        let barrel: Block = "barrel[facing=up]".parse().unwrap();
        assert_eq!(barrel, barrel.turned(1));
        for block in [stairs, log, barrel] {
            assert_eq!(block, block.turned(1).turned(1).turned(1).turned(1));
        }
    }
}
