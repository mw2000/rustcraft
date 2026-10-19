---
name: structures
description: Author or change rustcraft's structure templates (watchtowers, ruins, temples) in assets/structures/, in the game with /structure save or by script, and tune where they're placed. Use when adding a structure type, changing one's blocks or placement, or checking how often and where they appear.
---

# Structures

Each structure type is one TOML file in `assets/structures/`, built into the game with
`include_str!` in the `FILES` list in `src/chunks/generate/structures/mod.rs`, so add a new
file there too, at the end: where two types would stand together the later one gives way, so a
type added earlier would move those after it. Generation, placement and LODs are described in
`docs/design.md` §5.6.

## The style guide

Every structure follows the style guide in `docs/design.md` §5.6, and is checked in screenshots
of the real game before it lands. A checklist:

- [ ] **Silhouette:** varied heights, an overhanging roof, a tower, chimney or crest; never a
  flat-topped box.
- [ ] **Depth:** recessed doors and windows, pillars, posts, beams, corbels, string courses and
  cornices standing out, walls thicker at the base (a plinth or skirt), steps up to the door.
- [ ] **Palette:** a main material, a trim and an accent suited to the biome: sandstone,
  terracotta bands and blue inlay; mossy cobble, stone brick and jade; stone, logs and planks.
- [ ] **Weathering and variety:** per-cell choices of mossy and cracked blocks, holes, vines and
  drifts; a whole-structure choice or two (a wall's planks, a band's colour); broken corners.
- [ ] **Inside:** furnished (beds, barrels, bookshelves, tables of a fence and a slab, furnaces)
  and lit with lanterns or torches, the chests where they make sense.
- [ ] **Screenshots** from the front, from above, inside and from 100+ blocks away, read and
  iterated on.

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
# depth = [24, 64]               # underground: its floor this far under the lowest ground

[template]
size = [7, 20, 7]                # x, layers, z
floor = 1                        # the layer at ground level; those under it are sunk
foundation = "cobblestone"       # fills under it down to the ground
palette = { c = "cobblestone", s = "stone_brick_stairs[facing=east,half=top]" }   # one character a block
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
- **Seeded variation:** a palette entry can be a choice of blocks, by weight (`*n`, 1 if left
  out). `|` chooses afresh for each cell, for weathering: `stone_bricks*6|mossy_stone_bricks*2|cracked_stone_bricks*2`,
  `vines[facing=north]|air*3` for vines here and there, `barrel[facing=up]|air` for a barrel some
  have. `/` chooses once for the whole structure, for a variant: `terracotta/red_terracotta`.
  Every whole choice draws the same number, so entries with the same weights in the same order
  choose alike (`dark_planks_fence/planks_fence` with `dark_planks/planks`). Each placed
  structure chooses from its own seed, the same at every level of detail; `/structure place`
  from the world's seed and where it's placed, so saving it writes the blocks it chose.
- Biomes are named as `Biome::name` has them, and `base` is the base terrain no biome owns.
- **Underground:** with `depth`, the structure's floor is that many blocks (a number hashed
  in the range) under the lowest ground over its footprint, its top at least 8 under it; the
  ground over it must be `min_above_sea` over the sea and rise and fall no more than `max_rise`.
  It's built into the rock as it is, with no foundation or clearing: `-` carves tunnels and `.`
  leaves the rock, so caves crossing it meet it. LODs leave it out, as they leave out caves.
