use super::*;
#[cfg(feature = "client")]
use crate::chunks::chunk::CHUNK_SIZE;
use crate::chunks::generate::WORLD_HEIGHT;
#[cfg(feature = "client")]
use crate::curvature::WORLD_CURVE;
#[cfg(feature = "client")]
use crate::horizon::render_distance_at;

// Seeds enough to see the spread of what's placed.
const SEEDS: std::ops::Range<u32> = 0..8;
// The render distance the game starts with, in chunks, see `settings.toml`.
#[cfg(feature = "client")]
const SET_RENDER_DISTANCE: f32 = 1024.0;

fn peaks(landmarks: &Landmarks) -> impl Iterator<Item = &Peak> {
    landmarks.all().filter_map(|l| match l {
        Landmark::Peak(peak) => Some(peak),
        _ => None,
    })
}

// The highest ground near a landmark's middle, where its summit or rim is.
fn top(noise: &NoiseGenerator, landmark: &Landmark) -> f64 {
    let centre = landmark.centre();
    (-20..=20)
        .flat_map(|i| (-20..=20).map(move |j| centre + DVec2::new(i as f64, j as f64) * 12.0))
        .map(|at| noise.sample(at.x, at.y).height)
        .fold(f64::MIN, f64::max)
}

#[test]
fn test_landmarks_are_the_same_every_time_and_differ_between_worlds() {
    let describe = |seed| {
        let noise = NoiseGenerator::new(seed);
        let all: Vec<_> = noise.landmarks().all().map(|l| format!("{l:?}")).collect();
        all
    };
    assert_eq!(describe(3), describe(3));
    assert_ne!(describe(3), describe(4));
}

#[test]
fn test_landmarks_are_rare_far_apart_and_each_inside_its_cell() {
    let (mut kinds, mut total) = ([0; 3], 0);
    for seed in SEEDS {
        let noise = NoiseGenerator::new(seed);
        let all: Vec<_> = noise.landmarks().all().collect();
        // a dozen or so in a world of a thousand square kilometres, half of it sea
        assert!((6..=16).contains(&all.len()), "seed {seed}: {}", all.len());
        total += all.len();
        for (i, landmark) in all.iter().enumerate() {
            kinds[match landmark {
                Landmark::Peak(_) => 0,
                Landmark::Volcano(_) => 1,
                Landmark::GiantTree(_) => 2,
            }] += 1;
            // all it changes is inside its own cell
            let centre = landmark.centre();
            let cell = (centre / LANDMARK_CELL as f64).floor();
            let (low, high) = (
                cell * LANDMARK_CELL as f64,
                (cell + 1.0) * LANDMARK_CELL as f64,
            );
            assert!((centre - landmark.reach()).cmpge(low).all());
            assert!((centre + landmark.reach()).cmple(high).all());
            for other in &all[i + 1..] {
                let apart = centre.distance(other.centre());
                assert!(apart > landmark.reach() + other.reach(), "{apart}");
            }
        }
    }
    // every kind, none much commoner than the others
    assert!(
        kinds.iter().all(|&n| n * 6 >= total),
        "{kinds:?} of {total}"
    );
}

#[test]
fn test_landmarks_stand_where_they_suit() {
    for seed in SEEDS {
        let noise = NoiseGenerator::without_landmarks(seed);
        for landmark in NoiseGenerator::new(seed).landmarks().all() {
            let at = landmark.centre();
            let sample = noise.sample(at.x, at.y);
            let above_sea = sample.height - WATER_LEVEL as f64;
            assert!(above_sea > 0.0, "{landmark:?} at sea");
            if let Landmark::GiantTree(_) = landmark {
                let biome = noise.biomes().name(sample.biome);
                assert!(matches!(biome, Some("forest" | "jungle")), "{biome:?}");
            }
        }
    }
}

