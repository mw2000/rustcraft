//! Water and lava flowing (M4.3, INV-4): the server steps them on a clock of its own, 20 ticks a
//! second, as edits through the edit layer, so they're saved and sent as any edit is.
//!
//! Nothing is scanned. A fluid flows only once a change beside it wakes it (`World::
//! take_fluid_wakes`): a block broken by the sea, a bucket poured, or the fluid's own last
//! step. Woken, a cell is stepped its fluid's delay later: it works out its level from what
//! feeds it, as Minecraft's does, and spreads down if it can, and otherwise across towards the
//! nearest way down within its reach, or every way if there's none. The world loads still,
//! as everything generated is a source and loading wakes nothing.
//!
//! Only so many cells are stepped a tick, the earliest due first, and the rest wait for the
//! next, so a dam broken into a cavern floods it over a few seconds more rather than stalling
//! the server. A cell beside a chunk that isn't loaded waits for it to be, rather than flowing
//! into nothing or drying up for want of what feeds it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;

use super::sync::BlockChanges;
use crate::block::{Block, BlockType};
use crate::chunks::chunk::ChunkCoordinate;
use crate::fluids::{fluid_of, Fluid, Level, HORIZONTAL};
use crate::world::World;

/// How long a fluid tick is: 20 a second, the server's clock in Minecraft, which fluids' delays
/// are counted in (`Fluid::delay`).
pub const FLUID_TICK: Duration = Duration::from_millis(50);
/// How many cells are stepped a tick at most. Each costs up to a few tens of microseconds,
/// most of it seeking the nearest way down, and an edit, so this holds a flood to a few
/// milliseconds a tick, see §5.4a.
pub const STEPS_PER_TICK: usize = 256;
// How many ticks a frame can catch up on after a slow one; beyond that the fluids slow down
// rather than the frame.
const CATCH_UP: u32 = 3;

/// The fluids waiting to flow, and the clock they flow by.
#[derive(Resource)]
pub struct Fluids {
    /// Ticks since the world opened.
    tick: u64,
    // time towards the next tick
    owed: Duration,
    // the cells to step, each with the tick it's due, in a queue for each fluid: as each has
    // one delay, a queue's cells fall due in the order they're added
    queues: [VecDeque<(u64, I64Vec3)>; 2],
    // the cells in the queues, so none is in twice
    queued: HashSet<I64Vec3>,
    // cells that look at a chunk not loaded, by that chunk, to step once it is
    waiting: BTreeMap<[i64; 3], BTreeSet<[i64; 3]>>,
    // the blocks changed since they were last sent
    changed: BTreeSet<[i64; 3]>,
    /// How many cells are stepped a tick at most.
    pub steps_per_tick: usize,
}

impl Default for Fluids {
    fn default() -> Self {
        Self {
            tick: 0,
            owed: Duration::ZERO,
            queues: Default::default(),
            queued: HashSet::new(),
            waiting: BTreeMap::new(),
            changed: BTreeSet::new(),
            steps_per_tick: STEPS_PER_TICK,
        }
    }
}

fn queue_of(fluid: Fluid) -> usize {
    fluid as usize
}

impl Fluids {
    /// How many cells are waiting to be stepped, due or not, and for chunks to load.
    pub fn pending(&self) -> usize {
        self.queued.len() + self.waiting.values().map(BTreeSet::len).sum::<usize>()
    }

    /// Takes in what's changed in `world` since this was last called: the fluids in and beside
    /// each block changed are stepped their delay from now, and lava water's touched hardens at
    /// once, as it would before anything else flowed.
    pub fn wake(&mut self, world: &mut World) {
        loop {
            let woken = world.take_fluid_wakes();
            if woken.is_empty() {
                return;
            }
            for block in woken {
                let around = crate::block::Direction::ALL.map(|d| block + d.offset().as_i64vec3());
                for at in std::iter::once(block).chain(around) {
                    let Some((fluid, level)) = fluid_of(world.get(at)) else {
                        continue;
                    };
                    if !(fluid == Fluid::Lava && self.harden(world, at, level)) {
                        self.queue(at, fluid);
                    }
                }
            }
        }
    }

