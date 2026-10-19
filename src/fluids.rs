//! Water and lava as fluids (M4.3): what a fluid's block and state say about it, shared by the
//! server, which makes them flow (`server::fluids`), and the client, which draws their surfaces
//! and is carried along by them.
//!
//! A fluid's cell is a source, which stays where it is until something takes it away, or fluid
//! flowing from one, its `Level` the blocks it's come from it, or falling, fed from the fluid
//! over it. Everything the world generates is a source, so the seas stand still until something
//! disturbs them.

use bevy::math::{I64Vec3, Vec3};

use crate::block::{Block, BlockState, BlockType, Property};

/// Of a cell's height, where a source's surface stands, as water's always has (see
/// `generator::shading::WATER_TOP`).
pub const SOURCE_HEIGHT: f32 = 0.9;

/// The four ways round a fluid spreads, in the order it looks at them, so the same flow comes
/// out the same everywhere.
pub const HORIZONTAL: [I64Vec3; 4] = [I64Vec3::NEG_Z, I64Vec3::X, I64Vec3::Z, I64Vec3::NEG_X];

/// A block that flows.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Fluid {
    Water,
    Lava,
}

impl Fluid {
    pub const ALL: [Fluid; 2] = [Fluid::Water, Fluid::Lava];

    pub fn block(self) -> BlockType {
        match self {
            Fluid::Water => BlockType::Water,
            Fluid::Lava => BlockType::Lava,
        }
    }

    /// Its block at `level`.
    pub fn at(self, level: Level) -> Block {
        Block::new(self.block(), level.state()).expect("fluids have levels")
    }

    pub fn source(self) -> Block {
        self.at(Level::Source)
    }

    /// How many of the server's fluid ticks, 20 a second, it waits between steps: water
    /// spreads a block every quarter second and lava every second and a half, as in Minecraft.
    pub fn delay(self) -> u64 {
        match self {
            Fluid::Water => 5,
            Fluid::Lava => 30,
        }
    }

    /// How much weaker it gets each block it flows across, so water runs 7 blocks from its
    /// source and lava 3.
    pub fn drop_off(self) -> u8 {
        match self {
            Fluid::Water => 1,
            Fluid::Lava => 2,
        }
    }

    /// How far across it looks for the nearest way down, to flow that way.
    pub fn slope_reach(self) -> u32 {
        match self {
            Fluid::Water => 4,
            Fluid::Lava => 2,
        }
    }

    /// Whether two sources of it either side of a cell over solid ground make a source there:
    /// water does, so a pool two wide never runs dry, and lava doesn't.
    pub fn makes_sources(self) -> bool {
        self == Fluid::Water
    }

    /// How fast it carries what's in it, in blocks a second, at its steepest.
    pub fn current(self) -> f32 {
        match self {
            Fluid::Water => 1.4,
            Fluid::Lava => 0.5,
        }
    }
}

/// How much of a fluid is in a cell.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Level {
    Source,
    /// Fed from the fluid over it, so it fills its cell and spreads as a source would.
    Falling,
    /// Flowed this many blocks' worth from a source, 1 to 7, weaker the further.
    Flowing(u8),
}

impl Level {
    pub fn of(state: BlockState) -> Self {
        if state.get(Property::Falling) == 1 {
            Level::Falling
        } else {
            match state.get(Property::Level) {
                0 => Level::Source,
                level => Level::Flowing(level),
            }
        }
    }

    pub fn state(self) -> BlockState {
        let state = BlockState::default();
        match self {
            Level::Source => state,
            Level::Falling => state.with(Property::Falling, 1),
            Level::Flowing(level) => state.with(Property::Level, level.clamp(1, 7)),
        }
    }

    pub fn is_source(self) -> bool {
        self == Level::Source
    }

    /// How much fluid there is, in eighths of a source's: Minecraft's "amount", which spreading
    /// takes the fluid's drop-off from.
    pub fn amount(self) -> u8 {
        match self {
            Level::Source | Level::Falling => 8,
            Level::Flowing(level) => 8 - level,
        }
    }