#[test]
fn test_every_world_has_a_peak_or_a_volcano_in_sight_of_its_spawn() {
    for seed in SEEDS {
        let noise = NoiseGenerator::new(seed);
        let (x, z, _) = noise.find_spawn();
        let spawn = DVec2::new(x as f64, z as f64);
        let nearest = noise
            .landmarks()
            .all()
            .filter(|l| !matches!(l, Landmark::GiantTree(_)))
            .map(|l| l.centre().distance(spawn))
            .fold(f64::MAX, f64::min);
        // well inside the land drawn at the default render distance, 16 km
        assert!(nearest < 14_000.0, "seed {seed}: {nearest}");
    }
}

#[test]
fn test_peaks_tower_over_the_land_round_them_and_volcanoes_hold_magma() {
    for seed in SEEDS {
        let noise = NoiseGenerator::new(seed);
        let flat = NoiseGenerator::without_landmarks(seed);
        for landmark in noise.landmarks().all() {
            let at = landmark.centre();
            // over the ground that was at its middle
            let rise = top(&noise, landmark) - flat.sample(at.x, at.y).height;
            match landmark {
                Landmark::Peak(_) => {
                    assert!(rise > 300.0, "{rise}");
                    assert!(top(&noise, landmark) < WORLD_HEIGHT as f64);
                    let biome = noise.sample(at.x, at.y).biome;
                    assert_eq!(Some("summit"), noise.biomes().name(biome));
                }
                Landmark::Volcano(_) => {
                    assert!(rise > 250.0, "{rise}");
                    let biome = noise.sample(at.x, at.y).biome;
                    assert_eq!(Some("crater"), noise.biomes().name(biome));
                }
                Landmark::GiantTree(_) => assert_eq!(top(&flat, landmark), top(&noise, landmark)),
            }
        }
    }
}

#[test]
fn test_landmarks_blend_into_the_land_without_a_seam() {
    let noise = NoiseGenerator::new(3);
    let flat = NoiseGenerator::without_landmarks(3);
    for landmark in noise.landmarks().all() {
        if let Landmark::GiantTree(_) = landmark {
            continue;
        }
        let reach = landmark.reach();
        for direction in [DVec2::X, DVec2::NEG_Y, DVec2::new(0.6, 0.8)] {
            let at = |d: f64| landmark.centre() + direction * d;
            let mut last = None;
            for d in (0..(reach + 64.0) as i64).map(|d| d as f64) {
                let p = at(d);
                let (shaped, before) = (noise.sample(p.x, p.y), flat.sample(p.x, p.y));
                // the land as it was past its reach
                if d >= reach {
                    assert_eq!(shaped.height.to_bits(), before.height.to_bits(), "{d}");
                }
                // and nowhere a step much steeper than the land under it has
                let raised = shaped.height - before.height;
                if let Some(last) = last {
                    let step: f64 = raised - last;
                    assert!(step.abs() < 9.0, "a step of {step} {d} out");
                }
                last = Some(raised);
            }
        }
    }
}

#[test]
fn test_most_of_the_world_is_left_as_it_was() {
    // the cheap reject: away from landmarks a column is exactly what it was without them
    let noise = NoiseGenerator::new(3);
    let flat = NoiseGenerator::without_landmarks(3);
    let (mut near, mut total) = (0, 0);
    for i in -60..60 {
        for j in -60..60 {
            let (x, z) = (i as f64 * 271.0 + 13.0, j as f64 * 263.0 - 7.0);
            let inside = noise
                .landmarks()
                .all()
                .any(|l| l.centre().distance(DVec2::new(x, z)) < l.reach());
            total += 1;
            if inside {
                near += 1;
                continue;
            }
            let (a, b) = (noise.sample(x, z), flat.sample(x, z));
            assert_eq!(a.height.to_bits(), b.height.to_bits());
            assert_eq!((a.biome, a.trees.to_bits()), (b.biome, b.trees.to_bits()));
        }
    }
    assert!(near * 10 < total, "{near} of {total} within reach of one");
}

