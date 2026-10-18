---
name: sounds
description: Synthesise or change rustcraft's sounds (footsteps and breaking by surface, splashes, pickups, eating, hurt, chests, clicks, creature calls, ambience loops and music) with synth.py into assets/sounds/, and check them by analysis and spectrograms. Use when adding a sound, a surface or a creature call, or restyling or fixing one.
---

# Sounds

Every sound the game plays is made by `synth.py` from filtered noise, struck modes, bubbles,
chirps and plucked strings, so it's ours to ship (INV-8). Nothing is recorded or taken from a pack:
Minecraft and its resource packs (Enhanced Audio among them) are inspiration only (D-10). A Kenney
CC0 file is allowed if a synthesised one is clearly worse, credited in `assets/CREDITS.md`.

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
- **Music:** pads under a few plucked or bell notes wandering a pentatonic scale, in a big room,
  about a minute each, played rarely (`src/client/sound/ambience.rs`).

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

Keep `assets/sounds/` small (about 3.5 MB now): one-offs are mono at Vorbis quality 0.6, loops 0.4
and music 0.3 (`quality` in `main`, as libsndfile's compression level).
