//! The land seen from above, coloured from the terrain's own sampling (INV-1): shaded relief,
//! the sea, biomes with grass tinted as it's drawn in game, sea ice and snow on the peaks. The
//! map (§5.7) and the `world_map` example both draw with it.

use bevy::color::{ColorToComponents, LinearRgba, Srgba};
use bevy::math::{DVec2, DVec3, Vec3};

use crate::chunks::generate::biome::{Biomes, Climate};
use crate::chunks::generate::generator::{FREEZE_TEMPERATURE, SNOWY_GRASS_TEMPERATURE};
use crate::chunks::generate::noise::{NoiseGenerator, TerrainSample};
use crate::chunks::generate::WATER_LEVEL;

const SAND: [f64; 3] = [222.0, 196.0, 132.0];
const SEA_ICE: [f64; 3] = [190.0, 222.0, 240.0];
const GLACIER: [f64; 3] = [226.0, 238.0, 248.0];
const SNOW: [f32; 3] = [236.0, 242.0, 248.0];

/// A square grid of the terrain and its climate, sampled `scale` blocks apart, x across and z
/// down, so north is up.
pub struct Relief {
    size: usize,
    scale: f64,
    samples: Vec<(TerrainSample, Climate)>,
}

impl Relief {
    /// `size` samples each way from `corner` (x, z), shared between `threads` threads.
    pub fn sample(
        noise: &NoiseGenerator,
        corner: DVec2,
        scale: f64,
        size: usize,
        threads: usize,
    ) -> Self {
        let mut samples = vec![(TerrainSample::default(), Climate::default()); size * size];
        let fill = |first_row: usize, rows: &mut [(TerrainSample, Climate)]| {
            for (i, sample) in rows.iter_mut().enumerate() {
                let x = corner.x + (i % size) as f64 * scale;
                let z = corner.y + (first_row + i / size) as f64 * scale;
                *sample = (noise.sample(x, z), noise.climate(x, z));
            }
        };
        if threads <= 1 {
            fill(0, &mut samples);
        } else {
            let rows_per_thread = size.div_ceil(threads);
            std::thread::scope(|scope| {
                for (n, rows) in samples.chunks_mut(rows_per_thread * size).enumerate() {
                    scope.spawn(move || fill(n * rows_per_thread, rows));
                }
            });
        }
        Self {
            size,
            scale,
            samples,
        }
    }

    pub fn terrain(&self) -> impl Iterator<Item = &TerrainSample> {
        self.samples.iter().map(|(terrain, _)| terrain)
    }

    // The sample at (x, z), or the nearest at the grid's edge.
    fn at(&self, x: usize, z: usize) -> (TerrainSample, Climate) {
        let last = self.size - 1;
        self.samples[z.min(last) * self.size + x.min(last)]
    }

    fn height(&self, x: usize, z: usize) -> f64 {
        self.at(x, z).0.height
    }

    /// The colour of sample (`x`, `z`), as sRGB, lit from the north west by the slope between
    /// the samples either side. Lowlands are coloured by their biome if `biomes` is given, or
    /// else by height alone.
    pub fn colour(&self, x: usize, z: usize, biomes: Option<&Biomes>) -> [u8; 3] {
        let (sample, climate) = self.at(x, z);
        let above_sea = sample.height - WATER_LEVEL as f64;
        let mut colour = height_colour(above_sea);
        if let Some(biomes) = biomes {
            // up to where the height colours turn to rock
            if (2.0..140.0).contains(&above_sea) {
                let ground = match biomes.name(sample.biome) {
                    Some("desert") => SAND,
                    Some("glacier") => GLACIER,
                    _ => grass_map_colour(biomes, climate, sample.trees),
                };
                let rock = ((above_sea - 40.0) / 100.0).clamp(0.0, 1.0);
                colour = lerp_rgb(ground, colour, rock);
            }
            if above_sea <= 0.0 && above_sea > -60.0 {
                let ice =
                    (1.0 - climate.temperature as f64 / FREEZE_TEMPERATURE as f64).clamp(0.0, 1.0);
                if ice > 0.0 {
                    colour = lerp_rgb(colour, SEA_ICE, (ice * 4.0).min(1.0));
                }
            }
        }

        // Exaggerated, so gentle slopes still read at map scale.
        if above_sea > 0.0 {
            let across = |a: f64, b: f64| (a - b) / (2.0 * self.scale);
            let dx = across(self.height(x + 1, z), self.height(x.saturating_sub(1), z));
            let dz = across(self.height(x, z + 1), self.height(x, z.saturating_sub(1)));
            let shade = (1.0 - (dx + dz) * 3.0).clamp(0.4, 1.4);
            colour = colour.map(|c| c * shade);
        }
        colour.map(|c| c.clamp(0.0, 255.0) as u8)
    }
}

fn lerp_rgb(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    DVec3::from(a)
        .lerp(DVec3::from(b), t.clamp(0.0, 1.0))
        .into()
}

// Height above (or below) sea level to a map colour, white from the snow line up.
fn height_colour(above_sea: f64) -> [f64; 3] {
    const STOPS: [(f64, [f64; 3]); 10] = [
        (-380.0, [8.0, 18.0, 55.0]),
        (-200.0, [14.0, 34.0, 90.0]),
        (-70.0, [25.0, 70.0, 140.0]),
        (-30.0, [55.0, 120.0, 180.0]),
        (0.0, [100.0, 170.0, 205.0]),
        (0.01, [210.0, 200.0, 140.0]),
        (2.0, [90.0, 150.0, 70.0]),
        (40.0, [60.0, 115.0, 50.0]),
        (140.0, [125.0, 110.0, 95.0]),
        (250.0, [245.0, 245.0, 250.0]),
    ];
    if above_sea <= STOPS[0].0 {
        return STOPS[0].1;
    }
    for pair in STOPS.windows(2) {
        let ((h0, c0), (h1, c1)) = (pair[0], pair[1]);
        if above_sea < h1 {
            return lerp_rgb(c0, c1, (above_sea - h0) / (h1 - h0));
        }
    }
    STOPS[STOPS.len() - 1].1
}

// Grass in this climate, or the snow over it in the cold, under trees this dense, as an sRGB
// map colour. Biomes tint their grass and leaves, see `Biomes::grass_colour`.
fn grass_map_colour(biomes: &Biomes, climate: Climate, trees: f64) -> [f64; 3] {
    let canopy = (trees * 1.6).min(1.0) as f32;
    let ground = if climate.temperature < SNOWY_GRASS_TEMPERATURE {
        let snow = Vec3::from(SNOW) / 255.0;
        LinearRgba::from(Srgba::rgb(snow.x, snow.y, snow.z)).to_vec3()
    } else {
        biomes.grass_colour(climate)
    };
    let linear = ground.lerp(biomes.leaf_colour(climate), canopy);
    let srgb = Srgba::from(LinearRgba::rgb(linear.x, linear.y, linear.z)).to_vec3();
    (srgb.as_dvec3() * 255.0).into()
}
