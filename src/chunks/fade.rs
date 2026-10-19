use bevy::prelude::Resource;
use bevy::{
    asset::{Asset, Assets, Handle},
    camera::visibility::Visibility,
    ecs::hierarchy::Children,
    ecs::{
        component::Component,
        entity::Entity,
        system::{Commands, Query, Res},
    },
    math::Vec4,
    pbr::MeshMaterial3d,
    time::Time,
};

use super::material::{ChunkMaterial, TerrainMaterials, WaterMaterial};

// One step per level of the shaders' 8x8 dither pattern.
const FADE_STEPS: usize = 64;

/// The dither window that draws every pixel.
pub const FULLY_VISIBLE: Vec4 = Vec4::new(0.0, 1.0, 0.0, 0.0);

// A mesh fading out is drawn this fraction farther than it is, so the one replacing it wins where
// their surfaces coincide, as a LOD node's skirts and flat ground do with full detail.
const FADE_DEPTH_PUSH: f32 = 0.0005;

// A swap is staggered so one of the two meshes is always whole: the new one dithers in over the
// old for this share of the fade, then the old one dithers out from under it for the rest. So
// trees in the new mesh grow in over the old one's tint rather than popping up on top of it, and
// no pixel is ever given up by both.
const FADE_IN_SHARE: f32 = 0.5;

// The window drawn at `step` of a fade in: one more level of the dither is drawn each step.
fn fade_in_window(step: usize) -> Vec4 {
    Vec4::new(0.0, (step + 1) as f32 / FADE_STEPS as f32, 0.0, 0.0)
}

// The window drawn at `step` of a fade out, which hides `step` levels of the dither: whole at
// step 0, while what replaces it is still coming in, but pushed back already so that wins.
fn fade_out_window(step: usize) -> Vec4 {
    Vec4::new(step as f32 / FADE_STEPS as f32, 1.0, FADE_DEPTH_PUSH, 0.0)
}

// Which step of its fade, in or out, a mesh is at `progress` through it, or none once it's done.
fn fade_step(progress: f32, out: bool) -> Option<usize> {
    let progress = progress.max(0.0);
    let (start, end) = if out {
        (FADE_IN_SHARE, 1.0)
    } else {
        (0.0, FADE_IN_SHARE)
    };
    if progress >= end {
        return None;
    }
    if progress < start {
        // a fade out held whole while what replaces it comes in
        return Some(0);
    }
    let step = ((progress - start) / (end - start) * FADE_STEPS as f32) as usize;
    // once a fade out starts, one level is already gone, as one is already drawn as a fade in starts
    Some((step + out as usize).min(FADE_STEPS - 1))
}

#[derive(Resource)]
pub struct FadeMaterials {
    seconds: f32,
    chunk: Handle<ChunkMaterial>,
    water: Handle<WaterMaterial>,
    solid_water: Handle<WaterMaterial>,
    // a material for each step of a fade, indexed [out as usize][step]
    chunk_steps: [Vec<Handle<ChunkMaterial>>; 2],
    water_steps: [Vec<Handle<WaterMaterial>>; 2],
    solid_water_steps: [Vec<Handle<WaterMaterial>>; 2],
}

// The window drawn at `step` of a fade in or out.
fn window(out: bool, step: usize) -> Vec4 {
    if out {
        fade_out_window(step)
    } else {
        fade_in_window(step)
    }
}

impl FadeMaterials {
    pub fn new(
        seconds: f32,
        materials: &TerrainMaterials,
        chunk_materials: &mut Assets<ChunkMaterial>,
        water_materials: &mut Assets<WaterMaterial>,
    ) -> Self {
        let TerrainMaterials {
            chunk,
            water,
            solid_water,
        } = materials.clone();
        let water_fade = |m: WaterMaterial, fade: Vec4| WaterMaterial { fade, ..m };
        Self {
            seconds,
            chunk_steps: fade_steps(chunk_materials, &chunk, |m, fade| ChunkMaterial {
                fade,
                ..m
            }),
            water_steps: fade_steps(water_materials, &water, water_fade),
            solid_water_steps: fade_steps(water_materials, &solid_water, water_fade),
            chunk,
            water,
            solid_water,
        }
    }
}

