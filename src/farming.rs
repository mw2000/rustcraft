//! Farming (§5.5a): a hoe tills dirt or grass into farmland, seeds planted on it come up as
//! wheat, and wheat grows through `WHEAT`'s stages until it's ripe, quicker with water near.
//! Each stage is a block of its own, so growing is an edit like any other, saved and sent to
//! clients as one (§4.2). What the edit layer doesn't hold is when each crop was planted, which
//! the server keeps beside it in `Crops`, as it keeps containers, and saves with its chunk's
//! edits (`save::region`). A crop's stage follows from that and the time, so a field no one's
//! near grows nothing until its chunk's loaded again, and then catches up at once
//! (`server::farming`).

use bevy::math::{I64Vec3, U16Vec3};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::Resource;

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CHUNK_SIZE};
use crate::chunks::generate::trees::hash;
use crate::clock::DAY_TICKS;
use crate::world::World;

/// Wheat's stages, from the shoots seeds come up as to ripe ears.
pub const WHEAT: [BlockType; 8] = [
    BlockType::Wheat0,
    BlockType::Wheat1,
    BlockType::Wheat2,
    BlockType::Wheat3,
    BlockType::Wheat4,
    BlockType::Wheat5,
    BlockType::Wheat6,
    BlockType::RipeWheat,
];

/// How long a stage takes on average on dry farmland, in game days: wheat's ripe two and a half
/// days after it's planted, 25 minutes.
pub const STAGE_DAYS: f64 = 2.5 / (WHEAT.len() - 1) as f64;
/// How many times quicker crops grow with water within `WATER_REACH` of their farmland.
pub const WET_SPEED: f64 = 1.5;
// How far water wets farmland, in blocks sideways, level with it or a block above.
const WATER_REACH: i64 = 4;

/// What tilling `block` with a hoe makes of it, if it can be tilled.
pub fn tilled(block: BlockType) -> Option<BlockType> {
    matches!(block, BlockType::Dirt | BlockType::Grass).then_some(BlockType::Farmland)
}

/// Which of wheat's stages `block` is, if it's one.
pub fn stage(block: BlockType) -> Option<usize> {
    WHEAT.iter().position(|&stage| stage == block)
}

/// When, in ticks (`TimeOfDay`), a crop at `at` planted at tick `planted` reaches `stage`,
/// growing `wet` or dry. Each stage takes from 0.6 to 1.4 times `STAGE_DAYS`, as the world's
/// `seed`, the crop's place and when it was planted decide, so a field comes up unevenly and a
/// crop sown again grows differently, but always the same given those, on every machine.
pub fn reaches(seed: u32, at: I64Vec3, planted: u64, stage: usize, wet: bool) -> u64 {
    let crop = hash(u64::from(seed) << 32 ^ planted, at.x, at.z);
    let crop = hash(crop, at.y, 0);
    let days: f64 = (1..=stage)
        .map(|stage| {
            let share = (hash(crop, stage as i64, 0) >> 40) as f64 / (1u64 << 24) as f64;
            STAGE_DAYS * (0.6 + 0.8 * share)
        })
        .sum();
    let ticks = days * DAY_TICKS as f64 / if wet { WET_SPEED } else { 1.0 };
    planted.saturating_add(ticks.round() as u64)
}

/// The stage a crop's grown to by `now`, see `reaches`.
pub fn grown(seed: u32, at: I64Vec3, planted: u64, now: u64, wet: bool) -> usize {
    (1..WHEAT.len())
        .take_while(|&stage| reaches(seed, at, planted, stage, wet) <= now)
        .count()
}

/// Whether the crop at `at` has water near enough its farmland to grow quicker. Water's looked
/// for as the crop grows, as if it had always been as it is now, so water brought to a field
/// hurries it on at once and a field left dry waits for its time to catch up.
pub fn is_watered(world: &World, at: I64Vec3) -> bool {
    let farmland = at - I64Vec3::Y;
    (-WATER_REACH..=WATER_REACH).any(|x| {
        (-WATER_REACH..=WATER_REACH).any(|z| {
            (0..=1)
                .any(|y| world.get_block_at(farmland + I64Vec3::new(x, y, z)) == BlockType::Water)
        })
    })
}

/// Every crop growing in the world, by chunk and where in it, as the edit layer keeps its
/// edits: when it was planted, in ticks. A ripe crop has nothing left to grow, so it's let
/// go of. Only the server has them (INV-4).
#[derive(Resource, Default)]
pub struct Crops {
    chunks: HashMap<ChunkCoordinate, Field>,
    // the chunks whose crops changed since saving last asked
    changed: HashSet<ChunkCoordinate>,
}

// A chunk's crops, and the soonest any of them is due to grow, or sooner, as it's only lowered
// until the field's next looked at.
#[derive(Default)]
struct Field {
    crops: HashMap<U16Vec3, Crop>,
    due: u64,
}

#[derive(Clone, Copy)]
struct Crop {
    planted: u64,
    // when it's next to be looked at, as it's due to reach its next stage then
    due: u64,
}