    fn queue(&mut self, at: I64Vec3, fluid: Fluid) {
        if self.queued.insert(at) {
            let due = self.tick + fluid.delay();
            self.queues[queue_of(fluid)].push_back((due, at));
        }
    }

    /// Runs the fluids' clock on by `elapsed`, stepping the ticks that come round.
    pub fn run(&mut self, world: &mut World, elapsed: Duration) {
        self.owed += elapsed;
        let mut ticks = 0;
        while self.owed >= FLUID_TICK {
            self.owed -= FLUID_TICK;
            ticks += 1;
            if ticks > CATCH_UP {
                self.owed = Duration::ZERO;
                break;
            }
            self.step(world);
        }
    }

    /// One tick: the cells due are stepped, earliest first, as many as `steps_per_tick`, and
    /// what they change wakes the fluids beside it for later ticks.
    pub fn step(&mut self, world: &mut World) {
        self.tick += 1;
        self.thaw(world);
        for _ in 0..self.steps_per_tick {
            let due = |queue: &VecDeque<(u64, I64Vec3)>| {
                queue
                    .front()
                    .map(|&(due, _)| due)
                    .filter(|&due| due <= self.tick)
            };
            let next = match (due(&self.queues[0]), due(&self.queues[1])) {
                (Some(water), Some(lava)) if lava < water => 1,
                (Some(_), _) => 0,
                (None, Some(_)) => 1,
                (None, None) => break,
            };
            let (_, at) = self.queues[next].pop_front().expect("a cell due");
            self.queued.remove(&at);
            self.update(world, at);
        }
        self.wake(world);
    }

    /// The blocks the fluids have changed since this was last called, as they are now, by
    /// chunk, to be sent.
    pub fn take_changed(&mut self, world: &World) -> Vec<(ChunkCoordinate, Vec<(I64Vec3, Block)>)> {
        let mut chunks: BTreeMap<[i64; 3], Vec<(I64Vec3, Block)>> = BTreeMap::new();
        for at in std::mem::take(&mut self.changed) {
            let at = I64Vec3::from_array(at);
            let chunk = ChunkCoordinate::containing(at).0.to_array();
            chunks.entry(chunk).or_default().push((at, world.get(at)));
        }
        chunks
            .into_iter()
            .map(|(chunk, changes)| (ChunkCoordinate(I64Vec3::from_array(chunk)), changes))
            .collect()
    }

    // Queues again the cells waiting for chunks that have loaded.
    fn thaw(&mut self, world: &World) {
        let loaded: Vec<[i64; 3]> = self
            .waiting
            .keys()
            .copied()
            .filter(|&chunk| world.is_chunk_generated(ChunkCoordinate(I64Vec3::from_array(chunk))))
            .collect();
        for chunk in loaded {
            for at in self.waiting.remove(&chunk).into_iter().flatten() {
                let at = I64Vec3::from_array(at);
                if let Some((fluid, _)) = fluid_of(world.get(at)) {
                    self.queue(at, fluid);
                }
            }
        }
    }

    fn set(&mut self, world: &mut World, at: I64Vec3, to: Block) {
        if world.edit(at, to) {
            self.changed.insert(at.to_array());
        }
    }

