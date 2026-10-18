use std::collections::VecDeque;

use super::generator::FACE_NORMAL_OFFSET;

use bevy::math::IVec3;

/// Sky light under open sky. Each block it spreads sideways or up loses one level, and
/// blocks it passes through take their `light_opacity` too.
pub const MAX_LIGHT: u8 = 15;

// Sky light over a box of cells, worked out from how much light each lets through.
pub struct SkyLight {
    min: IVec3,
    size: IVec3,
    levels: Vec<u8>,
}

impl SkyLight {
    /// Lights the box of `size` cells from `min`, whose `light_opacity`s are in `opacity`
    /// in `SkyLight::index` order, with `sky` falling into the top of every column.
    pub fn new(min: IVec3, size: IVec3, sky: u8, opacity: Vec<u8>) -> Self {
        Self::with_sky(min, size, |_, _| sky, opacity)
    }

    /// As `new`, with `sky(x, z)` falling into the top of each column, from `min` as 0, 0.
    pub fn with_sky(
        min: IVec3,
        size: IVec3,
        sky: impl Fn(i32, i32) -> u8,
        opacity: Vec<u8>,
    ) -> Self {
        assert_eq!((size.x * size.y * size.z) as usize, opacity.len());
        let mut levels = vec![0; opacity.len()];
        // whether anything light can pass through is shaded, so light has somewhere to spread
        let mut shade = false;
        // each column is a contiguous run of cells, lowest first
        for (i, (column, opacity)) in levels
            .chunks_mut(size.y as usize)
            .zip(opacity.chunks(size.y as usize))
            .enumerate()
        {
            // straight down, it keeps its full strength through open air
            let mut level = sky(i as i32 / size.z, i as i32 % size.z);
            for (cell, &opacity) in column.iter_mut().zip(opacity).rev() {
                level = level.saturating_sub(opacity);
                *cell = level;
                shade |= opacity < MAX_LIGHT && level < MAX_LIGHT;
            }
        }
        let mut light = Self { min, size, levels };
        if shade {
            light.spread(&opacity);
        }
        light
    }

    pub fn index(size: IVec3, local: IVec3) -> usize {
        ((local.x * size.z + local.z) * size.y + local.y) as usize
    }

    // Floods light from every lit cell into any neighbour it would brighten.
    fn spread(&mut self, opacity: &[u8]) {
        let mut queue: VecDeque<IVec3> = VecDeque::new();
        for x in 0..self.size.x {
            for z in 0..self.size.z {
                for y in 0..self.size.y {
                    let local = IVec3::new(x, y, z);
                    let level = self.levels[Self::index(self.size, local)];
                    // only light beside something darker has anywhere to go
                    let darker = |offset: IVec3| {
                        let next = local + offset;
                        !(next.cmplt(IVec3::ZERO).any() || next.cmpge(self.size).any())
                            && self.levels[Self::index(self.size, next)] + 1 < level
                    };
                    if level > 1 && FACE_NORMAL_OFFSET.into_iter().any(darker) {
                        queue.push_back(local);
                    }
                }
            }
        }
        while let Some(local) = queue.pop_front() {
            let level = self.levels[Self::index(self.size, local)];
            for offset in FACE_NORMAL_OFFSET {
                let next = local + offset;
                if next.cmplt(IVec3::ZERO).any() || next.cmpge(self.size).any() {
                    continue;
                }
                let cell = Self::index(self.size, next);
                let lit = level.saturating_sub(1 + opacity[cell]);
                if lit > self.levels[cell] {
                    self.levels[cell] = lit;
                    queue.push_back(next);
                }
            }
        }
    }

    /// The light at `coord`, or full light outside the box.
    pub fn level(&self, coord: IVec3) -> u8 {
        let local = coord - self.min;
        if local.cmplt(IVec3::ZERO).any() || local.cmpge(self.size).any() {
            return MAX_LIGHT;
        }
        self.levels[Self::index(self.size, local)]
    }
}

/// The brightest light any block gives off, see `BlockType::emits`.
pub const MAX_EMITTED: u8 = 14;

/// How many blocks block light can reach from the block giving it off, before it's faded out.
pub const BLOCK_LIGHT_REACH: i32 = MAX_EMITTED as i32 - 1;

/// Light given off by blocks, over a box of cells. It loses a level each block it spreads, and
/// blocks it passes through take their `light_opacity` too, as sky light does.
#[derive(Default)]
pub struct BlockLight {
    min: IVec3,
    size: IVec3,
    levels: Vec<u8>,
}

