---
name: sounds
description: Synthesise or change rustcraft's sounds (footsteps and breaking by surface, splashes, pickups, eating, hurt, chests, clicks, creature calls, ambience loops and music) with synth.py into assets/sounds/, and check them by analysis and spectrograms. Use when adding a sound, a surface or a creature call, or restyling or fixing one.
---

# Sounds

Nearly every sound the game plays is made by `synth.py` from filtered noise, struck modes,
bubbles, chirps and plucked strings, so it's ours to ship (INV-8). Minecraft and its resource packs
(Enhanced Audio among them) are inspiration only (D-10). A CC0 recording is allowed where a
synthesised sound is clearly worse, credited in `assets/CREDITS.md`, and the ambience has some
(below). The wind isn't a file at all: `src/client/sound/ambience/wind.rs` makes it as it plays.

## Setup

Python with numpy, scipy, soundfile (libsndfile with Vorbis) and matplotlib. On NixOS:

```
nix-shell -p 'python3.withPackages (p: [p.numpy p.scipy p.soundfile p.matplotlib])'
```

## Make

```
cd .claude/skills/sounds
python synth.py                    # everything, under a minute
python synth.py step/stone break   # only paths starting with these
python synth.py --wav out/         # WAVs too, to listen to
```

Each path seeds its own random numbers, so a run makes the same files and changing one sound
leaves the rest alone. `catalogue()` lists every file. One-off sounds have `VARIATIONS` (4) takes,
named `<name>_1.ogg` to `_4`; `src/client/sound/mod.rs` has the same count and the file names in
`SoundKind::sound`, and its `test_every_sound_has_its_files` checks they're all there. Surfaces are
`SURFACES`, matching `Surface` in `src/client/sound/surface.rs`.

- **Levels:** each kind is levelled by `level` to a short-term loudness (the loudest 50 ms, roughly
  K-weighted): footsteps -21, breaking -16, small sounds -22 to -26, loops -27 and music -24 over
  their length, peaks pulled in under -1 dBFS. How loud each plays against the others is then
  `SoundKind::sound`'s volume in the game, and the play-time pitch range keeps takes varied.
- **Loops** are periodic by construction: noise shaped in the frequency domain (`periodic_band`),
  LFOs with whole cycles in the loop (`lfo`), events placed round the end (`place(..., wrap=True)`),
  reverb by circular convolution and IIR filters run round the loop (`around`). Keep to these, or
  the loop clicks where it repeats.
- **Creatures:** animals have a call each (`bleat`, `moo`, `oink`, `cluck`), pitched up in the game
  when hurt. The hostile kinds (`HOSTILES`: M3.3's `gloamer`, `lurker`, `slinger`,
  `barrow_brute`, and M4.10's `dunewight`, `rimewraith`, `canopy_spitter`, `listener`,
  `drowner`) have four cries each, `<kind>_call`, `_hurt`, `_attack` and `_death`, from
  functions of those names, voiced through `formants`, with `clicks` for the lurker, spitter and
  listener, `crumble` for the brute, `wheeze` and sand `grains` for the dunewight, `whistle`
  and `ice` for the rimewraith and `gurgle` for the drowner. A new kind goes on the end of
  `HOSTILES`, which seeds them in order, so the others' files stay as they are.
- **Music:** pads under a few plucked or bell notes wandering a pentatonic scale, in a big room,
  about a minute each, played rarely (`src/client/sound/ambience.rs`).

## Recordings

The ambience's sea, birds, crickets, under water, desert and leaves loops and the owl's hoot are
CC0 recordings ported from upstream rustcraft (#77), cut and looped there, listed with their
sources in `assets/CREDITS.md`. `synth.py` doesn't make them (it makes only the cave's loop of
the ambience), so a full run leaves them be. `recordings.py SOURCE_DIR` levels the originals to
the targets below, once, as one is added; don't run it again on its own output. Upstream's own
volume balance between them is `Loop::volume` in `src/client/sound/ambience/mod.rs`.

The wind is the decoder in `ambience/wind.rs`: `RC_WIND_OUT=dir cargo test --release
render_wind -- --ignored` writes two minutes of it at a few strengths as raw 44.1 kHz f32 stereo,
to look at as below (`np.fromfile(..., dtype="<f4").reshape(-1, 2)`).

## Check

You can't listen, so look:

```
python analyse.py out/             # a table, and spectrogram sheets out/<group>.png
python analyse.py out/ ambience
```

The table has each file's length, peak, loudness (as `level` measures it), clipped samples,
spectral centroid and, for loops, how big the jump is where the end meets the start (against a big
sample-to-sample step: under 1 is no click) and the level either side of it. Read the sheets
(`step.png`, `break.png`, `misc.png`, `ambience.png`, `music.png`, 70 dB from each one's peak):
footsteps should be short bursts that die away within 0.15 s, breaks fuller and longer, loops
even, with no stripe at the ends.

Keep `assets/sounds/` small (about 5 MB now, 2 MB of it the recorded loops): one-offs are mono at Vorbis quality 0.6, loops 0.4
and music 0.3 (`quality` in `main`, as libsndfile's compression level).