    // Steps the fluid at `at`, if there's still one there: works out its level again, unless
    // it's a source, and spreads it.
    fn update(&mut self, world: &mut World, at: I64Vec3) {
        // everything it looks at but for the ways down it seeks must be loaded
        let unloaded = std::iter::once(at)
            .chain(crate::block::Direction::ALL.map(|d| at + d.offset().as_i64vec3()))
            .map(ChunkCoordinate::containing)
            .find(|&chunk| !world.is_chunk_generated(chunk));
        if let Some(chunk) = unloaded {
            self.waiting
                .entry(chunk.0.to_array())
                .or_default()
                .insert(at.to_array());
            return;
        }
        let Some((fluid, mut level)) = fluid_of(world.get(at)) else {
            return;
        };
        if fluid == Fluid::Lava && self.harden(world, at, level) {
            return;
        }
        if !level.is_source() {
            match fed_level(world, at, fluid) {
                None => {
                    self.set(world, at, Block::AIR);
                    return;
                }
                Some(fed) if fed != level => {
                    level = fed;
                    self.set(world, at, fluid.at(level));
                }
                Some(_) => {}
            }
        }
        self.spread(world, at, fluid, level);
    }

    // Hardens the lava at `at` if water's over it or beside it: a source into obsidian, and
    // flowing lava into cobblestone. Returns whether it did.
    fn harden(&mut self, world: &mut World, at: I64Vec3, level: Level) -> bool {
        let touched = std::iter::once(I64Vec3::Y)
            .chain(HORIZONTAL)
            .any(|side| world.get_block_at(at + side) == BlockType::Water);
        if touched {
            let into = if level.is_source() {
                BlockType::Obsidian
            } else {
                BlockType::Cobblestone
            };
            self.set(world, at, into.into());
        }
        touched
    }

    // Spreads `fluid` at `level` from `at`: down if it can, and across too from a source or
    // where three sources meet; and otherwise across, unless it's over its own fluid or a drop.
    fn spread(&mut self, world: &mut World, at: I64Vec3, fluid: Fluid, level: Level) {
        let below = at - I64Vec3::Y;
        if can_flow_into(world, below, fluid, true) {
            self.flow_into(world, below, fluid, Level::Falling, true);
            if sources_beside(world, at, fluid) >= 3 {
                self.spread_across(world, at, fluid, level);
            }
        } else if level.is_source() || !is_drop(world, below, fluid) {
            self.spread_across(world, at, fluid, level);
        }
    }

    fn spread_across(&mut self, world: &mut World, at: I64Vec3, fluid: Fluid, level: Level) {
        let Some(amount) = level
            .amount()
            .checked_sub(fluid.drop_off())
            .filter(|&a| a > 0)
        else {
            return;
        };
        for side in ways(world, at, fluid) {
            let to = at + side;
            if can_flow_into(world, to, fluid, false) {
                self.flow_into(world, to, fluid, Level::Flowing(8 - amount), false);
            }
        }
    }

    // Puts `fluid` at `level` at `to`, or stone where lava falls onto water.
    fn flow_into(
        &mut self,
        world: &mut World,
        to: I64Vec3,
        fluid: Fluid,
        level: Level,
        down: bool,
    ) {
        let block = if fluid == Fluid::Lava && down && world.get_block_at(to) == BlockType::Water {
            BlockType::Stone.into()
        } else {
            fluid.at(level)
        };
        self.set(world, to, block);
    }
}

// The block at `at`, or none in a chunk not loaded, which fluids treat as a wall.
fn loaded(world: &World, at: I64Vec3) -> Option<Block> {
    world
        .is_chunk_generated(ChunkCoordinate::containing(at))
        .then(|| world.get(at))
}

// What the fluid at `at`, not a source, should be from what feeds it: a source between two
// sources over solid ground or a source, if it makes them; falling under its fluid; and
// otherwise as strong as the strongest beside it, less its drop-off, or nothing at all.
fn fed_level(world: &World, at: I64Vec3, fluid: Fluid) -> Option<Level> {
    let (mut most, mut sources) = (0, 0);
    for side in HORIZONTAL {
        if let Some((beside, level)) = fluid_of(world.get(at + side)) {
            if beside == fluid {
                sources += usize::from(level.is_source());
                most = most.max(level.amount());
            }
        }
    }
    if fluid.makes_sources() && sources >= 2 {
        let below = world.get(at - I64Vec3::Y);
        if below.kind.is_solid() || fluid_of(below) == Some((fluid, Level::Source)) {
            return Some(Level::Source);
        }
    }
    if fluid_of(world.get(at + I64Vec3::Y)).is_some_and(|(over, _)| over == fluid) {
        return Some(Level::Falling);
    }
    let amount = most.saturating_sub(fluid.drop_off());
    (amount > 0).then_some(Level::Flowing(8 - amount))
}

