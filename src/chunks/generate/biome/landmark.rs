//! Biomes no climate calls for, given their columns by landmarks (`landmarks`): a great
//! peak's snowy summit, a volcano's basalt and its crater's magma.

use super::{Biome, Climate};
use crate::block::BlockType;

/// Their names, which ores and structures go by.
pub const SUMMIT: &str = "summit";
pub const VOLCANO: &str = "volcano";
pub const CRATER: &str = "crater";

/// The height over the sea a summit's snow starts at, however hot it is down below, and the
/// steepest ground it settles on there, leaving the cliffs bare.
pub const SUMMIT_SNOW: i64 = 420;
const SUMMIT_SNOW_SLOPE: f64 = 3.0;

// The top of a great peak, snow-capped in any climate, as a high enough mountain is.
pub struct Summit;

impl Biome for Summit {
    fn name(&self) -> &'static str {
        SUMMIT
    }

    fn niche(&self, _climate: Climate) -> f64 {
        0.0
    }

    fn block(&self, base: BlockType, above_sea: i64, gradient: f64) -> BlockType {
        match base {
            BlockType::Grass | BlockType::Stone | BlockType::Gravel | BlockType::Sand
                if above_sea >= SUMMIT_SNOW && gradient <= SUMMIT_SNOW_SLOPE =>
            {
                BlockType::Snow
            }
            BlockType::Grass | BlockType::Sand => BlockType::Stone,
            other => other,
        }
    }
}

// Ground gentler than this on a volcano is grey ash, and steeper is bare basalt, so its
// apron is paler than its cone and gullies streak its flanks.
const ASH_GRADIENT: f64 = 0.55;

// The bare basalt and ash of a volcano's cone, under whatever snow settles on it.
pub struct Volcano;

impl Biome for Volcano {
    fn name(&self) -> &'static str {
        VOLCANO
    }

    fn niche(&self, _climate: Climate) -> f64 {
        0.0
    }

    fn block(&self, base: BlockType, _above_sea: i64, gradient: f64) -> BlockType {
        match base {
            BlockType::Grass | BlockType::Sand | BlockType::Stone | BlockType::Gravel
                if gradient < ASH_GRADIENT =>
            {
                BlockType::Gravel
            }
            BlockType::Grass | BlockType::Sand | BlockType::Stone | BlockType::Gravel => {
                BlockType::Basalt
            }
            other => other,
        }
    }
}

// Magma glowing on the floor of a crater and down the streams running from its rim.
pub struct Crater;

impl Biome for Crater {
    fn name(&self) -> &'static str {
        CRATER
    }

    fn niche(&self, _climate: Climate) -> f64 {
        0.0
    }

    fn block(&self, base: BlockType, _above_sea: i64, _gradient: f64) -> BlockType {
        match base {
            BlockType::Air | BlockType::Water | BlockType::Ice => base,
            _ => BlockType::Magma,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_summit_is_snow_capped_but_for_its_cliffs() {
        assert_eq!(
            BlockType::Snow,
            Summit.block(BlockType::Stone, SUMMIT_SNOW, 1.0)
        );
        assert_eq!(
            BlockType::Stone,
            Summit.block(BlockType::Stone, SUMMIT_SNOW, 4.0)
        );
        assert_eq!(
            BlockType::Stone,
            Summit.block(BlockType::Grass, SUMMIT_SNOW - 10, 1.0)
        );
        assert_eq!(0.0, Summit.niche(Climate::default()));
    }

    #[test]
    fn test_a_volcano_is_ash_and_basalt_under_its_snow_and_magma_in_its_crater() {
        assert_eq!(BlockType::Gravel, Volcano.block(BlockType::Grass, 40, 0.3));
        assert_eq!(BlockType::Basalt, Volcano.block(BlockType::Grass, 40, 1.0));
        assert_eq!(BlockType::Basalt, Volcano.block(BlockType::Stone, 300, 3.0));
        assert_eq!(BlockType::Snow, Volcano.block(BlockType::Snow, 400, 0.5));
        assert_eq!(BlockType::Magma, Crater.block(BlockType::Snow, 300, 0.0));
        assert_eq!(BlockType::Air, Crater.block(BlockType::Air, 300, 0.0));
        assert_eq!(0.0, Volcano.niche(Climate::default()));
    }
}