impl Crops {
    /// Plants a crop at `at` at `now`, in place of any that was there.
    pub fn plant(&mut self, at: I64Vec3, now: u64) {
        let coord = ChunkCoordinate::containing(at);
        self.put(coord, ChunkCoordinate::local_block(at), now);
        self.changed.insert(coord);
    }

    /// Puts back a crop loaded from a save, which as it's already saved isn't a change. It's
    /// looked at as soon as its chunk's loaded, to catch up on the time it's been away.
    pub fn put(&mut self, coord: ChunkCoordinate, local: U16Vec3, planted: u64) {
        let field = self.chunks.entry(coord).or_default();
        let due = 0;
        field.crops.insert(local, Crop { planted, due });
        field.due = due;
    }

    /// Forgets the crop at `at`, if there's one, as its block's changed.
    pub fn remove(&mut self, at: I64Vec3) {
        let coord = ChunkCoordinate::containing(at);
        let Some(field) = self.chunks.get_mut(&coord) else {
            return;
        };
        if field
            .crops
            .remove(&ChunkCoordinate::local_block(at))
            .is_some()
        {
            self.changed.insert(coord);
        }
        if field.crops.is_empty() {
            self.chunks.remove(&coord);
        }
    }

    /// When the crop at `at` was planted, if there's one.
    pub fn planted(&self, at: I64Vec3) -> Option<u64> {
        let field = self.chunks.get(&ChunkCoordinate::containing(at))?;
        let crop = field.crops.get(&ChunkCoordinate::local_block(at))?;
        Some(crop.planted)
    }

    /// Up to `most` of the crops due to grow by `now`, in the chunks `loaded` says are, with
    /// when they were planted. Each is to be told when it's next due, with `grew`. Only the
    /// fields due are looked through, so a frame with none costs a comparison a field.
    pub fn due(
        &mut self,
        now: u64,
        loaded: impl Fn(ChunkCoordinate) -> bool,
        most: usize,
    ) -> Vec<(I64Vec3, u64)> {
        let mut due = Vec::new();
        for (&coord, field) in &mut self.chunks {
            if field.due > now || !loaded(coord) {
                continue;
            }
            let origin = coord.0 * CHUNK_SIZE as i64;
            let mut later = u64::MAX;
            for (&local, crop) in &field.crops {
                if crop.due > now {
                    later = later.min(crop.due);
                } else if due.len() < most {
                    due.push((origin + local.as_i64vec3(), crop.planted));
                } else {
                    // the rest wait for the next frame, as does the field
                    return due;
                }
            }
            field.due = later;
        }
        due
    }

    /// Says when the crop at `at`, given by `due`, is next due to grow, or that it's done
    /// growing, ripe or gone, and is to be forgotten.
    pub fn grew(&mut self, at: I64Vec3, next: Option<u64>) {
        let Some(next) = next else {
            self.remove(at);
            return;
        };
        let coord = ChunkCoordinate::containing(at);
        if let Some(field) = self.chunks.get_mut(&coord) {
            if let Some(crop) = field.crops.get_mut(&ChunkCoordinate::local_block(at)) {
                crop.due = next;
                field.due = field.due.min(next);
            }
        }
    }

