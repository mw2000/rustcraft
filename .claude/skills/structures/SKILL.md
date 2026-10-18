---
name: structures
description: Author or change rustcraft's structure templates (watchtowers, ruins, temples) in assets/structures/, in the game with /structure save or by script, and tune where they're placed. Use when adding a structure type, changing one's blocks or placement, or checking how often and where they appear.
---

# Structures

Each structure type is one TOML file in `assets/structures/`, built into the game with
`include_str!` in the `STRUCTURES` list in `src/chunks/generate/structures/mod.rs`, so add a new
file there too. Generation, placement and LODs are described in `docs/design.md` §5.6.

## The format

```toml
name = "watchtower"              # salts its grid, so keep it once worlds have it

[placement]
cell = 640                       # one may stand in each cell this many blocks square
chance = 0.6                     # the share of cells that try for one
biomes = ["forest", "foothills"] # by `Biome::name`
min_above_sea = 12               # its floor, in blocks over the sea
max_rise = 4                     # most the ground may rise and fall under its footprint
hilltop = true                   # climb from its spot to the top of the hill

[template]
size = [7, 20, 7]                # x, layers, z
floor = 1                        # the layer at ground level; those under it are sunk
foundation = "cobblestone"       # fills under it down to the ground
palette = { c = "cobblestone", s = "stone_bricks" }   # one character a block name
markers = { loot = [[3, 18, 3]] }                     # template x, y, z by kind
layers = [
  '''
ccccccc
...
''',                             # bottom first; rows run +z down, characters +x across
]
```

- `.` leaves the world as it is and `-` clears it to air, which carves interiors out of the
  ground. Neither can be in the palette.
- Inside the footprint, the ground above the floor is cleared down to it and the ground below
  filled up to it with `foundation`, so `.` there is air above the floor. Trees keep clear of the
  footprint. Plants under it are cleared with the ground.
- Keep the height over the floor within `STRUCTURE_HEIGHT` (27) and the sunk layers within
  `STRUCTURE_DEPTH` (4); loading refuses a template that doesn't fit, or whose layers don't match
  its size.
- A `loot` marker is a chest, which rolls the loot table named after the structure (in
  `src/loot.rs`) the first time it's opened, so add a table for a new structure with chests.
  Other marker kinds are kept for later milestones.

## Authoring

### In the game

Most structures are best built in the game, in creative, as an operator (the host always is):

1. Build it, or start from one there is: `/structure place <name> [0|90|180|270]` builds a
   template (the world's own of that name, or else the built-in one) on the block you're
   looking at and selects it. `/structure list` lists them.
2. Mark what's not the structure's own: a **structure void** (in the picker, E) is a cell the
   template leaves as the world has it (`.`), and a **loot marker** is a `loot` marker. Both
   show only in creative, as small blue and gold boxes. Air in the box is saved as `-`, which
   clears what's there.
3. Select the box with `/pos1` and `/pos2` at two opposite corners: the block outlined, where
   you stand, or `x y z`. Placing selects what it placed already. At most 64 by 31 by 64.
4. `/structure save <name> [floor]`, the floor being the layer from the box's bottom that stands
   at ground level (otherwise that of what you placed, or 0). It's written to
   `structures/<name>.toml` in the world's directory (the message says where), with the
   placement of the structure of that name if there is one, or else defaults to edit, and the
   foundation the commonest block of its bottom layer. Saving under a built-in name reworks it:
   `/structure place` takes the world's copy from then on.
5. Copy the file into `assets/structures/`, add it to `FILES`, and edit `[placement]`.

Placing and saving round-trip: the watchtower placed and saved again is the same file's
structure.

### By script

A shape that's easier to describe than to build, or one drawn from a seed, can be written by a
script here, so a change is a small diff. `watchtower.py` writes
`assets/structures/watchtower.toml` from a fixed seed: edit it and rerun.

```
python3 .claude/skills/structures/watchtower.py
```

## Checking

- `cargo test --release --lib structures` checks loading, writing, rotation, placement, terrain
  fit (and LODs, in `generator/tests.rs`) and the authoring commands.
- Changing a template or placement changes generation, so the golden checksum
  (`src/chunks/generate/checksum.rs`) changes when the structure nearest seed 3's spawn does:
  update it deliberately (CLAUDE.md).
- Density: `RegionStore::structures_near` over a square of the world counts them. The
  watchtower comes to about 3 per 10 km² round seed 3's spawn, fewer where forest and foothills
  are scarce.
- To see one in the game, find its position with `structures_near` and fly there with `/tp`.
