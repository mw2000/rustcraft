//! A volcano: a broad cone of basalt with a crater at its top, magma glowing on the crater's
//! floor and in streams down its flanks, so it shows at night from far away. Shaped into the
//! height field, as a great peak is, with the cone's ground given to the volcano's biomes,
//! which turn it to basalt, the crater's floor to magma and the streams to lava (M4.3).

use std::f64::consts::TAU;

use bevy::math::DVec2;

use super::super::biome::smoothstep;
use super::super::biome::BiomeId;
use super::super::noise::{raise, NoiseGenerator};
use super::super::WATER_LEVEL;
use super::{pick, unit, Shaping, Site};

/// How far its foot spreads from its middle, in blocks, from the least to the most.
pub(super) const RADIUS: (f64, f64) = (1200.0, 1500.0);
// How high its rim stands over the ground at its middle.
const RISE: (f64, f64) = (340.0, 440.0);
// Its crater's radius at the rim, and how far down its floor is.
const CRATER_RADIUS: (f64, f64) = (110.0, 150.0);
const CRATER_DEPTH: (f64, f64) = (45.0, 65.0);
// The crater's floor is flat out to this share of its radius, then its walls climb to the rim.
const FLOOR: f64 = 0.5;
// Magma fills the crater out to this share of its radius, a little up its walls.
const LAKE: f64 = 0.6;
/// Its foot can be warped out past its radius by up to this share of it.
pub(super) const REACH: f64 = 1.0 + WARP;
const WARP: f64 = 0.06;
// Its flanks rise as the distance in from its foot to this power, steepening to the rim.
const FLANK_POWER: f64 = 2.3;
// It's built over the land round it from this share of its radius in, blending into it
// further out.
const BLEND: f64 = 0.8;
/// The cone is bare basalt in to this share of its radius, the line wandering by up to
/// `ASH_WANDER` of it, and the land round it keeps its own ground and trees further out.
pub(super) const ASH_LINE: f64 = 0.7;
const ASH_WANDER: f64 = 0.15;
// Ridges and gullies down its flanks.
const GULLY_HEIGHT: f64 = 14.0;
// Streams of magma run from the rim part way down, each a few blocks wide in a channel a
// block deep, too narrow for cells wider than `STREAM_MAX_CELL` to show.
const STREAMS: usize = 4;
const STREAM_LENGTH: (f64, f64) = (0.15, 0.4);
const STREAM_HALF_WIDTH: f64 = 2.0;
const STREAM_WANDER: f64 = 0.04;
const STREAM_MAX_CELL: f64 = 2.0;
// Where one stands: dry land, not far over the sea, not in the ice.
const SITE_ABOVE_SEA: (f64, f64) = (4.0, 140.0);
const COLDEST: f32 = 0.18;
const RING: f64 = 0.5;
const RING_POINTS: usize = 8;
const RING_LAND: usize = 6;

#[derive(Debug, Clone)]
pub struct Volcano {
    pub centre: DVec2,
    pub radius: f64,
    // the ground at its middle, which its rim and crater are measured from
    site: f64,
    rise: f64,
    crater: f64,
    depth: f64,
    // each stream's direction from the middle and how far it runs, in radii from the rim
    streams: [(f64, f64); STREAMS],
}

impl Volcano {
    // A volcano at `site`, with `h` deciding its size and streams, if the ground there suits one.
    pub(super) fn place(noise: &NoiseGenerator, site: &Site, h: u64) -> Option<Self> {
        if !(SITE_ABOVE_SEA.0..SITE_ABOVE_SEA.1).contains(&site.above_sea)
            || site.temperature < COLDEST
        {
            return None;
        }
        let radius = pick(h, 0, RADIUS);
        let ring = site.ring(noise, radius * RING, RING_POINTS);
        if ring
            .iter()
            .filter(|&&height| height > WATER_LEVEL as f64)
            .count()
            < RING_LAND
        {
            return None;
        }
        let streams = std::array::from_fn(|i| {
            let h = super::hash(h, i as i64, 11);
            let angle = (i as f64 + unit(h, 0) * 0.7) * TAU / STREAMS as f64;
            (angle, pick(h, 16, STREAM_LENGTH))
        });
        Some(Self {
            centre: site.at.as_dvec2(),
            radius,
            site: site.height,
            rise: pick(h, 16, RISE),
            crater: pick(h, 32, CRATER_RADIUS),
            depth: pick(h, 48, CRATER_DEPTH),
            streams,
        })
    }

    // The cone's height over its site at `distance` from its middle, before gullies.
    fn profile(&self, distance: f64) -> f64 {
        if distance >= self.crater {
            let flank = ((self.radius - distance) / (self.radius - self.crater)).max(0.0);
            self.rise * flank.powf(FLANK_POWER)
        } else {
            let wall = smoothstep(self.crater * FLOOR, self.crater, distance);
            self.rise - self.depth * (1.0 - wall)
        }
    }

    // Whether a stream of magma runs at `offset` from the middle, `distance` out.
    fn in_stream(&self, offset: DVec2, distance: f64) -> bool {
        let angle = offset.to_angle();
        self.streams.iter().any(|&(direction, length)| {
            let from_rim = (distance - self.crater) / self.radius;
            let wander = (distance / 97.0).sin() * STREAM_WANDER;
            let off = (angle - direction - wander + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
            (0.0..length).contains(&from_rim) && (off * distance).abs() < STREAM_HALF_WIDTH
        })
    }

    pub(super) fn shape(
        &self,
        shaping: &Shaping,
        x: f64,
        z: f64,
        cell: f64,
        (height, biome, trees): (f64, BiomeId, f64),
        (volcano, crater, lava_stream): (BiomeId, BiomeId, BiomeId),
    ) -> (f64, BiomeId, f64) {
        let warp = shaping.warp(x, z);
        let offset = DVec2::new(x, z) - self.centre + warp * WARP * self.radius;
        let distance = offset.length();
        let s = distance / self.radius;
        if s >= 1.0 {
            return (height, biome, trees);
        }
        // down the flanks and the crater's walls, leaving the magma in it flat
        let flank = smoothstep(self.crater * LAKE, self.crater, distance)
            * (1.0 - smoothstep(self.crater, self.radius, distance));
        let gullies = shaping.crags.get(x, z, cell) * GULLY_HEIGHT * flank.sqrt();
        let stream = cell <= STREAM_MAX_CELL && self.in_stream(offset, distance);
        let channel = if stream { 1.0 } else { 0.0 };
        let cone = self.site + self.profile(distance) + gullies - channel;
        // over the land, never digging into it, built up fully from `BLEND` of the way in
        let blend = smoothstep(1.0, BLEND, s);
        let lift = (cone - height).max(0.0) * blend;
        let ash = s + (warp.x + shaping.crags.get(x, z, cell) * 0.3) * ASH_WANDER;
        let owner = if distance < self.crater * LAKE {
            crater
        } else if stream {
            lava_stream
        } else if ash < ASH_LINE {
            volcano
        } else {
            biome
        };
        let trees = trees * smoothstep(ASH_LINE, ASH_LINE + 0.15, ash);
        (raise(height, lift), owner, trees)
    }

    /// Caves stay shut under the crater's magma, see `Landmarks::cave_roof`.
    pub(super) fn cave_roof(&self, x: i64, z: i64, ground: impl FnOnce() -> i64) -> Option<i64> {
        let distance = DVec2::new(x as f64, z as f64).distance(self.centre);
        (distance < self.crater + self.radius * WARP).then(ground)
    }
}