// A copy of the material at `base` for each step of a fade in, then of a fade out.
fn fade_steps<M: Asset + Clone>(
    materials: &mut Assets<M>,
    base: &Handle<M>,
    with_fade: impl Fn(M, Vec4) -> M,
) -> [Vec<Handle<M>>; 2] {
    let base = materials.get(base).unwrap().clone();
    [false, true].map(|out| {
        (0..FADE_STEPS)
            .map(|step| materials.add(with_fade(base.clone(), window(out, step))))
            .collect()
    })
}

/// On a water mesh drawn with the solid material.
#[derive(Component)]
pub struct SolidWater;

#[derive(Component)]
pub struct Fade {
    start: f32,
    out: bool,
    despawn: bool,
}

/// Fades `entity` in or out unless it's already shown or hidden as asked. The two are staggered,
/// see `FADE_IN_SHARE`, so a mesh and the one replacing it, set going together, never both leave
/// a pixel undrawn.
pub fn set_shown(
    commands: &mut Commands,
    entity: Entity,
    visibility: &mut Visibility,
    fade: Option<&Fade>,
    shown: bool,
    now: f32,
) {
    // one fading out to be despawned is gone however the rules would show it, or it would linger unowned
    if fade.is_some_and(|f| f.despawn) {
        return;
    }
    let currently_shown = *visibility != Visibility::Hidden && !fade.is_some_and(|f| f.out);
    if currently_shown == shown {
        return;
    }
    if shown {
        // one fading out is brought back by finishing a fade in at once, which resets its material
        let start = if fade.is_some() { f32::MIN } else { now };
        *visibility = Visibility::Inherited;
        commands.entity(entity).insert(Fade {
            start,
            out: false,
            despawn: false,
        });
    } else {
        commands.entity(entity).insert(Fade {
            start: now,
            out: true,
            despawn: false,
        });
    }
}

pub fn fade_out_and_despawn(commands: &mut Commands, entity: Entity, visible: bool, now: f32) {
    if visible {
        commands.entity(entity).insert(Fade {
            start: now,
            out: true,
            despawn: true,
        });
    } else {
        commands.entity(entity).despawn();
    }
}