#[test]
fn test_coarse_cells_keep_a_peaks_height() {
    // a coarse LOD cell sees the mean of the ground it covers, so a peak keeps its shape from
    // level to level, and the coarsest still stand nearly as tall as the summit
    let noise = NoiseGenerator::new(3);
    for peak in peaks(noise.landmarks()) {
        let summit = top(&noise, &Landmark::Peak(peak.clone()));
        for cell in [16.0, 64.0, 128.0] {
            let (mut error, mut count) = (0.0, 0);
            for i in -6..=6 {
                let p = peak.centre + DVec2::new(i as f64 * 97.0, i as f64 * -61.0);
                let mean = (0..8)
                    .flat_map(|a| (0..8).map(move |b| (a, b)))
                    .map(|(a, b)| {
                        let offset = DVec2::new(a as f64 - 3.5, b as f64 - 3.5) * cell / 8.0;
                        noise.sample(p.x + offset.x, p.y + offset.y).height
                    })
                    .sum::<f64>()
                    / 64.0;
                error += (noise.sample_for_cell(p.x, p.y, cell).height - mean).abs();
                count += 1;
            }
            let error = error / count as f64;
            assert!(error < 12.0, "cells {cell} wide off by {error}");
            let coarse_top = (-4..=4)
                .flat_map(|i| (-4..=4).map(move |j| DVec2::new(i as f64, j as f64) * cell))
                .map(|o| noise.sample_for_cell(peak.centre.x + o.x, peak.centre.y + o.y, cell))
                .map(|s| s.height)
                .fold(f64::MIN, f64::max);
            assert!(
                summit - coarse_top < 60.0,
                "{cell}: {coarse_top} under {summit}"
            );
        }
    }
}

// how far the land's drawn is the client's business, so the server's build leaves this out
#[cfg(feature = "client")]
#[test]
fn test_a_great_peak_rises_over_the_horizon_from_far_off() {
    // Seen from the ground or from a height, at nine tenths of the way to the edge of the land
    // drawn, inside the fog's last fade, a great peak's summit stands over lowland anywhere
    // between, as the curved world brings both down (see `curvature`), and over the land's edge.
    for seed in SEEDS {
        let noise = NoiseGenerator::new(seed);
        for peak in peaks(noise.landmarks()) {
            let summit = top(&noise, &Landmark::Peak(peak.clone())) as f32;
            for eye_above_sea in [5.0, 100.0, 300.0, 500.0] {
                let eye = WATER_LEVEL as f32 + eye_above_sea;
                let drawn = render_distance_at(SET_RENDER_DISTANCE, eye) * CHUNK_SIZE as f32;
                let far = drawn * 0.9;
                let angle = |height: f32, distance: f32| {
                    (height - WORLD_CURVE.drop_at(distance, eye) - eye) / distance
                };
                let seen = angle(summit, far);
                assert!(seen > -WORLD_CURVE.edge_dip(eye, drawn), "{eye} {far}");
                let lowland = WATER_LEVEL as f32 + 40.0;
                for step in 1..=20 {
                    let between = 2000.0 + (far - 2000.0) * step as f32 / 20.0;
                    assert!(
                        seen > angle(lowland, between),
                        "seed {seed}: from {eye} the summit {far} off is under lowland {between} off"
                    );
                }
            }
        }
    }
}

#[test]
fn test_structures_and_caves_keep_out_of_landmarks() {
    let noise = NoiseGenerator::new(3);
    for landmark in noise.landmarks().all() {
        let at = landmark.centre().as_i64vec2();
        assert!(noise.landmarks().keeps_clear(at));
        assert!(!noise.landmarks().keeps_clear(at + LANDMARK_CELL / 2));
        // shut under a crater and a giant tree's trunk, but not a peak
        let roof = noise.landmarks().cave_roof(at.x, at.y, || 500);
        assert_eq!(!matches!(landmark, Landmark::Peak(_)), roof.is_some());
        assert!(roof.is_none_or(|roof| roof <= 500));
    }
}