    /// The chunks with crops in them, in no particular order.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkCoordinate> + '_ {
        self.chunks.keys().copied()
    }

    /// The crops in the chunk at `coord` and when each was planted, in no particular order.
    pub fn in_chunk(&self, coord: ChunkCoordinate) -> impl Iterator<Item = (U16Vec3, u64)> + '_ {
        let crops = self.chunks.get(&coord).into_iter();
        crops.flat_map(|field| {
            field
                .crops
                .iter()
                .map(|(&local, crop)| (local, crop.planted))
        })
    }

    /// The chunks whose crops changed since this was last called.
    pub fn take_changed(&mut self) -> HashSet<ChunkCoordinate> {
        std::mem::take(&mut self.changed)
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;

    use super::{grown, is_watered, reaches, stage, tilled, Crops, STAGE_DAYS, WET_SPEED, WHEAT};
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::clock::{TimeOfDay, DAY_TICKS};
    use crate::world::World;

    const AT: I64Vec3 = I64Vec3::new(17, 460, -5);

    // The tick `days` days in.
    fn day(days: f64) -> u64 {
        TimeOfDay::from_days(days).0
    }

    // Ticks as days.
    fn days(ticks: u64) -> f64 {
        ticks as f64 / DAY_TICKS as f64
    }

    #[test]
    fn test_only_dirt_and_grass_are_tilled_into_farmland() {
        for block in BlockType::ALL {
            let expected = matches!(block, BlockType::Dirt | BlockType::Grass);
            assert_eq!(expected.then_some(BlockType::Farmland), tilled(block));
        }
        assert_eq!(Some(0), stage(BlockType::Wheat0));
        assert_eq!(Some(WHEAT.len() - 1), stage(BlockType::RipeWheat));
        assert_eq!(None, stage(BlockType::TallGrass));
    }

    #[test]
    fn test_wheat_ripens_in_about_two_and_a_half_days_and_quicker_wet() {
        let last = WHEAT.len() - 1;
        let mut ripe_dry = Vec::new();
        for x in 0..200 {
            let at = AT + I64Vec3::X * x;
            let dry = days(reaches(7, at, day(3.0), last, false)) - 3.0;
            let wet = days(reaches(7, at, day(3.0), last, true)) - 3.0;
            assert!((dry / wet - WET_SPEED).abs() < 1e-3, "{dry} {wet}");
            // each stage takes from 0.6 to 1.4 times its share, to the tick
            let tick = days(1);
            for stage in 1..=last {
                let took = days(
                    reaches(7, at, day(3.0), stage, false)
                        - reaches(7, at, day(3.0), stage - 1, false),
                );
                assert!(
                    (0.6 * STAGE_DAYS - tick..=1.4 * STAGE_DAYS + tick).contains(&took),
                    "{took}"
                );
            }
            ripe_dry.push(dry);
        }
        let mean = ripe_dry.iter().sum::<f64>() / ripe_dry.len() as f64;
        assert!((2.3..2.7).contains(&mean), "{mean} days");
        // and crops side by side come up at different times
        let spread = ripe_dry.iter().copied().fold(f64::NAN, f64::max)
            - ripe_dry.iter().copied().fold(f64::NAN, f64::min);
        assert!(spread > 0.3, "{spread} days");
    }

    #[test]
    fn test_growth_is_decided_by_the_seed_the_place_and_the_planting_alone() {
        let ripe = |seed, at, planted| reaches(seed, at, day(planted), WHEAT.len() - 1, false);
        assert_eq!(ripe(7, AT, 3.0), ripe(7, AT, 3.0));
        assert_ne!(ripe(7, AT, 3.0), ripe(8, AT, 3.0));
        assert_ne!(ripe(7, AT, 3.0) - day(3.0), ripe(7, AT, 3.5) - day(3.5));
        assert_ne!(ripe(7, AT, 3.0), ripe(7, AT + I64Vec3::Y, 3.0));
        // a tick later is another planting
        assert_ne!(
            ripe(7, AT, 3.0) - day(3.0),
            reaches(7, AT, day(3.0) + 1, WHEAT.len() - 1, false) - day(3.0) - 1
        );
        // stage by stage, never going back, from seeds at planting to ripe
        let planted = day(3.0);
        assert_eq!(0, grown(7, AT, planted, planted, false));
        assert_eq!(0, grown(7, AT, planted, day(2.0), false));
        let stages: Vec<_> = (0..40)
            .map(|tenth| grown(7, AT, planted, day(3.0 + tenth as f64 / 10.0), false))
            .collect();
        assert!(stages.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(WHEAT.len() - 1, stages[39]);
        assert_eq!(WHEAT.len() - 1, grown(7, AT, planted, day(100.0), false));
    }

    #[test]
    fn test_water_within_four_blocks_of_the_farmland_waters_a_crop() {
        let mut world = World::new(16);
        let coord = ChunkCoordinate::containing(AT);
        for dx in -1..=1 {
            world.insert_chunk(
                ChunkCoordinate(coord.0 + I64Vec3::X * dx),
                ChunkData::default(),
            );
        }
        assert!(!is_watered(&world, AT));
        let farmland = AT - I64Vec3::Y;
        world.edit(farmland + I64Vec3::new(5, 0, 0), BlockType::Water);
        world.edit(farmland + I64Vec3::new(0, -1, 0), BlockType::Water);
        world.edit(farmland + I64Vec3::new(0, 2, 0), BlockType::Water);
        assert!(!is_watered(&world, AT));
        world.edit(farmland + I64Vec3::new(-4, 1, 4), BlockType::Water);
        assert!(is_watered(&world, AT));
    }

    #[test]
    fn test_crops_are_due_at_once_then_when_they_said_and_forgotten_once_done() {
        let mut crops = Crops::default();
        let other = AT + I64Vec3::X * 40;
        crops.plant(AT, 100);
        crops.plant(other, 150);
        let loaded = |coord: ChunkCoordinate| coord == ChunkCoordinate::containing(AT);
        // only those in loaded chunks, and only so many a call
        assert_eq!(vec![(AT, 100)], crops.due(100, loaded, 10));
        assert!(crops.due(100, |_| true, 0).is_empty());
        assert_eq!(vec![(other, 150)], crops.due(150, |_| true, 10));
        crops.grew(AT, Some(200));
        crops.grew(other, Some(300));
        assert!(crops.due(199, |_| true, 10).is_empty());
        assert_eq!(vec![(AT, 100)], crops.due(200, |_| true, 10));
        assert_eq!(2, crops.take_changed().len());
        // ripe, it's let go of, which is a change to save
        crops.grew(AT, None);
        assert_eq!(None, crops.planted(AT));
        assert_eq!(Some(150), crops.planted(other));
        assert_eq!(1, crops.chunks().count());
        assert_eq!(1, crops.take_changed().len());
    }
}