impl BlockLight {
    /// Lights the box of `size` cells from `min` from `emitters`, each a cell and the level it
    /// gives off, through cells whose `light_opacity` is `opacity(cell)`. Light spreads as far
    /// round the box as it could still get back into it from, so it takes in any emitter within
    /// `BLOCK_LIGHT_REACH`, and leaves out those further away, whose light can't reach it.
    pub fn new(
        min: IVec3,
        size: IVec3,
        emitters: &[(IVec3, u8)],
        opacity: impl Fn(IVec3) -> u8,
    ) -> Self {
        let reach = IVec3::splat(BLOCK_LIGHT_REACH);
        let (around, around_size) = (min - reach, size + 2 * reach);
        // How many blocks a cell, from `around`, is from the box: light no brighter than that
        // can't get back in, so isn't spread. Past `around` that's further than any light is
        // bright, so light stays inside it.
        let away = |local: IVec3| {
            let (low, high) = (reach - local, local - (reach + size - 1));
            low.max(high).max(IVec3::ZERO).element_sum()
        };
        let mut levels = vec![0; (around_size.x * around_size.y * around_size.z) as usize];
        // the cells lit to each level, spread from brightest first, so each is spread from once,
        // at the brightest it gets
        let mut lit = vec![Vec::new(); MAX_EMITTED as usize + 1];
        for &(coord, level) in emitters {
            let local = coord - around;
            if level as i32 <= away(local) {
                continue;
            }
            let cell = SkyLight::index(around_size, local);
            if level > levels[cell] {
                levels[cell] = level;
                lit[level as usize].push(local);
            }
        }
        for level in (2..=MAX_EMITTED).rev() {
            while let Some(local) = lit[level as usize].pop() {
                // lit brighter since, and spread from then
                if levels[SkyLight::index(around_size, local)] != level {
                    continue;
                }
                for offset in FACE_NORMAL_OFFSET {
                    let next = local + offset;
                    let to = level.saturating_sub(1 + opacity(around + next));
                    if to as i32 <= away(next) {
                        continue;
                    }
                    let cell = SkyLight::index(around_size, next);
                    if to > levels[cell] {
                        levels[cell] = to;
                        lit[to as usize].push(next);
                    }
                }
            }
        }
        // just the box, a column at a time, as both run y innermost
        let mut inside = Vec::with_capacity((size.x * size.y * size.z) as usize);
        for x in 0..size.x {
            for z in 0..size.z {
                let start = SkyLight::index(around_size, reach + IVec3::new(x, 0, z));
                inside.extend_from_slice(&levels[start..start + size.y as usize]);
            }
        }
        Self {
            min,
            size,
            levels: inside,
        }
    }

    /// The light at `coord`, or none outside the box.
    pub fn level(&self, coord: IVec3) -> u8 {
        let local = coord - self.min;
        if local.cmplt(IVec3::ZERO).any() || local.cmpge(self.size).any() {
            return 0;
        }
        self.levels[SkyLight::index(self.size, local)]
    }
}

/// Sky light and block light together, which meshes are lit by.
pub struct Light {
    pub sky: SkyLight,
    pub block: BlockLight,
}

impl Light {
    /// Sky light alone, where nothing gives off light.
    pub fn sky(sky: SkyLight) -> Self {
        Self {
            sky,
            block: BlockLight::default(),
        }
    }

    /// The sky light and block light at `coord`.
    // Meshing reads it for every corner of every face, LODs' too, which have no block light.
    #[inline]
    pub fn levels(&self, coord: IVec3) -> [u8; 2] {
        [self.sky.level(coord), self.block.level(coord)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockType;

    fn light(size: IVec3, block: impl Fn(IVec3) -> BlockType) -> SkyLight {
        let mut opacity = vec![0; (size.x * size.y * size.z) as usize];
        for x in 0..size.x {
            for z in 0..size.z {
                for y in 0..size.y {
                    let c = IVec3::new(x, y, z);
                    opacity[SkyLight::index(size, c)] = block(c).light_opacity();
                }
            }
        }
        SkyLight::new(IVec3::ZERO, size, MAX_LIGHT, opacity)
    }

    // a 9x12x9 box with a flat roof of `roof` blocks at y 8, from x and z 2 to 6
    fn under_roof(roof: BlockType) -> SkyLight {
        light(IVec3::new(9, 12, 9), |c| {
            let roofed = (2..=6).contains(&c.x) && (2..=6).contains(&c.z) && c.y == 8;
            if c.y == 0 {
                BlockType::Stone
            } else if roofed {
                roof
            } else {
                BlockType::Air
            }
        })
    }

    #[test]
    fn test_open_sky_is_fully_lit_down_to_the_ground() {
        let light = under_roof(BlockType::Air);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(4, 1, 4)));
        assert_eq!(0, light.level(IVec3::new(4, 0, 4)));
    }