- A palette entry is a block's name, with its state in brackets if it has one, as saves and
  `/structure save` write it: `log[axis=x]`, `stone_brick_stairs[facing=east,half=top]`,
  `door[facing=north,hinge=right]` (the lower block; write its upper one `half=top` above it),
  `lantern[hanging=true]`, `ladder[facing=south]` (it faces out of the wall it's on, south
  for one on a wall to its north), `barrel[facing=up]`. Properties left out take their first
  value (`facing=north`, `half=bottom`, `axis=y`, `open=false`, `hinge=left`), so a plain
  name is the block upright, facing north, shut. Each block takes only the properties its
  render style has (`RenderStyle::state_properties`); loading refuses others. Turning a
  structure turns `facing` and swaps `axis` x and z with it, so write it as it stands unturned
  (north up the page, -z). Fences, walls and panes join their neighbours by themselves, and
  have no state. The building blocks to use are in `src/block.rs`'s table: stairs, slabs and
  walls of planks, dark planks, cobblestone, stone bricks, bricks and sandstone; mossy
  cobblestone, mossy and cracked stone bricks, polished stone, terracotta in five colours,
  dark and light planks, glass and panes, doors, trapdoors, ladders, vines, lanterns,
  bookshelves and barrels.
- Inside the footprint, the ground above the floor is cleared down to it and the ground below
  filled up to it with `foundation`, so `.` there is air above the floor. Trees keep clear of the
  footprint. Plants under it are cleared with the ground.
- Keep the height over the floor within `STRUCTURE_HEIGHT` (27) and the sunk layers within
  `STRUCTURE_DEPTH` (6); loading refuses a template that doesn't fit, or whose layers don't match
  its size.
- A `loot` marker is a chest, which rolls the loot table named after the structure (in
  `src/loot.rs`) the first time it's opened, so add a table for a new structure with chests.
  Other marker kinds are kept for later milestones, such as the dungeon's `spawner` (M3.3).

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
script here, so a change is a small diff. Every built-in structure is: `watchtower.py`,
`desert_temple.py`, `jungle_temple.py`, `forest_ruin.py`, `glacier_outpost.py`, `mineshaft.py`
and `dungeon.py` each write their file in `assets/structures/`, and the `village_*.py` scripts
the village pieces (below); edit one and rerun it.

```
cd .claude/skills/structures && python3 desert_temple.py
```

They're written with `build.py`, a small library: a `Build` box of cells holding block text
(`AIR` clears, None leaves), `fill`, `walls`, `corners`, `hollow`, `gable` and `hip` roofs of
any stairs, `door`, `ladder`, `mark` for markers, `stairs(material, facing, top)` and
`slab(material, top)` for the names, and `write`, which gives out palette keys by name. Facing:
north is up the page (-z); a stair climbs towards its facing, so a roof's south slope faces
north; ladders and vines face out of the wall they're on.

For a quick look before running the game, `preview.py` sketches a template from its four sides
and cut away at the layers given, coloured by the atlas (Pillow and NumPy):

```
python3 .claude/skills/structures/preview.py assets/structures/desert_temple.toml /tmp/t.png 1 3,9
```

## Village pieces

Villages (M3.4) aren't placed as one template: they're assembled from **pieces** laid along
their streets, one TOML file each in `assets/structures/village/`, written by
`village_timber.py`, `village_adobe.py`, `village_lodge.py` and `village_common.py` (the farm
and pen the timber and lodge villages share) on the helpers in `village.py` (timber frames,
windows with shutters and flower boxes, steep and merged roofs, spires, chimneys, lamps on
brackets). A piece is a structure file with a `[piece]` table in place of `[placement]`, which
`Build.write_piece` writes (trimming the height to what's built):

```toml
name = "timber_cottage"

[piece]
role = "house"          # house, hall, smithy, farm, pen, tower, well or lamp
styles = ["timber"]     # the village styles that build it: timber, adobe, lodge

[template]              # as any structure's
```

The village layout codes against this contract, so every piece keeps it:

- **Front to the south** (+z, the last row of every layer); the layout turns each piece to face
  its street, which lies beyond that edge. The well may be turned any way.
- **Layer `floor - 1` is the ground's surface** (floors, plinths, path, `grass` in timber
  gardens, `path` and sand in adobe yards), so little raw ground shows where the footprint was
  cut or filled; layer `floor` is the first air over it. The lodges' yards, and the farm's and
  pen's round them, are left as the world has them (`.`), so they're snow in snowy taiga and
  grass elsewhere rather than green squares on the snow. Most pieces
  have `floor = 1`; the timber and lodge wells sink their shafts with `floor = 3`.
