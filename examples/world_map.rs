// Renders the world's terrain heights to a shaded PNG, so terrain
// changes can be judged across the whole world in seconds instead of by flying around.
// Run with:
//   cargo run --release --example world_map -- --seed 42
// Zoom in on an area with a smaller --scale (blocks per pixel):
//   cargo run --release --example world_map -- --seed 42 --center 3000,-2000 --scale 2
// Colour lowlands by biome, with grass tinted by climate as it's drawn in game:
//   cargo run --release --example world_map -- --seed 42 --biomes

use std::env;

use bevy::math::DVec2;

use rustcraft::chunks::generate::biome::Biomes;
use rustcraft::chunks::generate::noise::{NoiseGenerator, TerrainSample, WORLD_SIZE};
use rustcraft::chunks::generate::WATER_LEVEL;
use rustcraft::map::relief::Relief;

struct Args {
    seed: u32,
    center: (f64, f64),
    scale: f64,
    pixels: u32,
    out: String,
    biomes: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        seed: 42,
        center: (0.0, 0.0),
        scale: (WORLD_SIZE as f64 * 1.1 / 2048.0).ceil(),
        pixels: 2048,
        out: "world_map.png".to_string(),
        biomes: false,
    };

    let mut argv = env::args().skip(1);
    while let Some(flag) = argv.next() {
        if flag == "--biomes" {
            args.biomes = true;
            continue;
        }
        let value = argv
            .next()
            .unwrap_or_else(|| panic!("missing value for {flag}"));
        match flag.as_str() {
            "--seed" => args.seed = value.parse().expect("--seed must be a u32"),
            "--center" => {
                let (x, z) = value.split_once(',').expect("--center must be x,z");
                args.center = (x.parse().expect("invalid x"), z.parse().expect("invalid z"));
            }
            "--scale" => args.scale = value.parse().expect("--scale must be a number"),
            "--pixels" => args.pixels = value.parse().expect("--pixels must be a u32"),
            "--out" => args.out = value,
            other => panic!("unknown flag {other}"),
        }
    }
    args
}

fn render(relief: &Relief, biomes: &Biomes, args: &Args) -> image::RgbImage {
    let biomes = args.biomes.then_some(biomes);
    image::RgbImage::from_fn(args.pixels, args.pixels, |x, z| {
        image::Rgb(relief.colour(x as usize, z as usize, biomes))
    })
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

fn print_stats(relief: &Relief, biomes: &Biomes) {
    let samples: Vec<TerrainSample> = relief.terrain().copied().collect();
    let land = samples
        .iter()
        .filter(|s| s.height >= WATER_LEVEL as f64)
        .count();
    println!("land: {:.1}%", 100.0 * land as f64 / samples.len() as f64);
    for name in biomes.names() {
        let count = samples
            .iter()
            .filter(|s| s.height >= WATER_LEVEL as f64 && biomes.name(s.biome) == Some(name))
            .count();
        println!(
            "{name}: {:.1}% of land",
            100.0 * count as f64 / land.max(1) as f64
        );
    }

    let mut heights: Vec<f64> = samples.iter().map(|s| s.height).collect();
    heights.sort_by(f64::total_cmp);
    let mut continents: Vec<f64> = samples.iter().map(|s| s.continentalness).collect();
    continents.sort_by(f64::total_cmp);
    for p in [0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 0.99, 1.0] {
        println!(
            "p{:<3} height={:6.1} continentalness={:6.3}",
            (p * 100.0) as u32,
            percentile(&heights, p),
            percentile(&continents, p)
        );
    }
}

fn main() {
    let args = parse_args();
    let noise = NoiseGenerator::new(args.seed);

    let start = std::time::Instant::now();
    let half = args.pixels as f64 / 2.0 * args.scale;
    let corner = DVec2::new(args.center.0 - half, args.center.1 - half);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let relief = Relief::sample(&noise, corner, args.scale, args.pixels as usize, threads);
    println!(
        "sampled {}x{} in {:.1?}",
        args.pixels,
        args.pixels,
        start.elapsed()
    );

    print_stats(&relief, noise.biomes());
    let spawn = noise.find_spawn();
    println!("spawn: x={} z={} height={:.1}", spawn.0, spawn.1, spawn.2);

    render(&relief, noise.biomes(), &args)
        .save(&args.out)
        .expect("failed to save image");
    println!("wrote {}", args.out);
}