    #[test]
    fn test_light_creeps_under_a_roof_and_fades_inwards() {
        let light = under_roof(BlockType::Stone);
        let edge = light.level(IVec3::new(2, 4, 4));
        let middle = light.level(IVec3::new(4, 4, 4));
        // the cell under the roof's edge is one step in from open sky
        assert_eq!(MAX_LIGHT - 1, edge);
        assert_eq!(MAX_LIGHT - 3, middle);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(1, 4, 4)));
    }

    #[test]
    fn test_leaves_shade_less_than_stone() {
        let leaves = under_roof(BlockType::Leaves).level(IVec3::new(4, 4, 4));
        let stone = under_roof(BlockType::Stone).level(IVec3::new(4, 4, 4));
        assert_eq!(MAX_LIGHT - 1, leaves);
        assert!(leaves > stone);
    }

    #[test]
    fn test_deep_water_darkens() {
        let light = light(IVec3::new(1, 20, 1), |c| {
            if c.y < 19 {
                BlockType::Water
            } else {
                BlockType::Air
            }
        });
        let depths: Vec<u8> = (0..20)
            .rev()
            .map(|y| light.level(IVec3::new(0, y, 0)))
            .collect();
        assert!(depths.windows(2).all(|w| w[0] >= w[1]), "{depths:?}");
        assert_eq!(0, light.level(IVec3::ZERO));
    }

    // Block light over a 20-block box from `emitters`, through stone where `stone` says.
    fn block_light(emitters: &[(IVec3, u8)], stone: impl Fn(IVec3) -> bool) -> BlockLight {
        let opacity = |c| {
            if stone(c) {
                BlockType::Stone.light_opacity()
            } else {
                0
            }
        };
        BlockLight::new(IVec3::ZERO, IVec3::splat(20), emitters, opacity)
    }

    #[test]
    fn test_block_light_falls_off_by_one_a_block() {
        let torch = IVec3::splat(10);
        let light = block_light(&[(torch, MAX_EMITTED)], |_| false);
        assert_eq!(MAX_EMITTED, light.level(torch));
        for step in 0..10 {
            assert_eq!(
                MAX_EMITTED - step,
                light.level(torch + IVec3::X * step as i32)
            );
        }
        // round corners just the same, by the blocks it steps through, and out to its reach
        assert_eq!(MAX_EMITTED - 9, light.level(IVec3::splat(13)));
        assert_eq!(1, light.level(IVec3::new(19, 14, 10)));
        assert_eq!(0, light.level(IVec3::new(19, 15, 10)));
        // the corner of the box is 30 blocks round the edges, past its reach
        assert_eq!(0, light.level(IVec3::ZERO));
        assert_eq!(0, light.level(IVec3::splat(-1)), "outside the box");
    }

    #[test]
    fn test_block_light_is_blocked_by_opaque_blocks_and_goes_round_them() {
        let torch = IVec3::new(10, 10, 10);
        // a wall at x 11, from y 5 up, so light behind it comes under it
        let light = block_light(&[(torch, MAX_EMITTED)], |c| c.x == 11 && c.y >= 5);
        assert_eq!(0, light.level(IVec3::new(11, 10, 10)));
        // down 6 to y 4, across 2 and up 1, while straight behind it is too far round
        assert_eq!(MAX_EMITTED - 9, light.level(IVec3::new(12, 5, 10)));
        assert_eq!(0, light.level(IVec3::new(12, 10, 10)));
        // shut in on every side, it lights nothing else
        let shut = block_light(&[(torch, MAX_EMITTED)], |c| {
            c != torch && (c - torch).abs().max_element() == 1
        });
        assert_eq!(0, shut.level(torch + IVec3::new(2, 0, 0)));
        assert_eq!(MAX_EMITTED, shut.level(torch));
    }

    #[test]
    fn test_block_light_takes_in_emitters_beyond_the_box_as_far_as_they_reach() {
        let near = IVec3::new(-5, 10, 10);
        let light = block_light(&[(near, MAX_EMITTED)], |_| false);
        assert_eq!(MAX_EMITTED - 5, light.level(IVec3::new(0, 10, 10)));
        let far = IVec3::new(-BLOCK_LIGHT_REACH - 1, 10, 10);
        let light = block_light(&[(far, MAX_EMITTED)], |_| false);
        assert_eq!(0, light.level(IVec3::new(0, 10, 10)));
        // the brighter of two lights a block, not their sum
        let two = block_light(&[(near, MAX_EMITTED), (IVec3::new(4, 10, 10), 6)], |_| {
            false
        });
        assert_eq!(MAX_EMITTED - 5, two.level(IVec3::new(0, 10, 10)));
        assert_eq!(6 - 2, two.level(IVec3::new(6, 10, 10)));
    }

    #[test]
    fn test_sky_light_and_block_light_are_kept_apart() {
        let light = Light {
            sky: under_roof(BlockType::Stone),
            block: block_light(&[(IVec3::new(4, 4, 4), MAX_EMITTED)], |_| false),
        };
        let shaded = light.sky.level(IVec3::new(4, 4, 4));
        assert_eq!([shaded, MAX_EMITTED], light.levels(IVec3::new(4, 4, 4)));
        assert_eq!(
            [MAX_LIGHT, MAX_EMITTED - 4],
            light.levels(IVec3::new(0, 4, 4))
        );
    }

    #[test]
    fn test_no_block_gives_off_more_than_the_brightest_light() {
        assert!(BlockType::ALL.iter().all(|b| b.emits() <= MAX_EMITTED));
        assert!(BlockType::ALL.iter().any(|b| b.emits() == MAX_EMITTED));
    }

    #[test]
    fn test_outside_the_box_is_lit() {
        let light = under_roof(BlockType::Stone);
        assert_eq!(MAX_LIGHT, light.level(IVec3::new(-1, 4, 4)));
    }
}