pub fn apply_fades(
    mut commands: Commands,
    time: Res<Time>,
    materials: Res<FadeMaterials>,
    mut fading: Query<(Entity, &Fade, &mut Visibility, Option<&Children>)>,
    mut chunk_materials: Query<&mut MeshMaterial3d<ChunkMaterial>>,
    mut water_materials: Query<(
        &mut MeshMaterial3d<WaterMaterial>,
        bevy::ecs::query::Has<SolidWater>,
    )>,
) {
    let now = time.elapsed_secs();

    for (entity, fade, mut visibility, children) in fading.iter_mut() {
        let progress = (now - fade.start) / materials.seconds.max(f32::EPSILON);
        let (chunk, water, solid_water) = match fade_step(progress, fade.out) {
            None => {
                if fade.out && fade.despawn {
                    commands.entity(entity).despawn();
                    continue;
                }
                if fade.out {
                    *visibility = Visibility::Hidden;
                }
                commands.entity(entity).remove::<Fade>();
                (&materials.chunk, &materials.water, &materials.solid_water)
            }
            Some(step) => {
                let out = fade.out as usize;
                (
                    &materials.chunk_steps[out][step],
                    &materials.water_steps[out][step],
                    &materials.solid_water_steps[out][step],
                )
            }
        };

        if let Ok(mut material) = chunk_materials.get_mut(entity) {
            if material.0 != *chunk {
                material.0 = chunk.clone();
            }
        }
        // water and the faces between leaves are drawn by child entities with their own materials
        for &child in children.into_iter().flatten() {
            if let Ok(mut material) = chunk_materials.get_mut(child) {
                if material.0 != *chunk {
                    material.0 = chunk.clone();
                }
            }
            if let Ok((mut material, solid)) = water_materials.get_mut(child) {
                let water = if solid { solid_water } else { water };
                if material.0 != *water {
                    material.0 = water.clone();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        camera::visibility::Visibility,
        ecs::{
            system::Commands,
            world::{CommandQueue, World},
        },
    };

    use super::{fade_in_window, fade_out_window, fade_step, set_shown, Fade, FADE_STEPS};

    // Every dither threshold the shader can produce, (level + 0.5) / 64.
    fn thresholds() -> impl Iterator<Item = f32> {
        (0..FADE_STEPS).map(|level| (level as f32 + 0.5) / FADE_STEPS as f32)
    }

    fn draws(window: bevy::math::Vec4, threshold: f32) -> bool {
        threshold >= window.x && threshold < window.y
    }

    #[test]
    fn test_fade_in_draws_one_more_level_per_step() {
        for step in 0..FADE_STEPS {
            let drawn = thresholds()
                .filter(|&t| draws(fade_in_window(step), t))
                .count();
            assert_eq!(step + 1, drawn);
        }
    }

    #[test]
    fn test_fade_out_hides_one_more_level_per_step() {
        for step in 0..FADE_STEPS {
            let hidden = thresholds()
                .filter(|&t| !draws(fade_out_window(step), t))
                .count();
            assert_eq!(step, hidden);
        }
    }

    #[test]
    fn test_one_of_a_swapped_pair_is_always_whole() {
        // a mesh and its replacement, set fading together, at every point through the fade
        for i in 0..=1000 {
            let progress = i as f32 / 1000.0;
            let coming = fade_step(progress, false).map_or(FADE_STEPS, |step| step + 1);
            let going = fade_step(progress, true).unwrap_or(FADE_STEPS);
            assert!(
                coming == FADE_STEPS || going == 0,
                "at {progress}: {coming} drawn and {going} hidden"
            );
        }
    }

    #[test]
    fn test_a_fade_in_finishes_as_the_fade_out_starts() {
        assert_eq!(None, fade_step(0.5, false));
        assert_eq!(Some(1), fade_step(0.5, true));
        assert_eq!(Some(0), fade_step(0.49, true));
        assert_eq!(None, fade_step(1.0, true));
    }

    mod swaps {
        use bevy::asset::{Assets, Handle};
        use bevy::camera::visibility::Visibility;
        use bevy::color::LinearRgba;
        use bevy::ecs::entity::Entity;
        use bevy::ecs::hierarchy::{ChildOf, Children};
        use bevy::ecs::system::{Commands, RunSystemOnce};
        use bevy::ecs::world::{CommandQueue, World};
        use bevy::math::Vec4;
        use bevy::pbr::MeshMaterial3d;
        use bevy::time::Time;
        use std::time::Duration;

        use super::draws;
        use super::thresholds;
        use crate::chunks::fade::{
            apply_fades, fade_out_and_despawn, set_shown, Fade, FadeMaterials, SolidWater,
            FULLY_VISIBLE,
        };
        use crate::chunks::material::{ChunkMaterial, TerrainMaterials, WaterMaterial};
        use crate::settings::DebugSettings;

        const SECONDS: f32 = 1.5;

        // A world with fade materials, and one drawn mesh: a group or node with its water and
        // the faces between its leaves as children, as `load_groups` and `draw_meshes` draw them.
        struct Swap {
            world: World,
            materials: TerrainMaterials,
        }

        impl Swap {
            fn new() -> Self {
                let mut world = World::new();
                let mut chunks = Assets::<ChunkMaterial>::default();
                let mut waters = Assets::<WaterMaterial>::default();
                let chunk = chunks.add(ChunkMaterial {
                    color: LinearRgba::WHITE,
                    texture: None,
                    fade: FULLY_VISIBLE,
                    cloud_pattern: Handle::default(),
                    cloud_layer: Handle::default(),
                    curvature: Handle::default(),
                    debug: DebugSettings::default(),
                });
                let water = WaterMaterial {
                    color: LinearRgba::WHITE,
                    texture: None,
                    fade: FULLY_VISIBLE,
                    cloud_pattern: Handle::default(),
                    cloud_layer: Handle::default(),
                    see_through: Vec4::ZERO,
                    curvature: Handle::default(),
                    solid: false,
                };
                let materials = TerrainMaterials {
                    chunk,
                    water: waters.add(water.clone()),
                    solid_water: waters.add(WaterMaterial {
                        solid: true,
                        ..water
                    }),
                };
                let fades = FadeMaterials::new(SECONDS, &materials, &mut chunks, &mut waters);
                world.insert_resource(chunks);
                world.insert_resource(waters);
                world.insert_resource(fades);
                world.insert_resource(Time::<()>::default());
                Self { world, materials }
            }

            fn mesh(&mut self, visibility: Visibility, solid_water: bool) -> Entity {
                let parent = self
                    .world
                    .spawn((visibility, MeshMaterial3d(self.materials.chunk.clone())))
                    .id();
                // the faces between its leaves
                self.world.spawn((
                    Visibility::Inherited,
                    MeshMaterial3d(self.materials.chunk.clone()),
                    ChildOf(parent),
                ));
                let water = if solid_water {
                    &self.materials.solid_water
                } else {
                    &self.materials.water
                };
                let mut water = self.world.spawn((
                    Visibility::Inherited,
                    MeshMaterial3d(water.clone()),
                    ChildOf(parent),
                ));
                if solid_water {
                    water.insert(SolidWater);
                }
                parent
            }

            fn now(&self) -> f32 {
                self.world.resource::<Time>().elapsed_secs()
            }

            fn set_shown(&mut self, entity: Entity, shown: bool) {
                let now = self.now();
                let mut visibility = *self.world.get::<Visibility>(entity).unwrap();
                let fade = self.world.get::<Fade>(entity).map(|f| Fade { ..*f });
                let mut queue = CommandQueue::default();
                set_shown(
                    &mut Commands::new(&mut queue, &self.world),
                    entity,
                    &mut visibility,
                    fade.as_ref(),
                    shown,
                    now,
                );
                queue.apply(&mut self.world);
                *self.world.get_mut::<Visibility>(entity).unwrap() = visibility;
            }

            fn frame(&mut self) {
                self.world
                    .resource_mut::<Time>()
                    .advance_by(Duration::from_secs_f32(1.0 / 60.0));
                self.world.run_system_once(apply_fades).unwrap();
            }

            // The windows `entity` and each of its children draw, or None where it's hidden or gone.
            fn windows(&self, entity: Entity) -> Option<Vec<Vec4>> {
                let shown = *self.world.get::<Visibility>(entity)? != Visibility::Hidden;
                let window = |e: Entity| {
                    if let Some(m) = self.world.get::<MeshMaterial3d<ChunkMaterial>>(e) {
                        let chunks = self.world.resource::<Assets<ChunkMaterial>>();
                        return chunks.get(&m.0).unwrap().fade;
                    }
                    let m = self.world.get::<MeshMaterial3d<WaterMaterial>>(e).unwrap();
                    let waters = self.world.resource::<Assets<WaterMaterial>>();
                    let water = waters.get(&m.0).unwrap();
                    assert_eq!(
                        self.world.get::<SolidWater>(e).is_some(),
                        water.solid,
                        "water drawn with the wrong material"
                    );
                    water.fade
                };
                let children = self.world.get::<Children>(entity).unwrap();
                shown.then(|| {
                    std::iter::once(entity)
                        .chain(children.iter().copied())
                        .map(window)
                        .collect()
                })
            }

            // Runs `old` and `new` through their fades, checking every frame that no dither
            // level is left undrawn by both, that where both draw the one going is pushed back,
            // and that each one's children fade with it.
            fn check(&mut self, old: Entity, new: Entity) {
                for frame in 0..(SECONDS * 60.0) as usize + 10 {
                    self.frame();
                    let at = format!("frame {frame}");
                    let (going, coming) = (self.windows(old), self.windows(new));
                    for windows in [&going, &coming].into_iter().flatten() {
                        assert!(
                            windows.iter().all(|w| w == &windows[0]),
                            "{at}: {windows:?}"
                        );
                    }
                    let coming = coming.expect("the new mesh is hidden")[0];
                    for t in thresholds() {
                        let old_draws = going.as_ref().is_some_and(|w| draws(w[0], t));
                        assert!(old_draws || draws(coming, t), "{at}: {t} undrawn");
                        if old_draws && draws(coming, t) {
                            let old = going.as_ref().unwrap()[0];
                            assert!(old.z > coming.z, "{at}: {old} not behind {coming}");
                        }
                    }
                }
                assert!(!self.world.entity(new).contains::<Fade>());
                assert_eq!(Some(vec![FULLY_VISIBLE; 3]), self.windows(new));
            }
        }

        #[test]
        fn test_a_mesh_and_its_replacement_cross_fade_with_no_gap() {
            for solid_water in [false, true] {
                let mut swap = Swap::new();
                swap.frame();
                let (old, new) = (
                    swap.mesh(Visibility::Inherited, solid_water),
                    swap.mesh(Visibility::Hidden, solid_water),
                );
                swap.set_shown(old, false);
                swap.set_shown(new, true);
                swap.check(old, new);
                assert_eq!(None, swap.windows(old), "the old mesh is still drawn");
                assert!(!swap.world.entity(old).contains::<Fade>());
            }
        }

        #[test]
        fn test_a_retired_node_cross_fades_with_what_replaces_it_and_is_despawned() {
            let mut swap = Swap::new();
            swap.frame();
            let (old, new) = (
                swap.mesh(Visibility::Inherited, true),
                swap.mesh(Visibility::Hidden, true),
            );
            let now = swap.now();
            let mut queue = CommandQueue::default();
            fade_out_and_despawn(&mut Commands::new(&mut queue, &swap.world), old, true, now);
            queue.apply(&mut swap.world);
            swap.set_shown(new, true);
            swap.check(old, new);
            assert!(swap.world.get_entity(old).is_err());
        }

        #[test]
        fn test_a_swap_reversed_halfway_leaves_no_gap() {
            // the camera turning back mid-fade: what was going comes back at once, and what was
            // coming goes, held whole first, so neither is ever short
            let mut swap = Swap::new();
            swap.frame();
            let (a, b) = (
                swap.mesh(Visibility::Inherited, false),
                swap.mesh(Visibility::Hidden, false),
            );
            swap.set_shown(a, false);
            swap.set_shown(b, true);
            for _ in 0..(SECONDS * 60.0 * 0.3) as usize {
                swap.frame();
            }
            swap.set_shown(a, true);
            swap.set_shown(b, false);
            swap.check(b, a);
        }
    }

    // `set_shown` for an entity with this visibility and fade, then what's left of it.
    fn shown(
        visibility: Visibility,
        fade: Option<Fade>,
        shown: bool,
    ) -> (Visibility, Option<(f32, bool)>) {
        let mut world = World::new();
        let entity = match fade {
            Some(fade) => world.spawn((visibility, fade)).id(),
            None => world.spawn(visibility).id(),
        };
        let mut queue = CommandQueue::default();
        let mut v = visibility;
        let existing = world.get::<Fade>(entity).map(|f| Fade {
            start: f.start,
            out: f.out,
            despawn: f.despawn,
        });
        set_shown(
            &mut Commands::new(&mut queue, &world),
            entity,
            &mut v,
            existing.as_ref(),
            shown,
            5.0,
        );
        queue.apply(&mut world);
        (v, world.get::<Fade>(entity).map(|f| (f.start, f.out)))
    }

    #[test]
    fn test_showing_fades_in_from_now() {
        assert_eq!(
            (Visibility::Inherited, Some((5.0, false))),
            shown(Visibility::Hidden, None, true)
        );
    }

    #[test]
    fn test_hiding_fades_out_from_now() {
        let (_, fade) = shown(Visibility::Inherited, None, false);
        assert_eq!(Some((5.0, true)), fade);
    }

    #[test]
    fn test_bringing_back_one_fading_out_finishes_its_fade_at_once() {
        let fading = Fade {
            start: 3.0,
            out: true,
            despawn: false,
        };
        let (visibility, fade) = shown(Visibility::Inherited, Some(fading), true);
        assert_eq!(Visibility::Inherited, visibility);
        // a fade that's long over, which resets its material
        assert_eq!(Some((f32::MIN, false)), fade);
    }

    #[test]
    fn test_one_fading_out_to_despawn_is_never_brought_back() {
        let fading = Fade {
            start: 3.0,
            out: true,
            despawn: true,
        };
        let (_, fade) = shown(Visibility::Inherited, Some(fading), true);
        assert_eq!(Some((3.0, true)), fade);
    }
}
