//! A great peak: a lone mountain standing out of low country, its summit near the top of the
//! world, snow-capped with bands of cliffs. It's shaped into the height field itself, raised
//! over the ground already there, so it blends into the land round it and every LOD, the map
//! and the shading of the snow line take it as they take any mountain.

use std::f64::consts::{PI, TAU};

use bevy::math::DVec2;

use super::super::biome::landmark::SUMMIT_SNOW;
use super::super::biome::{smoothstep, BiomeId};
use super::super::fractal::resolved;
use super::super::noise::{raise, NoiseGenerator};
use super::super::WATER_LEVEL;
use super::{pick, unit, Shaping, Site};

/// How far its foot spreads from its summit, in blocks, from the least to the most.
pub(super) const RADIUS: (f64, f64) = (1500.0, 1900.0);
// The height its summit is raised to before it's eased under the top of the world (see
// `noise::raise`), which brings these to about 950 and 990, 500 or more over the sea.
const SUMMIT: (f64, f64) = (1000.0, 1060.0);
/// Its foot can be warped out past its radius by up to this share of it.
pub(super) const REACH: f64 = 1.0 + WARP;
// Structures keep out of this share of its radius, the steep heart of the mountain.
pub(super) const CLEARING: f64 = 0.6;
const WARP: f64 = 0.12;

// Its flanks rise as the distance in from its foot to this power: concave, gentle at the foot
// and steepening to the summit, as a mountain worn by ice is.
const FLANK_POWER: f64 = 2.2;
// It has three to five faces between sharp ridges, flattened this much from a round cone.
const FACES: (f64, f64) = (3.0, 5.99);
const FACETED: f64 = 0.5;
// Crags over its flanks, strongest high up.
const CRAG_HEIGHT: f64 = 40.0;
// Bands of cliff: every so many blocks up its upper flanks, a steep step and then a ledge,
// over the share of each band the step takes. They're too fine for cells wider than about a
// band's width across the slope to see, so they fade out of coarse LODs.
const CLIFF_BAND: f64 = 34.0;
const CLIFF_STEP: f64 = 0.35;
const CLIFF_STRENGTH: f64 = 0.7;
const CLIFF_SPAN: f64 = 90.0;
// Its summit, given to the summit's biome from this far under the height its snow starts at,
// which is snow-capped in any climate.
const SUMMIT_BELOW_SNOW: i64 = 40;
// The ground it rises from: land, not far over the sea, and not in a range already.
const SITE_ABOVE_SEA: (f64, f64) = (6.0, 160.0);
const RING: f64 = 0.55;
const RING_POINTS: usize = 8;
const RING_LAND: usize = 6;
const RING_HIGHEST: f64 = 220.0;

#[derive(Debug, Clone)]
pub struct Peak {
    pub centre: DVec2,
    pub radius: f64,
    // how far its summit rises over the ground at its centre
    rise: f64,
    faces: f64,
    turn: f64,
}

impl Peak {
    // A peak at `site`, with `h` deciding its size and shape, if the ground there suits one.
    pub(super) fn place(noise: &NoiseGenerator, site: &Site, h: u64) -> Option<Self> {
        if !(SITE_ABOVE_SEA.0..SITE_ABOVE_SEA.1).contains(&site.above_sea) {
            return None;
        }
        let radius = pick(h, 0, RADIUS);
        let ring = site.ring(noise, radius * RING, RING_POINTS);
        let land = ring
            .iter()
            .filter(|&&height| height > WATER_LEVEL as f64)
            .count();
        if land < RING_LAND
            || ring
                .iter()
                .any(|&height| height > site.height + RING_HIGHEST)
        {
            return None;
        }
        Some(Self {
            centre: site.at.as_dvec2(),
            radius,
            rise: pick(h, 16, SUMMIT) - site.height,
            faces: pick(h, 32, FACES).floor(),
            turn: unit(h, 48) * TAU,
        })
    }

    // How far the peak raises the ground at (`x`, `z`), seen by a cell `cell` blocks wide.
    fn lift(&self, shaping: &Shaping, x: f64, z: f64, cell: f64) -> f64 {
        let offset = DVec2::new(x, z) - self.centre + shaping.warp(x, z) * WARP * self.radius;
        let distance = offset.length();
        // the distance as faces between ridges measure it: a face's middle counts as further
        // out than a ridge the same distance away, so each face falls away flatter
        let sector = TAU / self.faces;
        let across = (offset.to_angle() - self.turn).rem_euclid(sector) - sector / 2.0;
        let faceted = distance * across.cos() / (PI / self.faces).cos();
        let s = (distance + (faceted - distance) * FACETED) / self.radius;
        if s >= 1.0 {
            return 0.0;
        }
        let flank = (1.0 - s).powf(FLANK_POWER);
        let mut lift = self.rise * flank;
        let cliffs = CLIFF_STRENGTH * resolved(CLIFF_SPAN, cell) * smoothstep(0.1, 0.4, flank);
        if cliffs > 0.0 {
            let band = lift / CLIFF_BAND;
            let stepped =
                (band.floor() + smoothstep(1.0 - CLIFF_STEP, 1.0, band.fract())) * CLIFF_BAND;
            lift += (stepped - lift) * cliffs;
        }
        let crags = shaping.crags.get(x, z, cell) * CRAG_HEIGHT * flank.sqrt();
        (lift + crags).max(0.0)
    }

    pub(super) fn shape(
        &self,
        shaping: &Shaping,
        x: f64,
        z: f64,
        cell: f64,
        (height, biome, trees): (f64, BiomeId, f64),
        summit: BiomeId,
    ) -> (f64, BiomeId, f64) {
        let height = raise(height, self.lift(shaping, x, z, cell));
        let high = height - WATER_LEVEL as f64 >= (SUMMIT_SNOW - SUMMIT_BELOW_SNOW) as f64;
        (height, if high { summit } else { biome }, trees)
    }
}