- **The last row is an apron**, walkable and level with the street at layer `floor - 1`, at
  least at the entrance, so the street's spur meets it.
- **Markers** by kind:
  - `entrance`: exactly one (none for a lamp), in the apron row at layer `floor`, solid under it
    and two of air over it, just outside the way in. From it a villager (one wide, two tall,
    stepping up a block at a time, down at most three, not climbing ladders, opening doors) can
    walk to every `bed` and `work` marker.
  - `bed`: houses, one a resident (small 1, medium 2, large 3): the floor cell beside a `bed`.
  - `work`: every other role, 1–3, where the worker stands: by the forge and anvil, among the
    wheat, behind the counter or by the tables, outside the pen's fence, at the tower's foot.
  - `gather`: the well only, 4–8 spots round it where villagers meet by day.
  - `animal`: the pen only, 2–4 cells inside it.
  - `loot`: chests, rolling the `village` table, on a solid floor with room over them, inside
    the border.
- **Doors** one wide and two high, `door[facing=...]` facing out of the room with its
  `half=top` over it; steps up to them a block at a time.
- **Light:** every room has a lantern, and every cell a monster could stand on at the ground
  is in reach of one (13 blocks), yards and backs included, as nothing spawns where block
  light reaches. Roofs are left dark.
- **Pens hold:** the pathfinder takes a fence as a block high, so a pen's barrier is two high
  (a kerb or wall with a fence on it), with nothing a block high beside it inside or out (a
  bale, a slab, a barrel), which would be a step up onto it.
- **Sizes**, x by z with the apron: lamps 1×1 to 3×1 and at most 6 tall, wells 7×7 to 9×9 with
  a walkable paved border all round (streets leave from the middle of each side), small houses
  about 7×8, medium 9×10, large 11×12, halls 13×13, smithies 10×10, farms 13×11, pens 9×9,
  towers 7×7 and 20–25 tall.
- **Variety:** per-cell choices for weathering (moss, cracks, patched adobe, snow on a roof,
  barrels and flowers or not) and whole choices for variants (thatch or dark plank roofs,
  plaster or light plank walls, the terracotta band's colour, a snowy ridge or none). Every
  whole choice draws the same number modulo its total, so give choices that should vary apart
  different totals (2, 3, 5): `thatch/dark_planks` and `plaster*2/light_planks` vary apart,
  where two choices of two would always agree. `preview.py` draws them as the game does.

To check a piece, preview it (it draws a piece with its ground layer and its markers as dots
in the cutaways: entrance red, bed blue, work orange, gather purple, animal pink) and walk its
markers: a checker that loads each file, checks names, states and markers, walks from the
entrance and lights the cells as the game does catches most mistakes before the game does.

## Checking

- `cargo test --release --lib structures` checks loading, writing, rotation, placement, terrain
  fit (and LODs, in `generator/tests.rs`) and the authoring commands.
- Changing a template or placement changes generation, so the golden checksum
  (`src/chunks/generate/checksum.rs`) changes when the structure nearest seed 3's spawn does:
  update it deliberately (CLAUDE.md).
- Density: `RegionStore::structures_near` over a square of the world counts them; §5.6 (Built
  M2.4b) has each type's. Aim for a few surface structures in a player's first hour (some
  15 km walked), and more underground, where caves find them. The far climates' structures
  are further out, as their biomes are.
- To see one in the game, find its position with `structures_near` and fly there with `/tp`, or
  write a world whose `world.toml` puts a creative player looking at it and run the game with
  `RC_WORLD` set to it (headless under Xvfb, `BEVY_ASSET_ROOT` set to the repository).