// Whether `fluid` can flow into `to`: what's there is something fluid washes away, such as air
// or a plant, or it's lava falling onto water, which it turns to stone.
fn can_flow_into(world: &World, to: I64Vec3, fluid: Fluid, down: bool) -> bool {
    loaded(world, to).is_some_and(|block| match block.kind.fluid() {
        None => block.kind.is_replaceable(),
        Some(there) => fluid == Fluid::Lava && there == Fluid::Water && down,
    })
}

// Whether `fluid` over `below` would flow down into it, or has.
fn is_drop(world: &World, below: I64Vec3, fluid: Fluid) -> bool {
    can_flow_into(world, below, fluid, true)
        || loaded(world, below).is_some_and(|block| block.kind.fluid() == Some(fluid))
}

// Whether `fluid` can pass through `at` looking for a way down: anything it can flow into or
// has, but its own sources.
fn passes(world: &World, at: I64Vec3, fluid: Fluid) -> bool {
    loaded(world, at).is_some_and(|block| {
        block.kind.is_replaceable() && fluid_of(block) != Some((fluid, Level::Source))
    })
}

fn sources_beside(world: &World, at: I64Vec3, fluid: Fluid) -> usize {
    HORIZONTAL
        .into_iter()
        .filter(|&side| fluid_of(world.get(at + side)) == Some((fluid, Level::Source)))
        .count()
}

// The ways across `fluid` spreads from `at`: those towards the nearest way down within its
// reach, or every way it can if there's none.
fn ways(world: &World, at: I64Vec3, fluid: Fluid) -> Vec<I64Vec3> {
    let mut nearest = u32::MAX;
    let mut ways = Vec::new();
    for side in HORIZONTAL {
        let beside = at + side;
        if !passes(world, beside, fluid) {
            continue;
        }
        let distance = if is_drop(world, beside - I64Vec3::Y, fluid) {
            0
        } else {
            distance_down(world, beside, 1, -side, fluid)
        };
        if distance < nearest {
            ways.clear();
            nearest = distance;
        }
        if distance == nearest {
            ways.push(side);
        }
    }
    ways
}

// How many blocks across from `at`, `distance` out already, the nearest way down is, not
// going back the way it came, `from`, and looking no further than `fluid`'s reach.
fn distance_down(world: &World, at: I64Vec3, distance: u32, from: I64Vec3, fluid: Fluid) -> u32 {
    let mut nearest = u32::MAX;
    for side in HORIZONTAL.into_iter().filter(|&side| side != from) {
        let beside = at + side;
        if !passes(world, beside, fluid) {
            continue;
        }
        if is_drop(world, beside - I64Vec3::Y, fluid) {
            return distance;
        }
        if distance < fluid.slope_reach() {
            nearest = nearest.min(distance_down(world, beside, distance + 1, -side, fluid));
        }
    }
    nearest
}

/// Steps the fluids each fluid tick, waking them by the changes made this frame, and sends
/// what they change, a message a chunk.
pub(super) fn flow(
    time: Res<Time>,
    mut world: ResMut<World>,
    mut fluids: ResMut<Fluids>,
    mut changes: BlockChanges,
) {
    fluids.wake(&mut world);
    fluids.run(&mut world, time.delta());
    for (chunk, blocks) in fluids.take_changed(&world) {
        changes.write_chunk(chunk, &blocks);
    }
}

#[cfg(test)]
mod tests;