    /// Where its surface stands, as a share of its cell's height: a source's and a fall's
    /// where water's always has, and lower the weaker the flow.
    pub fn height(self) -> f32 {
        SOURCE_HEIGHT * f32::from(self.amount()) / 8.0
    }
}

/// The fluid in `block` and how much of it, if it's water or lava.
pub fn fluid_of(block: Block) -> Option<(Fluid, Level)> {
    block
        .kind
        .fluid()
        .map(|fluid| (fluid, Level::of(block.state)))
}

/// The way the fluid at `at` flows, across, as long as it's steepest: from where its surface
/// stands higher towards where it stands lower, and off any edge it pours over. Nothing for a
/// still pool. `get` is the world's block at a position.
pub fn flow(at: I64Vec3, get: impl Fn(I64Vec3) -> Block) -> Vec3 {
    let Some((fluid, level)) = fluid_of(get(at)) else {
        return Vec3::ZERO;
    };
    let height = |level: Level| match level {
        // a fall pushes nothing aside, standing as high as a source
        Level::Falling => Level::Source.height(),
        level => level.height(),
    };
    let here = height(level);
    let mut flow = Vec3::ZERO;
    for side in HORIZONTAL {
        let beside = get(at + side);
        let there = match fluid_of(beside) {
            Some((other, level)) if other == fluid => height(level),
            // over an edge, as low as it gets
            None if beside.kind.is_replaceable() => 0.0,
            _ => continue,
        };
        flow += side.as_vec3() * (here - there);
    }
    flow.normalize_or_zero()
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, Vec3};

    use super::{flow, fluid_of, Fluid, Level};
    use crate::block::{Block, BlockType};

    #[test]
    fn test_levels_are_kept_in_the_state_and_written_by_name() {
        for level in [
            Level::Source,
            Level::Falling,
            Level::Flowing(1),
            Level::Flowing(7),
        ] {
            for fluid in Fluid::ALL {
                let block = fluid.at(level);
                assert_eq!(Some((fluid, level)), fluid_of(block));
                let text = block.to_string();
                assert_eq!(Ok(block), text.parse::<Block>(), "{text}");
            }
        }
        // a source is the plain block, as the world generates it and old saves have it
        assert_eq!(Block::from(BlockType::Water), Fluid::Water.source());
        assert_eq!(
            "lava[level=3]",
            Fluid::Lava.at(Level::Flowing(3)).to_string()
        );
        assert_eq!(
            "water[falling=true]",
            Fluid::Water.at(Level::Falling).to_string()
        );
        assert_eq!(None, fluid_of(BlockType::Stone.into()));
    }

    #[test]
    fn test_weaker_flows_stand_lower() {
        let heights: Vec<f32> = (1..=7).map(|l| Level::Flowing(l).height()).collect();
        assert!(heights.windows(2).all(|w| w[0] > w[1]), "{heights:?}");
        assert!(Level::Source.height() > heights[0] && heights[6] > 0.0);
    }

    #[test]
    fn test_flow_runs_downhill_and_a_pool_is_still() {
        let water = |level| Fluid::Water.at(level);
        // a source at the origin, flowing water east of it, stone everywhere else
        let get = |at: I64Vec3| match at.to_array() {
            [0, 0, 0] => water(Level::Source),
            [1, 0, 0] => water(Level::Flowing(1)),
            [2, 0, 0] => water(Level::Flowing(2)),
            _ => BlockType::Stone.into(),
        };
        assert_eq!(Vec3::X, flow(I64Vec3::ZERO, get));
        assert_eq!(Vec3::X, flow(I64Vec3::X, get));
        let pool = |at: I64Vec3| match at.y {
            0 => water(Level::Source),
            _ => BlockType::Stone.into(),
        };
        assert_eq!(Vec3::ZERO, flow(I64Vec3::ZERO, pool));
    }
}
