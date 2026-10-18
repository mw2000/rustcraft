# rustcraft design

The source of truth for what rustcraft is becoming and how it's built. Agents and contributors
work from this document: read it before starting a piece of work, and update it in the same PR
when that work settles a decision, finishes a milestone or changes a plan.

How to read it:

- **Decided** items are settled. Don't re-argue them in a PR; propose a change to this doc instead.
- **Proposed** items are the current best plan. Following them needs no discussion; changing them
  means updating the doc and saying why.
- **Open** items need a decision before the work that depends on them starts. Ask, or bring a
  recommendation, rather than picking silently.
- IDs (`P1`, `INV-3`, `M0.4`, `D-2`) are stable. Refer to them in commits, PRs and code comments
  where it helps, and never reuse a retired one.

---

## 1. Vision

A survival and exploration voxel game, in the spirit of Minecraft but not a copy of it, built on a
renderer that draws the world out to the horizon. **Everything you can see is real, and you can
walk to it.**

You start with nothing, mine and craft your way up through tiers of tools and materials, and travel
across a huge, varied world to find the resources, structures and landmarks you need. Friends can
join your world. A creative mode lets you build freely.

It's an open-ended hobby project, and it should be easy for anyone to download and play.

### Pillars (decided)

- **P1 The horizon is the content.** The 16 km view, curved world and tall terrain aren't scenery:
  gameplay should give you reasons to look at the horizon and go there. Features that show up at
  distance (landmarks, structures, players' builds) are worth more than ones that don't.
- **P2 Survival you progress through by mining.** Ores come from digging, and better ores let you
  make better tools. Progression is the spine of the game.
- **P3 Exploration pays.** Resources and structures vary by region, so progressing means
  travelling. A journey of several kilometres should be normal and rewarding.
- **P4 Multiplayer.** Other players can join a world and play in it together. Singleplayer is the
  same game with one player.
- **P5 Creative mode.** Fly, build with any block, no damage. Also the tool for authoring
  structures.
- **P6 Easy to play.** Prebuilt downloads that just run, sensible defaults, and a small dedicated
  server for hosting.

### Non-goals (decided)

- A 1:1 Minecraft clone. Borrow what makes it fun (the loop, the feel, structures to discover), and
  make our own choices for the rest: our own blocks, recipes, progression and world.
- Modding APIs, competitive anti-cheat, or massive servers. Design for a handful of friends.
- Realism. Readable and fun beats simulated.

---

## 2. Where the code is now

What already exists, as of the `combined-prs` branch (main + PRs #74, #75, #76):

| Area | What's there | Where |
|---|---|---|
| World | 1024 blocks tall, sea level 448, 16³ chunks stored run-length encoded, seeded with `RC_SEED`; chunks held loaded by the client round the camera and by the server round each player | `src/world.rs`, `src/chunks/chunk.rs`, `src/chunks/generate/mod.rs`, `src/server/chunks.rs` |
| Terrain | Heightmap terrain from noise, sampled per 256-block region, or per coarse cell for far LODs, with caves carved out of it and ores in its stone (§5.4) | `src/chunks/generate/` (`noise.rs`, `region*.rs`, `columns.rs`, `caves.rs`, `ores.rs`) |
| Biomes | Climate-driven desert, savanna, jungle, forest, taiga, glacier, and foothills round the mountains; grass and leaves tinted by climate and biome (§5.7) | `src/chunks/generate/biome/` |
| Trees | Broadleaf, pine, jungle giants and acacias, by biome, placed on a deterministic 10-block grid, the same trees at every LOD level | `src/chunks/generate/trees.rs`, `generator/canopy.rs` |
| Plants | Tall grass, ferns and flowers on the grass, by climate and biome, drawn as crossed quads at full detail (§5.5a) | `src/chunks/generate/plants.rs`, `generator/mesh.rs` |
| Structures | Templates as data, placed on a grid of cells per type where the ground suits them, fitted to the ground, and drawn at every distance (§5.6); so far a ruined watchtower on hilltops | `src/chunks/generate/structures/`, `assets/structures/` |
| Rendering | Greedy meshing, an octree of LOD levels that cross-fade, out to 1024 chunks (2048 from high up); the land is bent around a round world | `src/chunks/lod/`, `generator/greedy.rs`, `src/chunks/fade.rs`, `src/curvature.rs`, `src/horizon.rs` |
| Light | Sky light (0–15) spread through each chunk, and block light from torches across chunk borders (§5.5) | `src/chunks/generate/light.rs` |
| Sky | A day's clock, 3D clouds, sun and moon, night sky, sun glare, water reflections, tonemapping | `src/clock.rs`, `src/clouds.rs`, `src/sky*.rs`, `src/tonemap.rs` |
| Player | Flying, or walking under gravity with block collision; swimming; survival (the default) and creative modes; breaking that takes time and the right tool in survival (§5.1) | `src/player/`, `src/collision.rs`, `src/game_mode.rs` |
| Menus | A title screen over the horizon of a world the terrain renderer draws; worlds to play, make, rename, duplicate and delete; saved servers and joining by address; settings in tabs (video, controls with every key rebindable, audio, accessibility), saved per player; a loading screen; the pause menu with Open to Friends and Save and Quit to Title; one set of shared widgets (M-U1) | `src/menu/`, `src/session.rs`, `src/preferences.rs`, `src/controls.rs`, `src/save/worlds.rs`, `src/client/servers.rs` |
| UI | Command bar with `/tp`, `/seed`, `/gamemode`, `/help` (T or /), hotbar (1–9, wheel, Q to drop), inventory screen in survival and block picker in creative (E), map of explored land (M, §5.7), debug overlay (F1) | `src/menu.rs`, `src/client/command_bar.rs`, `src/hotbar.rs`, `src/inventory/`, `src/picker.rs`, `src/map/`, `src/debug.rs` |
| Items | Every placeable block as an item, sticks, coal, ingots and tools that wear, stacks, a 36-slot inventory per player, blocks that drop items when broken in survival, dropped items to pick up (§5.2) | `src/items.rs`, `src/inventory/`, `src/server/drops.rs`, `src/client/drops.rs` |
| Crafting | Recipes from ingredient lists, by hand in the inventory screen or at a crafting table, and smelting in furnaces (§5.3) | `src/crafting.rs`, `src/inventory/recipes.rs`, `src/server/crafting.rs` |
| Containers | Blocks holding items of their own in the world, furnaces and chests, saved with the edits (§5.2) | `src/containers.rs`, `src/server/containers.rs`, `src/inventory/furnace.rs`, `src/inventory/chest.rs` |
| Loot | Loot tables, rolled from the seed and position into chests the world generates the first time they're opened (§5.6) | `src/loot.rs`, `src/server/containers.rs` |
| Farming | Hoes till farmland, seeds grow into wheat through eight stages on the server, catching up when their chunk loads, and wheat bakes into bread (§5.5a) | `src/farming.rs`, `src/server/farming.rs` |
| Settings | The renderer's defaults, and the player's own settings over them, volumes too, from the Settings screen | `assets/settings.toml`, `src/settings.rs`, `src/preferences.rs`, `src/client/sound/volume.rs` |
| Creatures | Kinds declared in a table, walking under gravity, finding their way by A* on the compute pool, spawning near players, hit, saved while nobody's near; so far sheep, which give mutton (§5.8) | `src/creatures/`, `src/server/creatures/`, `src/client/creatures.rs` |
| Sound | Footsteps by surface, breaking and placing, the player's own noises, others heard where they are, ambience by place and time, rare music, all synthesised (§5.10) | `src/client/sound/`, `assets/sounds/` |
| Tools | `bench`, `lod_sim` and `world_map` examples; `profile`, `block-textures`, `structures` and `sounds` skills | `examples/`, `.claude/skills/` |

Blocks are declared in the `blocks!` table in `src/block.rs`: the terrain's own (stone, grass, sand,
water, snow, logs, leaves, dirt, gravel, ice, sandstone), and ones for building and mining (planks,
cobblestone, stone bricks, bricks, coal, copper and iron ore, the far climates' gold, frost crystal
and jade ores and embers with blocks made from them, crafting table, furnace, chest), plants
(tall grass, poppy, dandelion, cornflower, fern), the torch and the bed, for farming farmland and
wheat's eight stages, and since M2.4a building blocks: stairs, slabs and walls cut from the
stones and planks, fences, doors, trapdoors, glass and panes, ladders, vines, lanterns, weathered
and coloured stone, two more woods of planks, bookshelves and barrels, with states (§4.1).

Blocks can be broken and placed, kept in an edit layer over the generated world (`src/edits.rs`,
`src/interaction.rs`), as requests the server checks (`src/protocol.rs`, `src/server/`,
`src/client/`). Worlds are saved as their edits, containers, crops and the player (`src/save/`).

Not there yet: hostile mobs and combat (M3.3).

---

## 3. Invariants

Rules that hold across the codebase. A change that breaks one needs this section updated first.

- **INV-1 The world is f(seed) + edits.** Everything generated (terrain, caves, ores, trees,
  structures) is a pure function of the seed and position, at any LOD resolution. The only state
  that isn't is the **edit layer** (§4.2): what players changed. Never store generated blocks;
  regenerate them.
- **INV-2 Generation is deterministic everywhere.** The same seed and game version produce the same
  blocks on every machine, since multiplayer clients generate terrain themselves (§4.4). No
  `HashMap` iteration order, thread timing or wall-clock time in generation, and care with
  floating point (§7, D-3).
- **INV-3 Generation is local.** Anything generated must be decidable from a bounded neighbourhood,
  the way trees use a grid of cells and a known reach (`TREE_REACH`). A chunk never waits on
  another chunk being generated first. Structures follow the same rule (§5.6).
- **INV-4 The server is the authority on game state.** Block edits, inventories, health, entities
  and time of day change on the server and are replicated. Clients predict for responsiveness but
  never decide. Singleplayer runs the same server in-process (§4.4).
- **INV-5 What's seen far away matches what's there.** A feature that appears in LODs appears the
  same at full detail, and the other way round, within what the LOD's resolution can show. This is
  why trees grow the same at every level, and it's what structures and edits must keep to.
- **INV-6 Content is data.** Blocks, items, recipes, loot tables and structures are each declared
  in one place, as a table or as data files the game loads, not as code scattered across systems.
  Adding a block shouldn't mean touching ten `match` statements.
- **INV-7 The frame budget is protected.** Gameplay systems don't block the main thread on
  generation or I/O. Measure performance-sensitive changes with the `profile` skill and say in the
  PR what they cost.
- **INV-8 Every asset is ours to ship.** Textures, models, animations and sounds are made for
  rustcraft (drawn or generated by our own scripts) or come from sources whose licence lets us
  redistribute them in an MIT/Apache project (CC0, or CC-BY with credit in `assets/CREDITS.md`).
  Minecraft and its resource packs are inspiration only: study how they look and sound, never
  copy, trace or adapt their files (D-10).

---

## 4. Architecture

### 4.1 Block registry

Every block is declared once, in the `blocks!` table in `src/block.rs` (INV-6, D-9).

- A row declares a `BlockType` variant with its properties: its name, whether it's solid,
  whether it's replaceable (a block placed into it takes its place: air, water, plants), its
  `light_opacity` and occlusion weight, the block light it `emits` (0–14, a torch's 14), its
  render style, and its tiles per face and for LODs. Properties are added to the row as the
  features that use them land.
  Since M1.1 a row says what the block `drops` when broken in survival: `Nothing`, `Itself`
  (its own block item), another block's item (§5.2) or, since M1.6, an item that isn't a block,
  as coal ore leaves coal. Since M1.2 it has a `hardness` (the
  seconds it takes to break by hand), the `tool` that breaks it quicker, if any, and the `tier`
  of that tool it needs to drop anything (§5.1, §5.3).
- Render styles (`RenderStyle`): `None` (air), `Cube` (faces, culled against opaque neighbours
  and greedy-merged), `Liquid` (water's own mesh), `Cross` (two diagonal quads, double-sided
  and alpha cut out, at full detail only) and `Torch` (a stick 2/16 wide and 10/16 tall on the
  floor of its cell, showing the middle of its tile, its flame drawn unshaded, at full detail
  only). A `Cross` block is a plant: not solid, lets all light through, hides no faces, and
  stands on the block below. A torch stands on the block below the same way
  (`BlockType::needs_ground`), but isn't built over. Torches stand on floors only; on walls
  they need blocks to face a way.
- `BlockType` stays a one-byte enum, stored in `ChunkData`'s runs, so generation and meshing can
  match on it and its properties are plain array lookups.
- Each block has a stable snake_case name such as `"sandstone"` (`BlockType::name` and
  `from_name`). Saves and the network protocol store names, or a palette mapped to names, never
  discriminants, so reordering the table can't corrupt a world.
- Textures go into the atlas with the `block-textures` skill: generated with ComfyUI, or drawn by
  its `draw.py` from the existing tiles' shades where they need hard edges (bricks, ores, crafted
  blocks) or ComfyUI isn't set up.

- **Built (M2.4a): block state and shapes** (`src/block/state.rs`, `src/block/shape.rs`,
  `src/building.rs`, `generator/shapes.rs`).
  - **State:** a `BlockState` is a byte beside a block's type: `facing` (north, east, south,
    west, and for some up and down; north is -z), `axis` (y, x, z), `half` (bottom or top, a
    door's lower or upper block; `hanging` for lanterns shares its bit), `open` and `hinge`,
    each in bits of its own, every property's first value 0. Which properties a block has
    comes from its render style (`RenderStyle::state_properties`), so the table declares it
    with the style. A `Block` is a type and a state: what's in a cell, edited, saved and sent.
    Its text, which templates, saves and messages use, is the name with the properties not at
    their first value, `stone_brick_stairs[facing=east,half=top]`, so a block with no state is
    its name and old saves and templates read as they were. Fences, walls and panes join their
    neighbours as they're drawn, from the blocks beside them, so store nothing.
  - **In `ChunkData`'s runs** the state takes the byte the block's run left spare, so a run is
    still 4 bytes; runs merge only where block and state both match. Everything generated has
    state 0 (logs stand upright), so terrain's runs are as they were. Structures' blocks with a
    state are put into a chunk once its runs are made, so terrain's cells stay a `BlockType`
    each. Meshing reads states only for the blocks with any, from a map the neighbourhood
    builds from the runs with one, empty for terrain.
  - **Render styles added:** `Pillar` (logs: ends along their axis, bark turned to run along
    it), `Fronted(tile)` (the furnace: a front facing the player), `Lidded` (the barrel: its lid
    any way), and the shapes: `Stairs`, `Slab`, `Fence`, `Wall`, `Pane`, `Door`, `Trapdoor`,
    `Ladder`, `Vine` and `Lantern`. Glass is a `Cube` that lets light through, hides nothing
    behind it, and draws no faces between its blocks, as ice. A shape is a few boxes in its
    cell (`shape::boxes`), for drawing, collision (fences and walls 1.5 tall to bodies, vines
    none) and the outline. Its faces show the part of their tile they cover, and are culled
    where the neighbour fills its whole face (`shape::fills_face`): opaque cubes, a slab's
    bottom or top, stairs' back and slab side; so a slab hides the face it sits on but not the
    one over its empty half, and cubes' faces are culled against stairs' backs too. Ladders and
    vines are a quad a sixteenth off their wall; lanterns map their boxes to their tile's parts,
    hang on a chain, and their glass glows unshaded as a torch's flame does. Vines are tinted
    as leaves. Shapes cut their tiles' holes out of their shadows (`generator::CUTOUT`), and
    glass by its atlas cell, as leaves do.
  - **Light:** stairs and slabs keep the sky out, so a roof of them roofs, but their cells
    would be dark for the faces lit from them, so they take the light of the brightest cell
    beside them, less one (`SkyLight::light_shapes`), only where a neighbourhood has any. The
    rest of the shapes, glass and panes let light through. Shapes' faces are lit flat, by the
    cell they face, or their own where that keeps light out, with no ambient occlusion.
    `BlockType::is_opaque` (a cube light can't get through, which hides faces) and
    `blocks_light` (roofs) are now apart.
  - **LODs** draw stairs, slabs and walls as cubes of their tiles, counting them as built in
    `LodEdits`, and leave out fences, panes, doors, trapdoors, ladders, vines and lanterns, as
    plants and torches, as too thin to make out (INV-5).
  - **Cost** (INV-7): meshing what flags each block needs (opaque, a cube near or far, has a
    state, a shape) is a table worked out once from the rows (`MESHING`). The bench's vertex
    counts are identical; chunk meshing is about 2% quicker than before (the opaque table), and
    LOD meshing runs the same instructions, within 0.1%. A pathological scene of a shape every two blocks on every
    floor (`bench 1 shapes`) meshes in about 1.0 s against 0.44 s, with 4.8 times the vertices.
  - Later: stairs' corners joining, double slabs, doors' hinges picked from their
    neighbours, chests facing a way, wall torches, and culling faces between shapes.

New blocks are added as features need them, and that's expected (§5).

### 4.2 Edit layer (proposed)

The world's state beyond the seed (INV-1).

- Edits are kept per chunk, as a sparse map from block position to `Block` (its type and, since
  M2.4a, its state, §4.1): what a player put there, air included. A chunk's blocks are its generated blocks with its edits applied on top.
- An edit marks its chunk, and any neighbour it borders, for remeshing and relighting.
- **Built (M0.2):** `Edits` (`src/edits.rs`) is held by `World`, which puts a chunk's edits back
  as it's loaded (`insert_chunk`). The world only changes through an `EditBlock` message, made by
  `apply_edits`, so the server can take that over (INV-4). An edit changes its loaded chunk at
  once, for collision, then rebuilds the group meshes of every chunk whose mesh reads the block
  (`chunks_meshed_with`): those whose sky light takes it in, which reaches 8 blocks sideways and a
  chunk down, and covers faces, ambient occlusion and shores across borders. Light is worked out
  per mesh from the chunk and the one above, so a shaft or a roof changes light that far down.
  An edit that changes block light (an emitter placed or broken, or a block letting light
  through differently within an emitter's reach) also rebuilds every chunk that light can reach,
  13 blocks round it (`World::take_relit`, §5.5); other edits rebuild no more than before.
  Rebuilds mesh from the world's blocks on the compute pool, ahead of loading work, and swap the
  new mesh onto the same entity with no fade (`group::rebuild_groups`, `load_groups`). Since
  M0.5b the server makes edits and the client queues the rebuilds (§4.4).
- Edits are what's saved (§4.3) and what's sent over the network (§4.4). Generated blocks are
  neither.
- Chunks with heavy edits (a big build) can switch to storing their whole contents rather than a
  sparse map.
- **Edits in LODs:** to keep INV-5 for big builds, LOD nodes take the edits they cover into
  account, so a tower shows up from far away.
- **Built (M2.8):** `LodEdits` (`src/chunks/lod/edits.rs`), held by `World` beside `Edits`.
  - **What a cell draws:** for each LOD level from 1 (2-block cells) to `EDITED_LEVELS` (8,
    256-block cells), the cells with edits in them, each a `CellEdits`: how many of its blocks
    are built of something LODs draw (cubes and water), the commonest of those, and how many
    are cleared, to air or to what only full detail draws, such as torches and plants. Level 1's
    are counted from the blocks, and each coarser cell is the sum of the 8 under it, its block
    the one most of their built blocks are (the first if two tie, so it's the same wherever
    it's worked out). A cell draws as structures do (§5.6): its commonest block where at least
    half of it is built, air where at least half is built or cleared, and otherwise what
    generated. So a brick tower is a column of cells and a pit a hole at every level whose
    cells it fills half of, and level 1 is exactly that majority of full detail's blocks.
    Coarser levels would need half a cell 512 wide, 67 million blocks, to show anything.
  - **By node:** a level's cells are its LOD nodes' cells, so they're kept by node, in a map
    from where each is in the node, including the ring of cells round it a mesh looks at (a
    cell on a node's edge is in its neighbours' too). A node finds what it draws with one
    lookup, as an `Arc` its mesh task takes, copied on write only if an edit lands while one's
    being built. `LodBlocks::sample` puts the drawn cells over what generated, before surface
    tops, skirts and light are worked out. A node is no longer `known_empty` if any of its
    cells, or its ring's, draw edits, so a tower in the sky or a hollow in the rock is meshed.
  - **Redrawing:** `World::edit` brings the 2-block cell over the block up to date and walks up
    the levels, stopping at the first cell that doesn't change. Where a cell's drawn block
    changes, its nodes are noted (`take_redrawn`, recorded only once something asks, so a
    dedicated server keeps none); a block that changes no cell's look, like a single block or
    one more on a big wall, redraws nothing. `rebuild_lods` (before `spawn_lods`) meshes each
    such node again on the async compute pool, swapping the new mesh onto the same entity with
    the same materials (`load_rebuilt_lods`), so it neither flickers nor fades; a node that had
    nothing to draw gets an entity, and one left with nothing is despawned. A node still being
    meshed waits until it's done, so edits in quick succession are taken in together; a node
    not drawn yet just takes in the edits when it's spawned. Edits wholly within full detail
    never reach a node drawn, as only levels 1 and up are LODs.
  - **Loading:** `LodEdits::of` works out a saved world's at once, bottom up (3 ms for 50,000
    edits). A client joined to a server makes its `World::joined`, whose LOD edits are sent by
    the server (§4.4, M2.8) rather than worked out from the few edits it has.
  - **Cost** (INV-7): with no edits nearby a node pays a lookup in an empty or small map;
    unedited LOD meshes are unchanged (bench vertex counts identical, times within noise). An
    edit costs about 1.3 µs more than before (50,000 into the world in 62 ms rather than
    2 ms), so a structure placed in one go still costs a frame or so. Meshing a node with a
    50,000-block tower in it costs about 35 µs more at level 1 and nothing measurable coarser.
    `lod_sim`, flying with a 50,000-block tower going up 1.5 km off at 400 blocks a frame,
    shows no holes and frames no slower than without.
  - **Known gaps:** edits don't take away what coarse nodes draw as boxes, so a structure
    demolished or a forest felled still stands in cells 4 or more wide. A build narrower than
    half a cell vanishes at that level: a 5-wide tower is gone beyond about 1 km. Boxes for
    builds, as structures have, would keep thin ones in sight further.

### 4.3 Saving (proposed)

- A world is a directory: `world.toml` (seed, game version, name, game mode, time of day),
  `players/` (position, inventory, health per player), and `edits/` holding region files of edits.
  Edit region files group 32×32 columns of chunks so a world isn't thousands of tiny files.
- Saves are small, because only edits are stored.
- Save on a timer and on quit, from a background task (INV-7).
- Saves carry a format version and a game version, and old saves keep loading or are migrated
  explicitly.
- **Built (M0.3):** `src/save/`. Worlds live under the platform's data directory
  (`~/.local/share/rustcraft/worlds/<name>/` on Linux, from the `dirs` crate), picked from
  the Singleplayer menu since M-U1, or with `RC_WORLD` (default `world`) for development. A missing world is made with `RC_SEED` or a random seed, and its
  `world.toml` written at once; an existing one keeps its seed, and a disagreeing `RC_SEED` is
  ignored with a log line.
  - `world.toml` holds the format version (`FORMAT_VERSION`, now 4), the game version that wrote
    it, the seed, the time of day, and a `[player]` table with the one local player's position,
    look (yaw and pitch), movement, game mode, hotbar (the selected slot) and inventory (each
    stack's slot, item name and count, and the stack on the cursor if the inventory screen was
    open). Saves without them get a new player's: format 1's hotbar blocks are ignored, and its
    players start with an empty inventory in survival and the creative kit in creative. Players
    move out to `players/` when there's more than one (M0.6). A world from a newer format is
    refused with an error saying so. Dropped items aren't saved (§5.2).
  - `edits/r.<x>.<z>.edits` holds a region's edits in postcard (D-6): a palette of block names,
    then each edited chunk's coordinates and its edits as a block index and a palette index.
    A palette name the game doesn't know, from a newer version, fails the load rather than
    turning into air. Nothing generated is written (INV-1), so a world never edited is just its
    `world.toml`.
  - **Since format 3 (M1.6)** a region file starts with a 0 byte and its format, 3, and each
    chunk has its containers after its edits: each as its block index and the container, its
    block's name, its slots (by item name, with wear) and its fire's three floats (§5.2). A
    format 2 file starts with its palette's length instead, never 0 as every file has an edit
    to name, so both are read, and a format 2 region's written as format 3 the next time it
    changes; nothing's rewritten until then. A newer region format fails the load. Containers
    changing mark their chunk for saving as edits do, so a furnace burning has its region
    written every 30 s while it does. `world.toml`'s format is 3 so an older game refuses the
    world rather than misreading its region files; it reads format 2's `world.toml` as it was.
  - **Since format 4 (M1.9b)** each chunk has its crops after its containers: each as its block
    index and when it was planted, in game days, an `f32` (§5.5a). A format 3 file (`0, 3`) is
    read with no crops, and written as format 4 the next time its region changes; format 2 still
    loads as before. Crops changing (planted, broken, ripened and let go of) mark their chunk for
    saving as edits do; a crop growing a stage is an edit. `world.toml`'s format is 4, for the
    same reason as 3, and format 2's and 3's are read as they were.
  - **Since format 5 (M2.4a)** a region's palette names each block with its state, in the text
    templates use (`stone_brick_stairs[facing=east]`, §4.1), each block in each state once. A
    block with no state is its name, so format 4 is laid out the same and read as format 5, and
    rewritten as it the next time its region changes; a state the game doesn't know fails the
    load. `world.toml`'s format is 5 so an older game refuses the world.
  - `Edits` remembers which chunks changed since the last save. Every 30 s, and only if
    something changed, the main thread copies `world.toml`'s contents and the edits of the
    regions with new ones, which is quick, and the IO pool writes them, one
    save at a time. Only those regions' files are rewritten. Each file is written beside its
    old one, synced and renamed over it, so a crash mid-save leaves the old file whole. A failed
    save logs an error and is tried again next time.
  - On quit, `save_on_exit` (in `OnAppExitSystems`, after the `AppExit` message) waits for any
    save in flight and writes the last one on the main thread, so it's done before the app
    exits. A killed process loses up to 30 s of play.
  - At startup every region file is loaded into `Edits` before chunks generate, which is fine
    while worlds are small. A big world would load regions lazily, as chunks near them load.
  - **Since M3.1** creatures are saved by region in `creatures/r.<x>.<z>.creatures`, apart from
    the edits, as they move and are written far more often: a format byte (1), then postcard of
    each creature's kind by name, the position of its feet and its health (§5.8). Each autosave
    gathers those near players and those asleep, and rewrites only the regions whose creatures
    aren't as they were last written, removing a region's file once it has none. A kind the game
    doesn't know fails the load, as items do. Older games ignore the directory, so the format
    isn't raised; one that opens the world finds no creatures, and leaves the files as they are.

### 4.4 Multiplayer (decided: client/server with an integrated server, on `bevy_replicon`, D-1)

- **Integrated server.** Singleplayer runs a server in the same process; multiplayer runs the same
  server either hosted by a player or as a dedicated, headless binary. One code path.
- **Terrain is never sent.** A client joining gets the seed, the game version, and the edits near
  it, and generates everything else itself (INV-1, INV-2). This is what makes a 16 km view possible
  over a network.
- **A determinism handshake.** On join, the client and server compare checksums of a few generated
  sample chunks. A mismatch refuses the join with a clear error, rather than letting players see
  different worlds (D-3).
- **Authority.** The server owns edits, inventories, health, entities and time (INV-4). Movement
  is decided by the client and checked by the server for plausibility (speed, passing through
  blocks), which is enough for friends playing together.
- **What's replicated.** Edits near each player, players (position, look, held item), entities
  near each player, time of day, and chat and command output.
- **Code layout.** Split into what runs on the server (simulation, edits, saving), what runs on the
  client (rendering, LODs, input, UI), and what's shared (blocks, generation, protocol). Gameplay
  systems are written to run on the server and must not depend on rendering.

#### The plan for M0.5 and M0.6

- **One App, replicon-style.** Three plugin sets: `SharedPlugin`, `ServerPlugin`, `ClientPlugin`.
  Systems that need authority run under an `is_authority()` condition (replicon's
  `ClientState::Disconnected`), which holds in singleplayer, for a listen server and for a
  dedicated server. In singleplayer no transport runs and replicon hands client messages straight
  to the server systems (`ClientId::Server`). A dedicated server is `SharedPlugin` + `ServerPlugin`
  on `MinimalPlugins`. Not a second App or SubApp: that would double `World`, need edit prediction
  from day one and complicate quitting and saving. The split is kept honest by tests instead: a
  headless `ServerPlugin` App in M0.5, and two Apps over renet2's memory transport in M0.6.
- **Server:** `World` authority (validates and applies edits; keeps chunks near each player
  generated for collision and validation, never meshed), `Edits` and saving, the clock, players
  (spawned from saves, game modes), commands, inventories and dropped items (since M1.1, §5.2),
  and later movement plausibility. The hotbar's selection is the client's, as it only says what's
  asked for.
- **Client:** meshing, LODs, fade, sky, clouds and every other renderer, the camera, input and UI,
  moving its own player (decided client-side, §4.4), interpolating other players (M0.6), and its
  own copy of `World` (the seed plus edits it's sent) for collision, raycasts and meshing.
- **Shared:** blocks, generation, `World`, `Edits`, collision, the movement step as a pure
  function, `GameMode`, `raycast` and `overlaps`, and the protocol in `src/protocol.rs`.
- **Protocol.** Blocks travel by name, or by a palette sent on joining (§4.1). Client to server:
  `RequestEdit { block, to, slot, seq }`, `RunCommand`, `Chat`, `EditReach` (M0.6c, M2.8), the inventory's requests
  (`InventoryClick`, `CloseInventory`, `DropItem`, `PickBlock`, §5.2), `UseBlock` and `Craft`
  (§5.3), `StartBreaking` (§5.1), and from M0.6
  `PlayerPose` (unreliable, ~20 Hz). Server to client: `Welcome { game_version, protocol, seed, palette, samples, player,
  time_of_day }`, `ChunkEdits` (edits of chunks near the player, on joining and as they move), `EditSummary` (LOD edits of those in view, M2.8),
  `BlockChanged { block, to, by }`, `EditRejected { seq, block, actual }`, `TimeSync`, `Teleport`,
  `CommandOutput`, `ChatLine`, and since M1.6 `ContainerChanged` (§5.2). Players are replicated entities, and so are dropped items; a
  player's `Inventory` goes to their owner alone (as `InventoryChanged` since M0.6a); poses go out as unreliable messages
  to everyone but their owner, so the server never overwrites a player's own movement.
- **Edits** become request, validate, broadcast: `use_target` sends `RequestEdit`; the server
  checks the block's in the world and loaded, within reach of the requester's eye, not placed into
  any player or into a solid block, allowed by their game mode, and not too fast; then applies it
  and broadcasts `BlockChanged`, or answers `EditRejected`. The client applies `BlockChanged` and
  **always** remeshes: in one App the client shares the server's `World`, so `World::edit` reports
  no change there. Client-side prediction with rollback comes in M0.6.
- **Chunk residency** is shared: `World` tracks who holds each chunk (client or server) and a chunk
  is only cleared once nobody does, so the client unloading behind the camera can't drop a chunk
  the server keeps for another player.
- **Determinism handshake** (D-3): a stable hash (not `DefaultHasher`) of `generate_chunk`'s output
  for a few fixed sample chunks near and far from spawn, sent in `Welcome` and checked by the
  client, which refuses to join on a mismatch.
- **Built (M0.5a):** the player's look is a `PlayerLook { yaw, pitch }` on the player, which
  movement, `target_block` and saving read; the camera, a child of the player at `EYE_HEIGHT`,
  only follows it (`follow_look`), and the player's own `Transform` has no rotation. The movement
  step is `player::physics::step`, a pure function of a `MoveInput`, the look, the `Player` and a
  block lookup; `player::input` turns keys and the mouse into those, and `player::underwater`
  holds the murk and underwater effects. The clock is `src/clock.rs` (`ClockPlugin`, and
  `wind_clock` for `[` and `]` on the client), apart from `move_sky`, which only places the sun
  and moon. `tests/headless.rs` runs saving, game modes and the clock on `MinimalPlugins`.
- **Built (M0.5b):** `bevy_replicon =0.45.2` with only its `client` and `server` features, so
  no transport and no `bevy_world_serialization`; it adds five small crates (replicon,
  `postcard-derive`, `deterministic-hash`, `smallbitvec`, `xxhash-rust`) and no Bevy features.
  The game adds `RepliconPlugins`, `ProtocolPlugin` (`src/protocol.rs`, which registers every
  message in one order), `ServerPlugin` (`src/server/`) and `ClientPlugin` (`src/client/`).
  With no transport `ClientState` stays `Disconnected` and `ServerState` `Stopped`, so
  `is_authority` holds and replicon passes messages across in `PostUpdate`: what's written as
  `RequestEdit` comes out as `FromClient<RequestEdit>` from `ClientId::Server` in
  `ClientSystems::Send`, and `ToClients<BlockChanged>` comes out as `BlockChanged` in
  `ServerSystems::Send`. Headless Apps also need `StatesPlugin`, and tests `app.finish()`, where
  replicon sets up its message systems. Blocks travel by name (`BlockType`'s serde), positions
  as `I64Vec3`, and `by`'s `ClientId` as its entity's bits, or none for the server.
  - `use_target` sends `RequestEdit { block, to, seq }`, `seq` counting up per request.
    `server::edits::receive_edits` runs between those two sets, so the request, the check, the
    change to `World` and the start of the group rebuild all happen in the click's frame (one
    `app.update()`, pinned by a test in `interaction`), as soon as in M0.2: the client runs
    `rebuild_groups` again in `PostUpdate`, straight after queueing, as well as in `Update`
    for sealed chunks the camera nears.
  - The server refuses an edit, answering `EditRejected { seq, block, actual }` to the requester
    alone, if the client is over its allowance (a burst of 20 refilled at 20 a second, above the
    ~15 clicks a second anyone manages, and counting refused requests), the block is beyond the
    world's layers or in a chunk not loaded, the block's nearest point is further than `REACH`
    plus 1 from the requester's eye, a place is into a solid block, of a block the player's
    `GameMode::can_place` refuses (air and water), in survival of a block not held in the
    hotbar slot the request names (since M1.1), or into any
    player's body (`overlaps`), or the block's already what's asked for. Otherwise it calls
    `World::edit` (which records the edit for saving, as before) and sends
    `BlockChanged { block, to, by: Some((client, seq)) }` to every client.
  - The requester's player is the one player until M0.5c gives players their `ClientId`
    (`player_of`). `EditBlock` and `apply_edits` are gone: `client::edits::apply_block_changes`
    calls `World::edit`, a no-op in one App, and always queues the rebuilds
    (`ChunkLoader::rebuild_edited`). Nothing reads `EditRejected` until prediction (M0.6).
  - Deviations: `SharedPlugin` is for now just `ProtocolPlugin`, as nothing else shared needs
    a plugin yet. Both sides run in `PostUpdate` rather than after replicon's `Receive` sets in
    `PreUpdate`, which would have cost singleplayer two frames; over a network messages arrive
    in `PreUpdate`, so the same systems still see them that frame.
- **Built (M0.5c):** `ServerPlugin`'s startup (`open_world`) opens the world and its save from
  a `WorldToOpen { dir, seed, time_of_day, region_distance }`, which `main` fills from
  `RC_WORLD`, `RC_SEED`, `RC_TIME` and the renderer's settings (now a resource, read once),
  and spawns the host's player from the saved `PlayerState` with a `PlayerOwner(ClientId)`,
  `ClientId::Server` for the host's. An App with no `WorldToOpen`, such as a test that builds
  its own `World`, opens nothing. The client's observer `mark_local_player` marks the host's
  player `LocalPlayer`, and `attach_camera` (in `PostStartup`, before transforms propagate, so
  nothing sees the camera loose) hangs the scene's camera from its eyes. Client systems
  (movement, look, targeting, the hotbar, the picker, the debug overlay, and commands until
  M0.5d) find their player by `LocalPlayer`; the server's `player_of` finds a requester's by
  `PlayerOwner`, and saving writes the host's. In M0.6 a joining client must be told which
  player is its own and attach the camera when it arrives, and a dedicated server spawns no
  host's player.
- **Built (M0.5d):** Enter in the command bar sends `RunCommand(line)`; the server's
  `server::commands::run_commands` (beside `receive_edits`, so in the frame it's typed) parses
  it with the shared `commands::parse`, from the sender's player's position, runs it against
  that player and answers `CommandOutput { text, error }` to the sender alone, which the bar
  prints as replicon hands it over. `/tp` moves the sender's body on the server, which in one
  App is the local player's own; a remote player decides their own movement, so from M0.6 they
  need a `Teleport`. `/gamemode` sets the sender's `GameMode` (open to anyone until operators,
  M0.6e) and `GameModePlugin` lands them as before; `/seed` reads the server's `World`.
  `CommandPlugin` now needs `ProtocolPlugin`.
- **Built (M0.5e):** the server's clock (`src/server/clock.rs`) runs only on the authority:
  `run_clock` moves `TimeOfDay` on at a `ClockRate`, and `sync_clock` sends every client
  `TimeSync { time, rate }` every 5 seconds and as soon as the rate changes (to the App itself
  in singleplayer, where nothing reads it). A client that isn't the authority sets its clock
  by each sync and runs it at the rate given in between (`client::clock::follow_clock`), and
  stands still until the first. `[` and `]` stay as they were, rather than becoming a `/time`
  command, so the sky can still be wound through dusk: the client sends `WindClock(-1 | 0 | 1)`
  as they're pressed and let go, and the server sets the rate, the last to ask having their
  way, and syncs at once, so everyone's sky winds together. `ClockPlugin` is gone; the server
  and client plugins each start `TimeOfDay` at `START_TIME` until a world's opened, and saving
  keeps the server's time as before. Over a network a client's clock jumps at each sync by
  however far its frames have drifted from the server's, a fraction of a second's worth, or a
  moment's while it's wound; neither is worth smoothing yet. A client that leaves while
  winding should unwind the clock (M0.6).
- **Built (M0.5f):** `World` keeps who holds each chunk, a bit for each `Holder` (`Client`,
  `Server`): `hold` as a chunk's asked for, before it's generated, and `release`, which unloads
  it once nothing holds it, in place of `clear_chunk`. The client holds a chunk as
  `gather_chunks` takes it and lets go in `unload_chunks`; a chunk the server has loaded already
  is taken as it is, with no generation. The server's `keep_chunks_near_players`
  (`src/server/chunks.rs`, in `PostUpdate` before edits are checked) holds every chunk within 2
  of a player's eye chunk, generating on the async compute pool (INV-7) those that aren't loaded
  or on their way, and never meshing; it lets go of them past 3, cancelling any still
  generating. Everything in reach of the eye (6 blocks with the slack) and the whole body is in
  the eye's chunk or one against it, at most √3 off, so 2 leaves a chunk's margin to move into,
  and letting go only past 3 keeps walking to and fro over a border from generating the same
  chunks again. It runs after the client's loader, which in singleplayer has taken those chunks
  already, so it generates nothing there (a flight in `lod_sim`, which now runs it, generated
  none). `World::reroofed` records no chunk the server alone holds, as nothing draws it, so a
  dedicated server, where no client holds anything, records nothing and needn't drain it.
  The server draining it instead would, in one App, take it from the client.
  `tests/headless.rs` checks a server on its own loads the chunks round its player and none far
  off, lets go of them as it leaves, checks edits against them, and records no reroofs.
- **Built (M0.6a):** `bevy_replicon_renet2 =1.0.0` (with `bevy_renet2`, `renet2`,
  `renet2_netcode` and `renetcode2`, all 1.0.0) and its `native_transport`, netcode over UDP,
  plus `memory_transport` for tests only. `RepliconRenetPlugins` runs beside `RepliconPlugins`;
  a server is started by inserting a `RenetServer` and `NetcodeServerTransport`, a client by a
  `RenetClient` and `NetcodeClientTransport` (`server::serve`, `client::join::connect`), and
  in `PreUpdate` the plugins move `ServerState` to `Running` and `ClientState` through
  `Connecting` to `Connected`, and back to `Disconnected` when the connection goes.
  - **Hosting:** `RC_HOST=<port>` (or empty for the default, 25600, `protocol::DEFAULT_PORT`),
    or Open to Friends in the pause menu, which shows the address to give, binds UDP on every
    interface (`server::host`) for up to 16 players. Singleplayer goes on as before: the host's
    client is still `Disconnected`, so it's the authority and replicon still passes its
    messages across locally. **Joining:** `RC_JOIN=<host[:port]>` adds no `ServerPlugin` or
    `SavePlugin`, so a client opens no world and saves nothing. The renderer's systems, the
    interaction systems and movement run once there's a `World` (`resource_exists`), and
    `gather_chunks` once the camera hangs from the player, so nothing runs ahead of the welcome.
  - **Versions:** `ProtocolPlugin` adds the game's version and `PROTOCOL_VERSION` to replicon's
    protocol hash, which a client sends as it connects; the server refuses one whose hash
    differs with a `ProtocolMismatch` and disconnects it, before anything else is sent, and the
    client says so in the command bar and the log (`client::join::refused`). So `Welcome`
    carries neither version. Failing to connect or losing the connection is said the same way.
  - **Joining a world:** once a client is authorised (`AuthorizedClient` added), the server's
    `players::welcome` spawns its player at the world's spawn with `PlayerOwner(client)` (the
    same `player::arriving` bundle as the host's) and sends it `Welcome { seed, player }`, the
    `PlayerState` it spawned. `sync_clock` syncs everyone as anyone joins, so the time comes
    with it as a `TimeSync`, rate and all. The client's `join::arrive` (in `PostUpdate`, before
    transforms propagate) builds its own `World` from the seed with no edits, spawns its
    `LocalPlayer` from the state, and hangs the camera from it; the loaders then generate
    terrain around it as in singleplayer (since M0.6b, once it's checked the world). When a
    client disconnects, `players::see_off` despawns its player and forgets its
    `EditAllowances`.
  - **Playing:** a client sends `PlayerPose { position, yaw, pitch }`, unreliable, every 50 ms
    while connected, and the server's `move_players` puts its player there before keeping
    chunks near players and checking edits, so reach and residency go by where it is. Edits
    are `RequestEdit` and `BlockChanged` as before; a client applies every `BlockChanged`,
    its own and others', to its own `World`, which records it in its edit layer if the chunk
    isn't loaded yet, so the chunk generates with it.
  - **Inventories and drops:** the server sends a client's player's `Inventory` to that client
    alone as `InventoryChanged` whenever it changes it (`players::send_inventories`), and the
    client puts it on its `LocalPlayer`; the host's is the server's own, so it's never sent.
    An `Inventory` is written slot by slot to postcard and as its saved form to TOML
    (`is_human_readable`), as postcard can't write the saved form's flattened stacks. Dropped
    items are spawned `Replicated`, the first thing replicated: `DroppedItem` with only its
    stack, and its `Transform` as only its translation (`Transform` has no serde without
    Bevy's `serialize` feature), to every client, which draws them with `client::drops` as
    the host does. Falling bypasses change detection on `DroppedItem` and leaves a resting
    item's `Transform` alone, so only moving items and changed stacks are sent again.
  - **Known gaps:** netcode runs unsecure: clients aren't authenticated (they pick their own
    random id) and nothing's encrypted, fine among friends until connect tokens are handed out.
    Not synced yet (edits came with M0.6c, other players, `/tp`, `/gamemode`, saving and
    plausibility with M0.6d): a client leaving while winding leaves the clock wound.
  - `tests/multiplayer.rs` runs a listen server and a client as two Apps over the memory
    transport: the client is authorised and welcomed with the server's seed and its player,
    its edit is made by the server and comes back as `BlockChanged`, the host's edit reaches
    it, its inventory follows the server's, the item a block it breaks in survival drops
    reaches it, leaving despawns its player, and a client of another version is refused and
    told so.
- **Built (M0.6b):** the determinism handshake (D-3). `world_checksum(seed)`
  (`src/chunks/generate/checksum.rs`) generates 24 sample chunks with `generate_chunk` from a
  `RegionStore` of its own: in the spawn's column and three thousands of blocks out (one far
  north, cold and wooded), the chunks over, at and two under the ground and layers 1, 6 and
  20, so plants, trees, water, caves and all three ores are in them (a test checks). It's
  64-bit FNV-1a over each block's name, so the block table's order doesn't matter, and each
  column's roof; tree tints, floats that only colour leaves, are left out. It takes 150 to 250
  ms on one core. The server starts it on a task as the world opens (`WorldChecksum`) and puts
  it in `Welcome { seed, checksum, player }`, waiting for it only if someone joins sooner.
  The client's `join::arrive` spawns its player at once (so its inventory has somewhere to go)
  and works its own out on a task; `check_world` makes the `World` once they match, or says
  "Your copy of rustcraft generates a different world from the host's…" in the command bar
  and the log and disconnects. `GenerationChecksum` holds the function the client uses, which
  a test swaps for one that disagrees. `PROTOCOL_VERSION` is 2. A golden test pins seed 3's
  checksum, so any change to what's generated fails it until the value is updated
  deliberately (§8), and CI's `determinism` job runs it on Linux, Windows and macOS.
- **Built (M0.6c):** edit sync (`src/server/sync.rs`, `src/client/edits.rs`).
  - **Areas:** a welcomed client's entity gets a `SyncedArea`: its player's chunk, its reach,
    the chunks whose edits it's been sent, and those in reach it's still to be, nearest first.
    The client tells its reach as it's welcomed, `EditReach(full_detail_reach)`, 36 chunks with
    the default settings (0, its own chunk, until then). As its player crosses into another
    chunk, `follow_players` forgets sent chunks more than 4 chunks beyond its reach (so
    walking along the edge doesn't send the same ones again; keeping every one would keep
    sending far-off clients changes for chunks they no longer draw) and lines up the edited
    chunks in reach not sent, by a scan of every edited chunk, which is quick even for
    thousands. `send_chunk_edits` sends each client up to 4,096 edits a frame, about 12 KB,
    as `ChunkEdits`, a chunk's edits as region files have them: a palette of block names and
    each edit as `(index in chunk, palette index)` (`region::local_index`), 3 bytes an edit.
    50,000 edits round the spawn in 32 chunks went in 149 KB over 11 frames (a test).
  - **`BlockChanged` by area:** `receive_edits` sends changes through `sync::BlockChanges`,
    to the host (`ClientId::Server`, whose client shares the world) and to each client whose
    area has the chunk sent or in reach. One out of reach is sent the chunk's edits as it
    comes near, and a chunk with no edits before is sent again whole the next time its
    client moves, which is rare and cheap. `Welcome` carries `you`, the client's `ClientId`.
  - **Client:** `apply_chunk_edits` puts the edits into its `World` (`World::edit`), so a
    chunk not loaded takes them as it's generated and one loaded is rebuilt, before
    `apply_block_changes`, as a chunk's edits are older than the changes after them. Both
    keep what comes before the client has its `World` (while it checks the checksum) and
    apply it once it does, so nothing sent before the world's made is lost.
  - **Prediction:** a client joined to a server makes its own `RequestEdit`s in its world at
    once (`predict_edits`) and keeps `Predictions`: the block of each request unanswered,
    and for each such block what the server has there, as last told, and how many are
    unanswered. While any are, the server's changes and chunk edits there update only what
    it has; once the last is answered, `BlockChanged { by: (you, seq) }` or `EditRejected`,
    the block becomes what the server has, which is what was predicted unless it refused or
    someone else changed it. A confirmation that changes nothing rebuilds nothing, so a
    predicted edit is meshed once. Rolling back to the tracked value rather than
    `EditRejected`'s `actual` stays right when a later `BlockChanged` overtakes the refusal on
    its own channel. On the host and in singleplayer nothing's predicted, as the server
    answers in the same frame.
  - **Order:** replicon gives each message its own channel, ordered only within itself, so
    under packet loss a chunk's `ChunkEdits` and a `BlockChanged` can overtake each other.
    The server counts the `BlockChanged`s it sends each client, and each `ChunkEdits` says
    how many came before it; the client counts them as they come (`EditOrder`, a joined
    client's alone). A change no newer than its chunk's latest edits is skipped, and so is a
    block in a chunk's edits that a newer change has set. Changes are remembered by block only
    until the next chunk's edits arrive, as those come in order and nothing on its way can be
    older. `BlockChanged` itself is unchanged. `PROTOCOL_VERSION` is 3.
  - **Known gaps:** applying a chunk's edits to a loaded chunk
    sets them a block at a time into its runs, which is slow for a chunk with thousands; on
    joining chunks aren't loaded yet, so this only bites a client walking into a big build.
  - `tests/multiplayer.rs` adds: edits made before joining arrive and show as the chunk's
    generated, and those far off only once the player goes there; a change far from a
    client isn't sent to it but reaches it as it comes near; a client sees its own edit at
    once and undone when it's refused; edits sent while a slow client checks its world wait
    for it; a chunk's edits and a change overtaking each other (injected) leave the newer; and
    the cost of joining among 50,000 edits.
- **Built (M2.8):** LOD edits for what a client sees beyond full detail (§4.2).
  - **Protocol:** `EditReach { full_detail, view }` now says how far the client draws, in
    chunks, as well as how far it wants edits, and `EditSummary` carries LOD edits: by chunk,
    each 2-block cell's place in it (0–511), its commonest block built as an index into a
    palette, and its built and cleared counts, a nibble each, 3 or 4 bytes a cell. Every cell
    goes as it is when it's sent, on one ordered channel, so a client putting them in as they
    come, `LodEdits::set` working out the coarser cells as the server does, ends up with the
    server's. `PROTOCOL_VERSION` is 13.
  - **Areas:** `SyncedArea` keeps a second, wider area out to the view: as its player moves,
    chunks with edits that have come into view are lined up, nearest first, and those more
    than 4 chunks beyond it forgotten. Every change through `BlockChanges` notes its 2-block
    cell for the clients that see its chunk, and a chunk with no edits before is taken into
    view by its first. `send_edit_summaries` sends each client the cells changed and as many
    chunks' cells as make up 4,096 cells a frame, in one `EditSummary`. A client uses only
    these for its LODs, near and far, and its `ChunkEdits` and `BlockChanged`s for full detail.
  - **Bandwidth:** a house of 172 blocks is 44 cells, 179 bytes; a 50,000-block tower 1.6 km
    off, 6,300 cells, 23 KB, against 149 KB as `ChunkEdits`. A block changed far off is about
    20 bytes.
  - **Known gaps:** the view is the set render distance, so flying high, which draws further,
    shows no edits beyond it.
  - `tests/multiplayer.rs` adds: a client 1.6 km from a 50,000-block tower, with none of its
    edits, is sent its LOD edits and has every level's cells as the host has them, and a change
    there by the host reaches it.
- **Built (M0.6d):** other players (`src/server/players.rs`, `src/client/others.rs`,
  `src/client/models.rs`, `src/player/name.rs`).
  - **Names:** `PlayerName`, up to 16 letters, digits, `_` and `-`, so it's safe as a file name
    anywhere; anything else is left out, and nothing left is "Player". A game plays as
    `RC_NAME`, or else the user it runs as (`USER`, `USERNAME`). A client sends its name as
    it connects, in netcode's 256 bytes of user data (`protocol::name_to_user_data`), so it's
    there as the client's authorised and `welcome` needs no message to wait for. The server
    makes it unique, ignoring case as some file systems do, by the first number after it free
    (`alex`, `alex2`). The host's comes in `WorldToOpen::name`. Joining and leaving print "X
    joined the game" and "X left the game" in everyone's command bar, as `CommandOutput`.
  - **Replication:** players are spawned `Replicated` (`players::spawning`) with
    `PlayerName`, `GameMode` and `HeldItem` (what's in the selected hotbar slot, which the
    server keeps up to date from the inventory and the slot each pose names) replicated.
    `PlayerOwner` is now immutable and a replicon visibility filter, so a player is shown to
    every client but their owner's, which moves its own; nothing else, dropped items
    included, is hidden by it. Their place and look go as `PlayerMoved { player, at,
    position, yaw, pitch }`, unreliable and mapped to the client's entity: the server relays
    each pose it accepts to everyone but its sender as it arrives, and sends the host's every
    `POSE_INTERVAL` (50 ms), to clients only. A pose is stamped with its sender's clock (`at`,
    milliseconds), so receivers space poses by when they were made, not by when they arrived.
  - **Interpolation:** a client sees each other player (the host's App the joiners', a
    joiner the host and the rest) as a `SeenPlayer` entity of its own, a relationship whose
    target `Seen` is on the player, so it goes when they do, on the host as the server's
    entity is despawned and on a joiner as replicon despawns its copy. On the host the
    server's entity can't be drawn from, as its `Transform` jumps to each pose. `Poses` keeps
    the latest 8 and shows the moment 100 ms behind the newest, two poses' worth: between the
    two either side, yaw the short way round, or past the newest the way the last two were
    going, for at most another 100 ms. What's shown moves on at the client's own pace,
    drifting back towards 100 ms behind at a tenth of its error per 100 ms, and jumps there if
    it's fallen 200 ms behind; a pose overtaken by a newer one is dropped. It's hidden until
    its first pose.
  - **Models** (`DrawPlayersPlugin`, beside `DrawDropsPlugin`, whose item meshes it draws in
    the right hand): boxes in flat colours, two blocks tall as the body is: legs of trousers,
    a torso in one of six shirts chosen by the name's hash, bare arms, a head with hair and
    eyes, facing -z as the look does. Five meshes and ten `StandardMaterial`s for everyone,
    like dropped items, which don't bend with the curvature shader either, as neither is
    drawn far off. The body turns with yaw and the head with pitch; legs and arms swing with
    the distance walked across, more the faster up to a walk. The name is a UI label placed
    each frame where the camera sees the head, hidden behind the camera or past 64 blocks.
  - **Plausibility:** `PlayerPose` now carries `at`, `movement`, the hotbar `slot` and
    `teleports`. The server moves a client's player only if it could have got there:
    `PoseCheck` keeps how far ahead of the server's clock its movement has got, in seconds of
    moving at `physics::top_speed` (flying 80 blocks a second straight or rising, 113
    diagonally; walking 5.6 across, 9 up, 60 falling, flying taken as walking unless the game
    mode flies), with 10% over for rounding, and refuses a pose that would take it more than
    half a second ahead, or that walks the body into a solid block it wasn't already in.
    Bursts from jitter and hitches are let through; going faster than the mode allows for
    longer isn't. A refused pose answers `Teleport { position }` with where the server has the
    player, and so does `/tp` on a player from elsewhere. The client counts the teleports it's
    heard of and says how many with each pose, so the poses it sent before hearing, from where
    it no longer is, are ignored rather than refused again. It stops the player as it moves
    them. `GameModeChanged` tells a client its player's game mode as the server changes it,
    and `land_when_flying_is_lost` lands them. `REACH_SLACK` is 3: a block for rounding and
    two for 0.1 s, a pose's interval and a one-way trip, of flying at the usual 20 blocks a
    second; at the sprint's 80 it'd be 9, too lenient for what's rare while building.
  - **Saving:** `world.toml` keeps the host's player, unchanged (format 2). Each player from
    elsewhere is `players/<name>.toml`, a `PlayerState`, all read as the world opens (as
    edits are), and written by the autosave when changed, as they leave (which saves at the
    next frame's autosave) and as the game quits. A joiner arrives where their save has them.
    Older games ignore the directory, so the format isn't raised.
  - `PROTOCOL_VERSION` is 5.
  - **Known gaps:** names aren't authenticated, so anyone can join as anyone and take their
    save, and since M0.6e their operator's permission, which is fine among friends. Labels
    show through terrain. The host's own player is only relayed, never
    checked, as it's the server's.
  - `tests/multiplayer.rs` adds, with up to three Apps: two joiners named alike get names of
    their own, each sees the host and the other but not themselves, and one leaving goes from
    the other's screen with a line saying so; a walker and the host are seen moving between
    their poses, not jumping from one to the next; a pose a hundred blocks off or into the
    ground is refused with a `Teleport` back; `/tp` and `/gamemode` reach a joiner's own
    player; and a joiner comes back to a server opened again from the save where they left,
    looking the same way with what they carried. Tests that moved a client far now `/tp` it.
- **Built (M0.6e):** chat and operators (`src/server/chat.rs`, `src/server/commands.rs`,
  `src/commands.rs`).
  - **Chat:** a line typed into the command bar without a leading `/` goes as `Chat(text)`;
    the server takes out control characters, trims it and cuts it to `MAX_CHAT_LENGTH` (256
    characters, which the bar also stops typing at), and sends `ChatLine { from, text }` to
    everyone, the host included, which the bar prints as `<name> text` and fades like any
    other line. In singleplayer that's the player's own line coming back. T or Enter opens
    the bar, which while open shows the last 20 lines. Each client may say 5 lines at once
    and then one a second (`ChatAllowances`, the same token bucket as `EditAllowances`, now
    `server::allowances`), which lets a quick exchange through but not a flood of
    everyone's bar; `/msg` counts against it too. Over it, the sender is told it's too quick.
  - **Operators:** `Operators`, the names in `world.toml`'s `ops`, compared ignoring case as
    names are. It's optional and left out when empty, and older games ignore it, so the
    format isn't raised. The host
    (`ClientId::Server`) always is one, whatever its name. Which commands only operators may
    run is one table in `commands.rs`, `USAGES`, which `/help` prints from, showing each
    player only what they may run: `/tp` and `/gamemode` (anything that changes the world or
    another player), `/op` and `/deop`. `/seed`, `/list`, `/msg` and `/help` are anyone's.
    `parse` refuses an operators' command to anyone else before reading its arguments, with
    "You don't have permission to use /…". There's no `/time`, but `[` and `]` wind
    everyone's clock (M0.5e), so the server ignores `WindClock` from anyone else, telling
    them "Only operators can change the time" as they start winding, and `/help` lists the
    keys among the operators' commands.
  - **Commands:** `/tp <player>`, `/tp <player> <x> <y> <z>` (`~` from where the sender is,
    as in Minecraft) and `/gamemode <mode> [player]` act on others, who are told who did it;
    `/op <name>` and `/deop <name>` take any name a player could have, here or not, save
    soon (`WorldSave::save_soon`) rather than at the next autosave, and tell the player if
    they're here; the host can't be de-opped. `/list` names everyone here, and
    `/msg <player> <text>` sends "X whispers to you: …" to them alone, as `CommandOutput`.
    A player's found by name ignoring case.
  - `PROTOCOL_VERSION` is 8.
  - **Known gaps:** operators are only as safe as names, which aren't authenticated (M0.6d):
    anyone who can reach the server can join under an operator's name and run what they
    may, so a host should only open a world to friends until clients are authenticated.
    Chat isn't saved, so a joiner sees only what's said after they arrive.
  - `tests/multiplayer.rs` adds: chat reaches the host and every joiner under the sender's
    name, tidied and cut short; a flood of chat is held to 5 lines and the sender told, and
    a second later another gets through; a joiner who isn't an operator is refused
    `/gamemode`, `/tp` and `/op` and shown only the open commands in `/help`, until the host
    `/op`s them, which `world.toml` keeps, after which they can change the host's game mode
    and teleport them but not de-op them, and a `/deop` takes it away again; a joiner's
    winding changes nothing until they're an operator; `/msg` reaches only its target; and
    `/list`. Tests that `/tp` a joiner make them an operator first.
- **Built (M-R1):** the dedicated server (`src/bin/server.rs`, `src/server/console.rs`).
  - **The binary:** `rustcraft-server` is `DedicatedServerPlugins` (`StatesPlugin`, replicon
    and renet2, `ProtocolPlugin`, `ServerPlugin`, `SavePlugin`, `GameModePlugin` and
    `ConsolePlugin`) on `MinimalPlugins`, logging to stdout, with Bevy's Ctrl-C handler, which
    `ctrlc`'s `termination` feature makes catch SIGTERM too, so stopping it as a service saves.
    It runs 30 frames a second: clients pose at 20 Hz and are seen 100 ms behind, nothing the
    server runs needs more, and it halves what an idle server costs the machine; replicon's
    fixed tick is set to match, so it sends once a frame. `--world <name or path>`, `--port`
    and `--seed`, or `RC_WORLD`, `RC_HOST` and `RC_SEED`, and `--help`. It takes players with
    `server::host`, and exits if it can't bind the port.
  - **No host:** `WorldToOpen::host` is the host's name, or none, which spawns no player, so a
    dedicated server only has joiners'. `world.toml`'s `[player]`, the host's from playing the
    world in the game, is kept as it was, so a world moves between the game and a server.
    Residency, roofs and light needed nothing: the server holds chunks round its players
    alone, and `World` records reroofs and relighting only in chunks a client in the same App
    holds, so with none it records nothing to drain (a test). The night's skipped once every
    player there sleeps, all of them joiners. The clock runs while nobody's on, as furnaces do:
    the world goes on, as a Minecraft server's does, and costs nothing with no chunks loaded.
    `run_clock` now counts in an `f64` beside `TimeOfDay`: an `f32` of 1,024 days can't add a
    30th of a second (5.6e-5 days), so a server left on for a week would have stopped its
    clock, and crops with it, which grow by the days it counts.
  - **Console:** a thread reads lines from stdin (`Console`), and `ConsolePlugin` writes each
    as the App's own `RunCommand` or `Chat`, which replicon hands the server from
    `ClientId::Server` as it does the host's, and logs the `CommandOutput` that comes back. So
    it runs the server's commands as they are: always as an operator, with no player of its
    own, so `~` is from the spawn and `/tp` and `/gamemode` must name a player, and `/list`
    can say nobody's here. Commands are typed with or without the `/`; `say <text>` chats as
    "Server" (`CONSOLE_NAME`), `stop` saves and exits, and `help` lists them. A server with
    stdin closed, as a service, reads nothing and runs on.
  - **The `client` feature**, on by default, gates Bevy's render, window, winit, gilrs, x11
    and UI features, and audio since M-A3, and the modules that need them: `client`, `clouds`,
    `curvature`, `debug`, `horizon`, `hotbar`, `hud`, `interaction`, `menu`, `picker`, `sky`,
    `sky_dome`, `tonemap`, the inventory's screens, `player::{input, underwater}`, and the chunks' loaders, LOD
    drawing, fades and materials (`TerrainPlugin` now in `chunks/terrain.rs`). What they held
    that the server needs moved out: `Hotbar` to `inventory`, `overlaps` to `collision`,
    `REACH` to `player`, and the command bar to `client::command_bar`, leaving `commands.rs`
    the parsing the server runs. `cargo build --release --bin rustcraft-server
    --no-default-features` builds 234 crates rather than 397, with no wgpu, winit, gilrs or
    X11, into a 36 MB binary (20 MB stripped, against 105 MB with the client's features)
    linking only libc, libm and libgcc. Idle, it takes 2% of a core and 28 MB. Bevy's `bevy_mesh` stays, as generation builds meshes
    beside blocks, and `serialize`, for the entities messages name, which `bevy_window` turned
    on before; `trace` moved to the client, as its spans marked every line of the server's log.
    CI lints that build too; `lod_sim` and `tests/multiplayer.rs` need the feature.
  - A joiner can't be called "Server", which `welcome` numbers like a name taken ("Server2"),
    so nobody chats as the console. `PROTOCOL_VERSION` is 11.
  - **Known gaps:** the console's `help` lists `[ and ]`, which it can't press. Started with interrupts
    ignored, as a script's background job is, `ctrlc` declines to handle signals, so SIGTERM
    stops the server without saving; from a terminal, systemd or Docker it saves. A client
    quitting doesn't say so, so the server sees it leave only once it times out, about 15
    seconds on. A client's clock still adds in an `f32` between syncs, so after 512 days at
    60 frames a second its sky moves only at each sync, 5 seconds' worth at a time.
  - `tests/multiplayer.rs` runs `DedicatedServerPlugins` with a console fed by a channel:
    nobody's there until a client joins; `list`, `op` and `say` reach it, with or without the
    `/`; the clock runs with nobody on; building in the air records no reroofs; leaving saves
    the player; and `stop` saves the operators and edits with no `[player]`. `src/save/` checks
    a host's `[player]` survives a server, and `server::clock` that the clock runs on at 2,000
    days.
- **Built (M3.1):** creatures (§5.8).
  - **Replication:** each is spawned `Replicated` with `Creature` (its kind, by name),
    `CreaturePose { at, position, yaw }` and `Flinches` (how many times it's been hurt). The
    server sets the pose every `POSE_INTERVAL` while it moves or turns, stamped with its own clock
    in milliseconds, so it's sent 20 times a second at most and not at all while it stands still;
    a client keeps each pose in the same `Poses` buffer as other players' and draws a
    `SeenCreature` of its own from it, 100 ms behind, on the host as on a joiner. Its health,
    brain and way stay on the server.
  - **Visibility:** a creature has an immutable `Whereabouts`, its 32-block cell of the world,
    inserted again as it crosses into another, and each client's entity a `Viewpoint`, its
    player's cell, likewise; replicon's filter shows a client the creatures within 3 cells across
    of its player (96 to 128 blocks), and despawns them on it as they go further. Crossing a cell
    re-evaluates one creature against every client, or one client against every creature, which
    is cheap for hundreds.
  - **Hitting:** `HitCreature { creature }`, mapped from the client's entity to the server's, as
    the player left-clicks a creature nearer than any block (`interaction::TargetCreature`). The
    server checks the player's alive and the creature's box within `REACH` and its slack of their
    eye, and that it isn't still hurt from the last hit (0.5 s), which also holds a flurry of
    clicks to one.
  - **Residency:** besides the chunks within 2 of a player's eye, the server holds those within
    4 across in the eye's layer and the one above and below (149 chunks a player, not 33), so
    creatures have ground to walk on out past the 48 blocks they spawn within, on a dedicated
    server too. They're let go a chunk further out, as before. Generation is on the compute pool;
    in singleplayer the client holds all of these already.
  - `/summon <creature> [<x> <y> <z>]`, for operators, brings one. `PROTOCOL_VERSION` is 14.
- **Built (M2.4a):** `RequestEdit::to`, `BlockChanged::to` and `EditRejected::actual` are a
  `Block`, a type and its state, sent as its text, and `ChunkEdits`' palette names blocks with
  their states. A door's other block, and what loses what held it up, is sent as a
  `BlockChanged` by nobody. `UseBlock` on a door or trapdoor opens or shuts it (both of a door's
  blocks) for anyone in reach, in either mode. `PROTOCOL_VERSION` is 15.
  - `tests/multiplayer.rs` adds: a joiner sees a sheep dropped beside it fall smoothly between
    its poses, isn't shown one beside the host 300 blocks off, and hurts it with a hit, its
    flinch reaching it.
- **Layout:** `src/protocol.rs`; `src/server/` (`mod.rs` with `ServerPlugin`,
  `DedicatedServerPlugins` and `is_authority`, `edits.rs`, `chunks.rs`, `commands.rs`,
  `chat.rs`, `allowances.rs`, `clock.rs`, `console.rs`, `host.rs`, `players.rs`, `sync.rs`,
  `creatures/`); `src/save/` under the server; `src/client/` (`mod.rs` with `ClientPlugin` and
  today's scene setup, `edits.rs`, `player.rs`, `join.rs`, `others.rs`, `models.rs`,
  `creatures.rs`, `command_bar.rs`), all
  behind the `client` feature with the rest of what draws. `src/bin/server.rs` is the dedicated
  server. Shared modules stay where they are.

### 4.5 Game modes (proposed)

- **Survival:** health, hunger (D-5), breaking takes time and the right tool, finite blocks.
- **Creative:** flying, any block from a picker, instant breaking, no damage.
- **Spectator (later):** fly through blocks, no interaction.

Switched with `/gamemode`, which in multiplayer needs operator permission.

- **Built (M0.4):** `GameMode` (`src/game_mode.rs`) is a component on the player, so each player
  can have their own, saved with them (§4.3). Everything the modes change asks `GameMode`'s
  methods (`can_fly`, `has_every_block`, `breaks_at_once`, `drops_broken_blocks`). Creative
  flies (F toggles), breaks at once, has the block picker and never uses blocks up. Survival
  can't fly, and switching to it drops a flying player into walking; **since M1.1** it places
  only what's in the hotbar slot held, using it up, and what it breaks drops its item (§5.2),
  and **since M1.2** breaking takes time and the right tool (§5.1). Middle click only selects
  a block already in the hotbar. `/gamemode survival|creative` (or `s`/`c`) switches, keeping
  the inventory as it is.
- **Survival is the default since M1.2**, once breaking times and crafting made it a game: a
  new player, or one saved before game modes, starts in survival, walking, with nothing.
  `/gamemode c` switches to creative. (Until then it was creative, as survival with instant
  breaking and nothing to make was only a slower creative.)

---

## 5. Game systems

### 5.1 Interaction

- Look at a block within reach (about 5 blocks) with a voxel raycast, and outline it.
- Left mouse breaks: instant in creative; in survival over a time set by hardness and tool, with
  cracks drawn on the block.
- Right mouse places the held block against the face looked at, unless it would overlap a player,
  or uses a block (opening a chest, a door, a crafting table).
- Mouse wheel and 1–9 pick the hotbar slot. Q throws one of the held item, Ctrl+Q the stack.
- **Built (M0.2):** `src/interaction.rs` steps a ray from the eye through the blocks it crosses
  (a DDA), stopping at the first solid one or plant within 5 blocks, so it passes through air and water,
  and outlines it with a gizmo. Left click breaks it at once, right click places the hotbar's
  selected block against the face looked at, or in place of a replaceable block looked at (tall
  grass), into air, water or a plant but not into the player's body
  (checked by the server since M0.5b, §4.4), and middle click picks the block looked at into the
  hotbar (§5.2). None of it happens while paused, in the inventory or with the command bar open.
  Since M1.1 survival places from the hotbar slot held, and breaking drops items (§5.2).
- **Built (M2.4a): placing with state, doors and climbing** (`src/building.rs`, §4.1).
  - **The state a block's placed in** is the client's, from where the look meets the block
    (`Hit::point`, from the box of its shape the ray meets, so a slab's top is clicked on its
    top) and the face: stairs face the way the player looks, and slabs, stairs and trapdoors
    take the half of a side clicked, the top against a ceiling; logs lie along the face's axis
    and barrels face out of it; furnaces face the player; ladders and vines go flat against
    the face clicked; lanterns hang under a ceiling; doors face the way the player looks and
    turn on the side of their middle clicked. The server only checks it's a state the block
    can be in (`Block`'s text can't say another), as players are trusted with which way they
    face (§4.4).
  - **Held up:** ladders need a whole solid block behind them, vines that or vines over them,
    doors and standing lanterns solid ground, hanging lanterns a solid block over them, and
    plants and torches as before (`needs_support`, `supported`). The server refuses one placed
    with nothing to hold it, and takes away, leaving its item, whatever an edit leaves
    unsupported on any side of it.
  - **Doors** are placed by their lower block, which the server puts the upper one over, if
    there's room for it and no one in it; breaking either takes both and leaves one door.
    Right-clicking a door or trapdoor in either mode sends `UseBlock`, and the server opens or
    shuts it; an open door stands against its hinge side, so it can be walked through.
  - **Collision** is by the boxes of each cell's shape (`collision::boxes_at`), fences and walls
    standing 1.5 tall so they can't be jumped. Walking steps up onto anything up to 0.6 high
    without jumping, as onto a slab or a stair, if it gets further across that way. In a
    ladder or vines a body climbs at 3 blocks a second while jumping or walking into it, and
    otherwise slides down no faster; the server's falls count climbing as no fall, as water,
    and standing on any box's top as standing.
  - **Outline** draws each box of the shape looked at.

- **Built (M1.2):** breaking in survival takes time (`src/breaking.rs` holds the rules both
  sides use).
  - **Break time** is the block's `hardness` (§4.1), its seconds by hand, divided by the speed
    of the tool held if it's the kind the block names and of the tier it needs, or by 1
    otherwise. A block broken without that tier, by hand or with too low a tier or the wrong
    kind of tool, takes as long as by hand and drops nothing. The numbers, with tools' speeds
    of 4, 6, 8 and 10 from wood to iron (§5.2):

    | Block | Hardness (s by hand) | Tool | Tier | Wood | Stone | Copper | Iron |
    |---|---|---|---|---|---|---|---|
    | Plants, torches | 0 | | 0 | | | | |
    | Leaves, snow | 0.3 | (snow: shovel) | 0 | | | | |
    | Dirt, sand, ice | 0.75 | shovel (ice: pickaxe) | 0 | 0.19 | 0.13 | 0.09 | 0.08 |
    | Grass, gravel | 0.9 | shovel | 0 | 0.23 | 0.15 | 0.11 | 0.09 |
    | Logs, planks | 3 | axe | 0 | 0.75 | 0.5 | 0.38 | 0.3 |
    | Crafting table | 3.75 | axe | 0 | 0.94 | 0.63 | 0.47 | 0.38 |
    | Sandstone | 4 | pickaxe | 1 | 1 | 0.67 | 0.5 | 0.4 |
    | Stone, cobblestone, bricks, stone bricks | 7.5 | pickaxe | 1 | 1.9 | 1.25 | 0.94 | 0.75 |
    | Furnace | 8 | pickaxe | 1 | 2 | 1.33 | 1 | 0.8 |
    | Coal ore | 9 | pickaxe | 1 | 2.25 | 1.5 | 1.13 | 0.9 |
    | Copper ore | 9 | pickaxe | 2 | (9, nothing) | 1.5 | 1.13 | 0.9 |
    | Iron ore | 9 | pickaxe | 3 | (9, nothing) | (9, nothing) | 1.13 | 0.9 |

  - **Holding to break** (`interaction::break_target`, `Breaking`): in survival the left mouse
    is held on a block until it's been held as long as it takes with the hotbar's selected
    tool. Looking away or letting go starts again from nothing, and so does leaving play for a
    menu or the inventory. After a block breaks, the next starts a quarter of a second later, as
    in Minecraft, so a held click doesn't mow through plants a block a frame. Creative breaks at
    a click, as before.
  - **Cracks** (`src/client/cracks.rs`): a cube a hair bigger than the block, alpha-blended,
    draws one of 8 stages (`CRACK_STAGES`) of `assets/textures/cracks.png`, a network of
    cracks that spreads from the middle, drawn by the block-textures skill's `cracks.py`.
  - **The server checks it** (INV-4): the client sends `StartBreaking { block }` as it starts
    on a block (not one that breaks at a touch), and the server notes the time
    (`BreakingStarted` on the player); a survival `RequestEdit` breaking a block is refused
    unless that block was started on at least its break time, for the tool in the slot the
    request names, ago, less 0.1 s of slack (`BREAK_SLACK`), and a break spends the start, so a
    block put back in the same place takes as long again. The client times its hold from the
    frame it sends the start to the frame it sends the edit, which in one App is the server's
    timing to the frame; over a network both take about as long to arrive, so their delay
    cancels, all but jitter and a server frame's wait. A joined client predicts the break like
    any edit (§4.4): the block goes as the hold ends, and comes back if the server refuses. A tenth lets no one break stone with a
    wooden pickaxe more than a twentieth quicker.
  - **Wear:** each block broken with a tool in survival, but for one that breaks at a touch,
    wears it by 1 (§5.2); a tool worn to its durability breaks and is gone. Leaves still drop
    nothing: apples and saplings come with farming (M1.9b).

### 5.2 Inventory and items

- Items are data (INV-6): block items for every placeable block, plus tools, materials, food.
- Hotbar of 9 slots plus a main inventory, with stacks (64 by default, tools 1). Durability on
  tools.
- Dropped items exist as small entities that can be picked up.
- Containers (chests, furnaces) hold their own inventories, stored as block entity state in the
  edit layer.
- **Built (M0.4):** `Hotbar` (`src/hotbar.rs`) is a component on the player: 9 slots of
  `Option<BlockType>` and the selected one, which M1.1 turns into item stacks with counts. It's
  drawn at the bottom centre with each block's tile from the atlas, the selected slot framed in
  white, and redrawn only when it changes. 1–9 and the wheel select, wrapping round; middle
  click selects the slot already holding the block looked at, or in creative puts it in the
  selected slot. Creative starts with nine building blocks. E in creative opens the block picker
  (`src/picker.rs`), a grid of every solid block with its name under the pointer; clicking one
  puts it in the selected slot. It's the `GameState::Inventory` state, which M1.1 makes the
  inventory too: the game runs on behind it, but the player, looking and breaking only run in
  `Playing`, the cursor's free, E or Escape closes it, and the command bar can't open over it
  (nor over the pause menu).
- **Built (M1.1):**
  - **Items** are `Item` in `src/items.rs`: `Item::Block(BlockType)` for every tangible block,
    which isn't a row of its own, so the item table can't fall out of step with `blocks!`, and a
    variant for each row of the `items!` table, which crafting fills with sticks, ingots and
    tools (M1.5). A row has a stable snake_case name and columns
    (`max_stack` so far, 64 for blocks); tools' tier, durability and speed, and food's hunger,
    join them as columns, and non-block items get tiles of their own to draw them with. Items
    are saved by name, as blocks are, and drawn in the UI by their block's tile
    (`inventory::icons`). A block row's `drops` (§4.1) says what breaking it leaves: grass
    leaves dirt, stone cobblestone, leaves and plants nothing for now, ores and the rest
    themselves.
  - **Stacks and inventories:** `ItemStack { item, count }` holds 1 up to the item's
    `max_stack`. `Inventory` (`src/inventory/`) is a component on the player: 36 slots, 0–8 the
    hotbar and 9–35 the main inventory, and the stack on the cursor in the inventory screen.
    Items picked up top up the stacks of them there are first and then fill the first empty
    slot, the hotbar's before the rest. `Hotbar` is now only the selected slot, a view of slots
    0–8, still chosen by the client; its strip draws each slot's icon and, in survival, the
    count of a stack of more than one in its corner, redrawn only when the inventory, the
    selection or the game mode changes. Creative starts with full stacks of nine building
    blocks, survival with nothing.
  - **The server changes them (INV-4)**, as clients ask (`src/server/inventory.rs`, beside
    `receive_edits`, so in one App in the frame it's asked): `InventoryClick { slot, click }`,
    `click` being `Left` (pick up, put down, top up or swap), `Right` (pick up half, rounding
    up, or put down one) or `Shift` (move the stack between the hotbar and the main inventory,
    topping up stacks first); `CloseInventory`, which puts the cursor's stack back or throws
    what doesn't fit; `DropItem { slot, all }`, from Q and Ctrl+Q; and `PickBlock { slot,
    block }`, a full stack of a block in a hotbar slot for a player with every block, from the
    picker or middle click. A request for a slot there isn't changes nothing. `RequestEdit`
    names the hotbar slot it places from, and in survival the server refuses a place unless
    that slot holds the block, and takes one from it when it's made; creative never uses
    blocks up. Since M0.6a the server sends each client its player's `Inventory` as it changes
    (`InventoryChanged`, §4.4); nothing needs predicting, as clicks are answered within a round trip.
  - **Dropped items** (`src/server/drops.rs`) are `DroppedItem { stack, .. }` entities the
    server moves: a broken block's item hops out of where it was, a thrown one leaves the eye
    the way the player looks, and both fall under gravity, colliding with blocks through
    `collision::move_and_collide` (ground not loaded holds them, as it does players), sliding
    to a stop, and rising out of a block placed over them. Two of the same item within a block
    of each other gather into the older, up to a full stack (every pair is looked at, which is
    fine for the few dozen there are). A player within 1.5 blocks of one picks up what their
    inventory has room for, after half a second for a block's item and two for one thrown, so
    the thrower can get it away. They go after five minutes, and aren't saved: a world reopened
    has none. The client draws each as a cube a quarter of a block across with its block's
    tiles, spinning and bobbing over the spot (`src/client/drops.rs`): one mesh per item and
    one `StandardMaterial` on the block atlas for all, so they batch. They're lit by the sun
    even underground until block light (M1.7) gives them something better.
  - **The inventory screen** (`src/inventory/screen.rs`): E opens `GameState::Inventory` in
    either mode, now from the menu's key handling, and it shows the block picker in creative
    and the inventory screen in survival, which hides the hotbar strip. It has the 27 main
    slots over the 9 hotbar slots, buttons like the picker's, the hovered stack's name under
    them, and the stack carried following the mouse. Left, right and shift-click send
    `InventoryClick`, and closing it (E or Escape) sends `CloseInventory`.
- **Built (M1.5):**
  - **The `items!` table's rows** are a stick, copper and iron ingots, and a pickaxe, an axe
    and a shovel in each of wood, stone, copper and iron (swords wait for combat, M3.3). A row is
    a name and, for a tool, `tool(kind, tier)`, which reads its durability and speed from one
    `TIERS` table: wood lasts 64 blocks and breaks what it's for 4 times as fast as a hand,
    stone 128 and 6, copper 192 and 8, iron 256 and 10. Tools stack to 1. Ingots only come
    from smelting (M1.6), so the copper and iron tools' recipes wait on it.
  - **Wear is part of the stack:** `ItemStack { item, count, wear }`, `wear` counting the
    blocks a tool has broken (M1.2 wears them), so a tool keeps its wear when it's moved,
    dropped or saved. It's saved as `wear = n` beside the item and count, left out at 0, and a
    stack worn past its tool's durability, or a worn item that isn't a tool, fails the load.
    Two tools never stack (`ItemStack::stacks_with`), so clicking one onto another swaps them.
    A worn tool's icon has a bar under it, green when new through yellow to red.
  - **Sprites:** items that aren't blocks draw a 16px tile of `assets/textures/items.png`,
    one per row in the table's order (`Item::sprite`), drawn by the block-textures skill's
    `items.py`: tools lie on the diagonal, handle bottom left and head top right, in a shade of
    their material lit from the upper left and outlined. Dropped, they're an upright square of
    their sprite that spins, rather than a cube.

- **Built (M1.6): containers** (`src/containers.rs`), made to be reused by chests (M2.3).
  - **The model:** a `Container` is the block it's in (which says what kind it is: so far only
    the furnace, `Container::new`), its slots, and a `Fire`, which only a furnace lights. The
    server keeps every one in `Containers`, a resource beside the edit layer kept the way it
    keeps edits, by chunk and block in the chunk, and remembering which chunks' containers
    changed for saving. Clients never have it (INV-4). A container's made empty the first time
    its block's used, so a furnace placed and never opened has none, and goes when its block
    changes: breaking a furnace, in either mode, drops what was in it where it stood, as items
    are never lost with their container.
  - **Opening one:** right-clicking a furnace in survival sends `UseBlock` as a crafting
    table's does, and the client opens the inventory screen at it (`Station::Furnace`); the
    server, if it's in reach, makes it if need be and puts `OpenContainer` on the player until
    `CloseInventory`. Several players can have one open at once.
  - **Clicks:** `InventoryClick` names the container whose slot it's on, or none for the
    player's own, so a click meant for one the server's since closed changes nothing.
    `Container::click` works as an inventory's slot (the same `inventory::click_stack`), but a
    furnace's fuel slot takes only fuel and its output is only taken, all of it at once onto
    the cursor if there's room, and shift-click moves a stack into the inventory. Shift-click
    on the player's own slot with a furnace open moves what smelts into its input and fuel onto
    its fire, and anything else between the hotbar and the main inventory as before.
  - **Sending:** `send_containers` sends each player the container they have open as
    `ContainerChanged { at, container }` whenever its slots differ from what they were last
    sent, and while only its fire changes at most every 100 ms, which is enough for the flame
    and arrow to move smoothly at under a kilobyte a second. Once it's gone or out of their
    reach it sends `container: None` and closes it, which closes their screen. The host is sent
    it too, through replicon locally, so the client side has one path.
  - **The screen** (`src/inventory/furnace.rs`): at a furnace the crafting list gives way to
    its panel, the input over the flame over the fuel and an arrow to the output, drawn from
    `ContainerView`, what the server last sent. The flame shows the fuel left against how long
    it burns in all, and the arrow how far the item's got, each by showing that much of a lit
    tile over an unlit one (`assets/textures/furnace.png`, drawn by the block-textures skill's
    `ui.py`). Slots are the inventory's buttons and stack views (`StackView::Container`).
  - **Not done:** a furnace looks the same lit or not. A lit tile is cheap, but swapping the
    block for a lit furnace through the edit layer would rebuild meshes and send `BlockChanged`
    each time it lights and goes out. Since M2.4a it faces the player who placed it, its mouth
    on its front; a lit front would be a state, sent as a change each time it lights.
- **Built (M2.3): chests.** `chest` is a block (planks' hardness, quicker with an axe, its own
  top and a side with a latch on every face, as blocks have no facing yet) made from 8 planks at
  a crafting table, and a container of 27 slots (`Container::new`). Its rules are a furnace's
  without the furnace's: any slot takes anything, and a stack shift-clicked in from the
  inventory tops up the stacks of it there and then fills the first empty slot, as picking up
  does (`inventory::add_to`); shift-clicked out, it goes into the inventory. Opening, clicks,
  sending (`ContainerChanged`) and breaking are the furnace's paths unchanged, so several
  players can have one open and see each other's changes, and a chest broken leaves itself and
  what was in it. Its container's saved as a furnace's is, its fire never lit, so the region
  format stays 4.
  - **The screen** (`src/inventory/chest.rs`): right-clicking a chest in survival opens the
    inventory screen with the chest's 27 slots over the inventory's, and no crafting list.
    `ContainerView::opening` holds an empty container of the block's kind until the server's
    sent the real one, so the screen knows what it's showing from the first frame.
  - **Loot:** a chest the world generated is filled from a loot table the first time it's
    opened or broken (§5.6).
  - `PROTOCOL_VERSION` is 10, for the new block and recipe.

### 5.3 Crafting and progression

- Recipes are data (INV-6), and a recipe is a list of ingredients and an output, made at a
  station: by hand, at a crafting table, and for smelting at a furnace (D-4).
- Tool tiers set what can be mined: a block mined without its tier drops nothing (§5.1, built in
  M1.2 to tier 4). Coal needs only a wooden pickaxe, rather than stone as first planned, as it's
  for torches, wanted on the first night. Proposed tiers, to rename and tune freely:

  | Tier | Material | Mines | Where it's found |
  |---|---|---|---|
  | 0 | Hand | Dirt, sand, logs, leaves | Everywhere |
  | 1 | Wood | Stone, coal | Everywhere |
  | 2 | Stone | Copper | Everywhere, shallow |
  | 3 | Copper | Iron | Everywhere, mid-depth |
  | 4 | Iron | Regional ores | Specific regions (P3) |
  | 5 | Bright steel, an alloy of iron and a regional material | The deepest ores | Deep, everywhere |

- **Built (M1.5):** `src/crafting.rs`.
  - **Recipes** are the `recipes!` table: each row a `Recipe` variant with a stable name, sent
    by name as blocks and items are, its `Station` (`Hand` or `CraftingTable`), its ingredients
    as items and counts, and its output. A crafting table makes what hands can and more. By
    hand: a log makes 4 planks, 2 planks 4 sticks, and 4 planks a crafting table. At a table:
    each tool from 3 of its head's material (planks, cobblestone, a copper or iron ingot) and 2
    sticks, or 1 and 2 for a shovel; a furnace from 8 cobblestone; 4 stone bricks from 4
    cobblestone; and sandstone from 4 sand. Torches are made by hand, 4 from a coal ore and a
    stick, as coal ore drops itself for now; a coal item, from mining or smelting, can take its
    place later. Smelting comes with the furnace (M1.6). Making a recipe takes its ingredients from the main
    inventory before the hotbar, so what's held stays in hand.
  - **The crafting list** (`src/inventory/recipes.rs`) is beside the inventory screen's slots:
    two columns of buttons, each the output with its count, an arrow, and the ingredients with
    theirs, bright when what's carried makes it and dimmed when not, its full description
    under the slots as it's hovered. E opens it by hand, and right-clicking a crafting table in
    survival opens it at the table (`CraftingStation`), with the table's recipes after the
    hand's; in creative a table is built against like any block. Clicking a recipe makes it
    once into the inventory, rather than onto the cursor, so shift-click, which makes as many
    as the ingredients allow, works the same way.
  - **The server crafts (INV-4)** (`src/server/crafting.rs`): `UseBlock { block }` from right
    click opens a station for the player, if it's one and in reach, kept as `AtStation` on them
    until `CloseInventory`. `Craft { recipe, all }` makes a hand recipe anywhere, and a table's
    only while the player's open table is still there and in reach. What doesn't fit is dropped
    at the player, and shift-crafting stops there, so a full inventory never becomes a heap.
    Stacks are sent with their wear always, as postcard can't skip a field, and saved without
    it when there's none. `PROTOCOL_VERSION` is 4.
- **Built (M1.6): smelting.** The `recipes!` table's `Station::Furnace` rows are smelting,
  one item into one: copper ore into a copper ingot, iron ore into an iron ingot, and
  cobblestone into stone. `Station::makes` never crafts them, so they're neither listed nor
  made by `Craft`; a furnace's `Container::run` makes them over time.
  - **Rules:** an item takes 10 s (`SMELT_TIME`). Fuel is `FUELS`, in items smelted: coal 8,
    logs and planks 1½, sticks ½. A furnace with nothing burning takes one fuel only if its
    input smelts and its output has room for what it makes; once lit, a fuel burns to the end
    whether there's anything to smelt or not, as in Minecraft. An item part smelted when the
    fire goes out, the input's taken, or the output fills, starts again. `run` steps from event
    to event (the time's up, the fuel's burnt, an item's done), so a long step ends as frame
    by frame does (a test runs 1000 s both ways).
  - **Where they run:** a furnace reads and writes only its own slots, so the server runs every
    one that's burning each frame (`run_furnaces`, and `Containers` keeps the set that might
    be), whether anyone's near and its chunk's held or not. One left behind smelts on as if
    its player had stayed and needs no catching up when they're back; time doesn't pass for it
    while the world's closed, as for anything else. When regions load lazily, with
    containers coming and going with them, this needs to become catch-up from elapsed world
    time as a region loads.
  - **Coal:** the `items!` table has `Coal`, which coal ore drops (`Drops::Item`), and torches
    are made from a coal and a stick, so coal ore no longer drops itself and torches no longer
    take the ore. Coal ore still needs a wooden pickaxe to drop it. Charcoal and glass wait for logs and
    sand to have something to smelt into.
  - `PROTOCOL_VERSION` is 6.
- **Built (M2.4a): building blocks' recipes.** At a crafting table stairs are made 6 of
  their block into 4 and slabs 3 into 6, walls 6 into 6, fences of 4 planks and 2 sticks into
  3, doors of 6 planks into 3, trapdoors of 6 into 2, ladders of 7 sticks into 3, a lantern of
  an iron ingot and a torch, 16 panes of 6 glass, dark planks of 4 planks and a coal (stained)
  and light ones of 4 planks and a sand (scoured), 4 polished stone of 4 stone, mossy
  cobblestone and stone bricks of the block and vines, coloured terracotta of 8 terracotta and a
  flower or tall grass for green, a bookshelf of 6 planks and 3 wheat (books of pressed straw),
  and a barrel of 6 planks and 2 planks slabs. By hand, vines are plaited from 3 tall grass, as
  none grow yet. Furnaces smelt sand into glass, stone bricks into cracked ones and dirt into
  terracotta. Glass drops itself: there are no tools for taking it whole, and windows are
  rebuilt often. A barrel is a container of 27 slots, opened as a chest is. The crafting list
  is five columns wide to fit a table's sixty-odd recipes.

- **Progression needs travel (P3):** higher tiers need ores found only in certain climates (under
  glaciers, deep under deserts, in mountain cores), so moving up means going somewhere.
- **Built (M2.5): regional ores and bright steel.** Each far climate has an ore under it that
  needs an iron pickaxe; any of their materials blended with iron makes bright steel, tier 5,
  which mines the embers at the bottom of the world:

  | Material | Ore, under | Heights | Mined with | Made into |
  |---|---|---|---|---|
  | Gold ingot | Gold ore, deep under desert and savanna | 48–288 | Iron | Bright steel blend, gold block |
  | Frost crystal | Frost crystal ore, under glacier and taiga | 192–440 | Iron | Bright steel blend, frost crystal block, ember lamp |
  | Jade | Jade ore, under jungle | 160–424 | Iron | Bright steel blend, polished jade |
  | Bright steel ingot | (an alloy) | | | Tier 5 tools, bright steel block |
  | Ember | Ember ore, every climate, glowing faintly (light 5) | 16–112 | Bright steel | Ember lamp |

  - **Bright steel:** a crafting table blends an iron ingot with a gold ingot, a frost crystal
    or a jade into a bright steel blend, and a furnace smelts the blend into an ingot, as
    smelting stays one item into one. Any of the three will do, so a player heads for
    whichever far climate is nearest their spawn, rather than one fixed direction; the world
    is too big to send everyone to the same corner of it. Recipes keep to two ingredients, as
    the crafting list's cards fit no more.
  - **Tier 5 tools** (`BrightSteelPickaxe`, axe, shovel, hoe) take bright steel ingots as
    iron's take iron: durability 384 and speed 13, against iron's 256 and 10 (`TIERS`).
  - **Blocks:** the ores (hardness 10, embers 12) and, made 4 into 1 at a table, a gold block,
    a frost crystal block, polished jade and a bright steel block; an ember lamp, an ember in
    4 frost crystals, gives light 14 as a torch does but is a full block. Tiles and sprites are
    drawn by the `block-textures` skill's `draw.py` and `items.py` (INV-8).
  - **Loot:** the glacier outpost holds frost crystal, the desert temple gold ingots and jade,
    and the dungeon embers and gold; each now and then bright steel, ingots or a tool.
  - `PROTOCOL_VERSION` is unchanged: blocks, items and recipes travel by name. The golden
    checksum changed.

### 5.4 Underground: caves and ores

- Caves (`src/chunks/generate/caves.rs`) are carved from three 3D noise fields. Tunnels run
  where two fields' zero surfaces meet, so tunnels sharing a surface cross and join into
  networks; one field is stretched vertically, so most tunnels climb gently enough to walk. They're
  mostly 2–6 blocks wide. Caverns open where all three meet, growing with depth from a widening at
  a junction to halls twenty or thirty blocks across near the bottom of the world.
- The noise is sampled every 4 blocks and blended, and cells that can't hold a cave are ruled out
  from their corners, so a chunk underground costs about 15 µs more to generate.
- Caves stay 5 blocks under the ground and taper shut towards it. Nothing simulates water, so they
  stay 12 blocks under any sea floor within 16 blocks, rising 2 for each block away from it, and
  no water is ever over or beside a cave.
- Cave mouths break the surface on dry land where a 2D noise is high, a few dozen to a square
  kilometre, so caves can be found from above.
- LODs leave caves out, mouths included: caves are hidden from far away, so distant LODs keep
  drawing the surface, and LOD meshing pays nothing. Full-detail chunks with caves in them aren't
  opaque, so `is_chunk_buried` meshes them; buried LOD nodes (`lod/spawn.rs`) stay empty, since
  LOD meshes never draw caves. Showing big mouths at mid-range LODs is left for later (INV-5).
- Sky light falls straight down columns, so each chunk records which of its columns have ground
  over them (`ChunkData::roofed`), and light reaches into the chunks below only down open shafts
  (`Caves::sky_reach`). Edits keep it up to date down the loaded chunks under them
  (`World::edit`), so a shaft dug into a cave lights it. Cave interiors are dark but for the
  torches players bring (§5.5).
- A chunk sealed off from the sky, with every column of it and the chunks beside it roofed, is
  only seen from in the caves, so it's meshed only near the camera: within 9 chunks from under
  ground, further than sight reaches along a cave, and 3 from the open. On the surface the caves
  under the rest of the full-detail area cost nothing to draw.
- Ores (`src/chunks/generate/ores.rs`) turn stone to coal, copper and iron in veins: lumpy
  clumps grown a block at a time from starts hashed from the seed on a grid of chunk-sized cells,
  each staying within 2 blocks of its start, so a chunk takes them from its own cell and the 26
  around it (INV-2, INV-3). They replace only stone, after caves are carved, so they show on cave
  walls and bare cliffs. They're full detail only: LODs don't draw underground, so their output
  is unchanged. They add about 2 µs to a chunk underground, 2–3% of generating one, and the
  extra runs they split the stone into make meshing the chunks round them a few percent dearer.
  The bands, by absolute height (sea level 448), each thinning out over 32 blocks inside its
  ends, and the rates in the thick of them, tuned in the `ORES` table:

  | Ore | Heights | Veins per 16³ | Blocks a vein | Share of stone |
  |---|---|---|---|---|
  | Coal | 192 to the top | 3 | 3–12 | ~0.55% |
  | Copper | 128–456 | 2.5 | 3–8 | ~0.35% |
  | Iron | 16–392 | 1.5 | 2–6 | ~0.15% |

  Coal is in the stone from just under the surface; copper is a short dig under lowland ground;
  iron starts fifty or more blocks down, a deep dig or a walk down the caves. A tunnel through
  the thick of the bands passes a vein of coal or copper every thirty blocks or so, and of iron
  every sixty.
- **Built (M2.5): regional ores.** The `ORES` table gives each ore the biomes it's found
  under, by name as structures name them, or none for every climate. A vein grows as before,
  and each block of it turns stone to ore only where the column it's in belongs to one of its
  biomes, which every chunk knows for its own columns, so it's still local (INV-3) and a vein
  under a climate's edge stops at it. LODs are unchanged. The rarer ores come in bigger veins:

  | Ore | Under | Heights | Veins per 16³ | Blocks a vein | Share of stone |
  |---|---|---|---|---|---|
  | Gold | Desert, savanna | 48–288 | 0.4 | 6–16 | ~0.10% |
  | Frost crystal | Glacier, taiga | 192–440 | 0.4 | 6–16 | ~0.10% |
  | Jade | Jungle | 160–424 | 0.4 | 6–16 | ~0.10% |
  | Ember | Everywhere | 16–112 | 0.4 | 4–10 | ~0.065% |

  Gold is a long dig under the deserts, 150 blocks and more; frost crystal and jade start a
  little under the sea, a short dig under their lowlands. Embers glow (light 5), so they show in
  the dark of the deepest caverns. Tests check over many columns that each comes up under its
  own biomes and depths only, rarer than iron, and in real chunks under each far biome. The
  four new ores add about 9,500 instructions to generating a chunk underground, 0.5% (counted,
  as the machine was too loaded to time). The checksum samples two more layers, so they take
  in gold under the spawn's savanna and embers, besides frost crystal and jade under the taiga
  and jungle columns.
- Underground structures (mineshafts, dungeons) come with §5.6.

### 5.5 Survival

- Health with fall damage, drowning, and later mob damage. Death drops your inventory where you
  died and respawns you at your spawn point (a bed sets it).
- Hunger (D-5): a food bar that drains slowly, faster while sprinting, jumping or healing.
  Eating refills it; a fuller bar heals over time and an empty one hurts, down to half a heart.
  No saturation or exhaustion arithmetic beyond that.
- **Built (M1.8):** health, hunger, breath, death and beds, for players in survival; creative
  takes no damage and needs no food.
  - **State:** `Vitals { health, food, air }` (`src/survival.rs`) on each player: 20 health (ten
    hearts), 20 food, 10 bubbles, saved in `PlayerState` with the bed a player respawns on
    (`SpawnBed`), both with serde defaults, so older saves are whole and respawn at the world's
    spawn, and joiners keep theirs in `players/<name>.toml`. Only the server changes them
    (`src/server/survival.rs`, INV-4), sending a client its own as `VitalsChanged { vitals,
    asleep }` when they change; the host's are the server's own.
  - **Falls, and why the client can't skip them:** movement is the client's (§4.4), so the
    server judges falls from where it sees each player, frame by frame (a client's poses, or
    the host's body), against the ground in its own world (`Footing`, `surroundings`): feet
    within 0.05 of the top of a solid block under a corner of the body are standing. It keeps
    the highest point since a player last stood or swam, from the ground they left, however
    sparse the poses, and when it next finds them standing they take a point a block beyond 3
    (`fall_damage`, floored: 4 blocks hurt 1, 23 kill). Feet or middle in water breaks a fall.
    Clients say nothing about falling, so leaving damage out means lying about where they are,
    which the pose checks (M0.6d) catch past a slack. Not judged: moving more than 8 blocks
    between frames (being teleported or respawned, beyond any fall or flight in a pose's 50 ms),
    and falls before a player's first stood since arriving, respawning or being put somewhere,
    which is a new player dropping onto ground that's only just loaded (a test arrives one).
  - **Drowning:** 15 seconds of breath with the eyes under water, the bubbles counting it
    down over the food bar, then 2 damage a second; it comes back in 2 seconds above water.
  - **Hunger:** a point every 35 seconds just living, three times as fast sprinting (judged by
    the server from 0.5-second windows of poses faster than 5 blocks a second), 1/40 of a point
    a jump (leaving the ground upwards) and half a point a point healed. Above 17 food a player
    heals a point every 4 seconds; at none they're hurt a point every 4 seconds down to half a
    heart, so starving never kills. Eating is holding right click with food in the hotbar,
    `Eat(Some(slot))` and `Eat(None)` as it starts and stops: the server eats one every 1.6
    seconds while it's held, refusing when the bar's full or it isn't food.
  - **Death:** at no health the server drops everything carried where they fell, scattered
    (`DroppedItem::scattered`, a server `Chance` RNG), tells everyone "X fell from a high
    place" or "X drowned" in the command bar, and the dead pick nothing up. The client shows
    the death screen (`GameState::Dead`); its Respawn button sends `Respawn`, and the server
    puts them, whole and fed, on their bed if it's still there or the world's spawn, sending a
    joiner a `Teleport`.
  - **Beds:** `bed`, a cube for now, from 3 planks and 3 tall grass at a crafting table: a
    frame and a grass mattress, as there's no wool without sheep, and tall grass is the one soft
    thing a player can gather (it now drops itself). Since M3.2 it takes 3 wool instead (§5.8). Right-clicking one in reach (`UseBlock`)
    sets the respawn point, and at night (the sun down, 0.74 to 0.24) lies the player down
    (`Asleep`), fading their screen; moving half a block gets them up. Once every player has
    been asleep for 2 seconds the clock skips to the next morning (0.26) and everyone wakes.
  - **HUD** (`src/hud.rs`): hearts over the hotbar's left, the food bar over its right filling
    from the right, bubbles over it while short of breath, hidden in creative and behind the
    inventory screen; a red flash and a slight tilt of the view when hurt. The icons are
    `assets/textures/hud.png`, drawn by the block-textures skill's `hud.py`.
  - `PROTOCOL_VERSION` is 7.
  - **Later:** a two-block bed lain in, with a Leave Bed button; damage from mobs and lava; leaves dropping apples as they decay, once they do.
- Day and night already exist; night brings hostile mobs (§5.8).
- **Block light (built, M1.7).** Blocks that `emit` light (the torch, 14) light the blocks round
  them alongside sky light, so caves and nights can be lit. It loses a level a block, and the
  `light_opacity` of what it passes through, so a torch's light reaches 13 blocks
  (`BLOCK_LIGHT_REACH`), round corners but not through walls.
  - Like sky light it's worked out per mesh, on the compute pool, from the chunk and its 26
    neighbours (`neighbourhood_light`, `BlockLight`), for the chunk and a block round it. Every
    block that can light those is within 13 of them, so inside the neighbours already decoded
    for meshing: light crosses chunk borders exactly, with nothing stored, saved or kept in step
    (INV-1). It spreads brightest first, so each block is spread from once, and only as far
    out as it could still get back into the chunk. A chunk with no emitter in its neighbours
    pays only a scan of their runs for one, and meshing costs what it did. Meshing a chunk
    among torches on every floor 8 blocks apart costs about 0.05 ms more (a quarter more), and
    4 apart 0.08 ms, with the torches' faces and the faces their light stops merging.
  - Placing or breaking an emitter, or changing how a block lets light through within an
    emitter's reach, rebuilds the meshes that light reaches (§4.2).
  - Meshes carry it per vertex beside sky light, smoothed the same way, in the tangent attribute
    (Bevy has no spare one), and only meshes with any block light carry it, so the rest draw
    exactly as before. The shader lights a face by the brighter, colour by colour, of the sky's
    light, which follows the day, and a warm torchlight from block light, which doesn't. A
    torch's flame is drawn at full brightness whatever the light. Ambient occlusion is as before.
  - LODs leave edits out, so have no block light; torches aren't drawn in them, like plants.
  - Later: wall torches (blocks with a facing), lit water (its shader takes sky light only),
    a flicker, and coloured or dimmer emitters (lava, glowstone) in the `emits` column. Since
    M2.4a lanterns give 13, a little less than a torch.

### 5.5a Farming and food

- **Plants are a new kind of block**: drawn as two crossed quads rather than a cube, not solid,
  broken in one hit, and resting on the block below (they pop off when it goes). Tall grass and
  flowers use the same render style, which the meshers and LODs need to support (LODs can leave
  small plants out or draw them as a tint).
- **Farming:** a hoe tills dirt or grass into farmland; seeds (dropped by breaking tall grass)
  planted on farmland grow through stages over time, faster near water; ripe wheat drops wheat and
  seeds. Growth is server-side and deterministic in what it needs: a chunk's crops advance from the
  time they were planted, so unloaded fields catch up when they load rather than ticking forever.
- **Food:** bread from wheat at a crafting table; apples from leaves sometimes; meat from animals
  (§5.8). Food items say how much hunger they fill. Built: raw mutton and chicken fill 2, beef
  and pork 3, and cooked in a furnace mutton and chicken 6 and beef and pork 8 (M3.2).
- **Built (M1.9a):** the `Cross` render style and four plants, `tall_grass` (greyscale, tinted to
  the climate's grass), `poppy`, `dandelion` and `cornflower`, their tiles drawn by the
  block-textures skill's `draw.py`. The chunk mesher draws each as two quads through its cell,
  each wound both ways over shared vertices, facing up so they're lit as the ground is, with the
  light of their own cell; they sway from the roots up, more gently than leaves, and cut out their
  texture's holes in the shadow and depth passes as leaves do (`foliage.wesl`,
  `world_prepass.wesl`). LODs leave plants out, so they draw just as before. Generation puts one
  on a grass column's top by the column's hash alone (`generate/plants.rs`, INV-2, INV-3), on
  gentle ground with its top block intact: tall grass in tufts, dense where it's wet and warm and
  sparse where it's dry or cold, and flowers in patches of one kind, rarer. Deserts, glaciers and
  snow have no grass, so no plants. Trees grow over them. The server lets plants go only on solid
  ground and removes a plant whose ground is broken, as a `BlockChanged` of its own; it simply
  goes, dropping nothing until seeds from tall grass come with farming (since M1.8 tall grass
  drops itself, for beds).
- **Built (M1.8):** the `apple`, the first food: food items have a `fills` column in `items!`
  (`food(4)`), and leaves drop one a time in 20 when broken (`Drops::Chance(Item::Apple, 20)`
  in `blocks!`).
- **Built (M1.9b): farming** (`src/farming.rs`, `src/server/farming.rs`).
  - **Hoes and farmland:** `Hoe` is a tool kind, in each tier (`wooden_hoe` to `iron_hoe`, 2 of
    the head's material and 2 sticks at a crafting table), quicker at breaking nothing. Right
    click with one on dirt or grass with air above asks for an edit to `farmland`
    (`farming::tilled`), which the server makes only with a hoe in the slot named, wearing it a
    block's worth in survival; `RequestEdit` needed nothing new, as a till is told from a place
    by the block it's on. Farmland is a full cube with a furrowed top tile, broken like dirt
    and dropping dirt; a solid block placed on it packs it back into dirt. It doesn't dry out
    left unplanted, nor is it trampled: neither seemed worth its state yet.
  - **Seeds and wheat:** tall grass drops a `seeds` one time in eight beside itself
    (`Drops::Both`, with `Drops::Several` for counts, so a block can leave more than one stack).
    Items have a `plants` column, so seeds place `wheat_0` (`Item::places`), and a crop stands
    only on farmland (`BlockType::stands_on`, beside `needs_ground`), so breaking or packing the
    farmland pops it. Wheat's stages are eight blocks, `wheat_0` to `wheat_6` and `ripe_wheat`,
    declared a line each with a `crop` row helper: crosses like the other plants, but not built
    over (not replaceable), so a misplaced block can't wipe out a field. Unripe wheat drops its
    seed; ripe wheat drops a `wheat` and 1–3 seeds. Their tiles, untinted, from green shoots to
    bearded gold ears, and farmland's are drawn by `draw.py`.
  - **Growing (INV-4):** a stage is a block, so growing is an edit, saved and sent as any is.
    The server keeps each crop's planting time, in game days (`TimeOfDay`, so sleeping through
    the night grows crops too), in `Crops` beside the edit layer, as containers are kept, and a
    crop's stage is a function of it: each of the seven steps takes 0.6–1.4 times a seventh of
    2.5 days, by a hash of the seed, the crop's place and its planting time, divided by 1.5 if
    there's water within 4 blocks sideways of its farmland, at its level or one above
    (`farming::reaches`, `grown`). Dry wheat is ripe in about 2.5 days (25 minutes), watered
    in about 1.7. Water's judged as the crop grows, as if it had always been as it is, so
    water brought to a field hurries it on at once; a stage never goes back.
  - **Scheduled, and caught up:** each crop remembers when it's next due, and each chunk's
    field the soonest of its crops, so a frame looks at a number per field and only at crops
    that are due (`grow_crops`, at most 256 a frame). A due crop in a loaded chunk is brought
    to the stage its time says, which after a long time away may be several at once, as one
    `BlockChanged`; one in a chunk no one holds waits, so a field far off costs nothing until
    a player's near and then catches up exactly as if it had grown live (a test checks both
    ways and an unloaded field). Ripe crops, and crops whose block was changed, are forgotten.
    Clients see the changes like any other edit; a big field come back into reach sends its
    changes over a few frames.
  - **Food:** `bread`, 3 wheat at a crafting table, fills 5 (an apple 4). Sprites for hoes,
    seeds, wheat and bread are drawn by `items.py`.
  - **Creative:** the picker offers farmland, ripe wheat (to decorate with, standing still) and
    seeds, which grow; `PickBlock` carries an item rather than a block, sent by name as before.
    `PROTOCOL_VERSION` is 9.
  - **Later:** farmland drying out and trampling, more crops (carrots, potatoes) and saplings,
    bone-meal-like speed-ups, and middle-clicking a crop picking its seeds.

### 5.6 Structures

Structures make exploring worth it: villages, temples, ruins, mineshafts, dungeons, and a few huge
landmarks visible from far away (P1).

- **Placement** generalizes the tree system (INV-3): each structure type has a grid of cells,
  with at most one structure per cell, at a deterministic spot, if the cell's biome, slope and
  height suit it. A chunk asks which structures reach into it and stamps their blocks. Each
  structure type has a known maximum size, playing the role `TREE_REACH` plays for trees.
- **Templates are data** (INV-6): block layouts stored as files in `assets/structures/`, with
  markers for loot chests, spawners and variant parts. Big structures (villages) are assembled from
  pieces by rules, such as houses placed along paths.
- **Authoring in creative mode:** select a box and save it as a template with a command such as
  `/structure save <name>`. This is how most structures get made.
- **Terrain fit:** structures flatten or fill the ground under them as needed, and blend in at the
  edges.
- **Visible at distance (INV-5):** structures appear in the LODs like trees do, and big landmarks
  are tall enough to be seen from tens of kilometres. Lit windows at night are a goal.
- **Loot:** chests carry loot tables (data), giving good reasons to visit.
- **Built (M2.3): loot tables** (`src/loot.rs`). `TABLES` is every loot table (INV-6): a stable
  name, a range of rolls, and entries of an item, a range of counts and a weight. There's one
  for each structure planned, `ruin`, `watchtower`, `outpost`, `mineshaft`, `dungeon` and
  `temple`, better the deeper or further out it's found: food, coal and torches near the
  surface and the start, with a little metal and a tool now and then, and iron and iron tools
  deep down and far away. `LootTable::roll(seed, at, slots)` draws the rolls, each entry by
  weight and its count in its range, each stack into a random empty slot, from an RNG seeded
  from the world's seed and the chest's position alone (INV-2), so every player, machine and
  reload gets the same chest, and neighbouring chests differ.
- **Loot chests (M2.3)**, the contract structures keep. Nothing about loot is stored in the
  generated world (INV-1); a chest's loot comes to exist when it's first looked into.
  - **Generation** puts a `chest` block at each `loot` marker of a template (`LOOT`: the
    template's cell there becomes a chest as it's loaded, so full detail and LODs agree), and
    records nothing else. A structure's chests roll from the loot table with the structure's
    name; a test checks every structure with `loot` markers has one. M2.4's structures add a
    table to `TABLES` under their name as they add their file. If one template ever wants
    different chests, marker kinds such as `loot.<table>` can name tables per marker without
    changing the server's side.
  - **The server** (`server::containers::fill_loot`) treats a `chest` with no edit at its
    position (players' chests are edits) and no chest container as unopened loot. The first
    time anyone opens it or tries to break it, `loot::generated_table` finds the structure
    whose `loot` marker is there, through `RegionStore::structures_near` as chunks find theirs
    (its placement's remembered from generating the chunk, so this is a lookup), and its table
    is rolled from the seed and the chest's position into a new container. From then on it's an
    ordinary container, saved with its chunk's region like any other, so what's taken stays
    taken and nothing's rolled again; a chunk with only containers and no edits is saved too.
    Breaking one unopened leaves its loot. A generated chest no structure accounts for (a
    template changed under an old world) opens empty, and a chest a player places where one was
    broken is theirs, and empty.
  - **Proven on the watchtower:** its top floor's marker is a chest, which seed 3's tower
    nearest the spawn rolls the same from a fresh world as from one that's generated it, and
    which the server fills once on first opening and keeps through closing and reloading.
    Putting the chest there changed the golden checksum, which samples that structure's chunks.
- **First structures, proposed:** desert temple, forest ruin, glacier outpost, mountain
  watchtower, mineshaft, dungeon, then villages.
- New biomes come with the structures that need them, such as a jungle for jungle temples.
- **Built (M2.1): the system, with one structure** (`src/chunks/generate/structures/`).
  - **Templates** are TOML files in `assets/structures/`, one a type, read once into
    `STRUCTURES` from files built in with `include_str!`, so generation reads nothing from disk,
    on a server as on a client. A file names the type, its placement, and its template: a size,
    the layer standing at ground level (those under it are sunk), a foundation block, a palette
    of one character a block name (with its state since M2.4a, `log[axis=x]`, §4.1; turning a
    structure turns its blocks' facing and axis), markers by kind (`loot` points, chests since M2.3), and the layers as
    text grids, bottom first. `.` leaves the world as it is and `-` clears it to air, carving
    interiors out of the ground. Text over a binary format with a dump tool, since a template a
    few thousand blocks big reads and diffs as a picture of itself; M2.2's `/structure save`
    writes the same. Templates are refused at load if they don't match their size or are taller
    than `STRUCTURE_HEIGHT` (27, as tall as trees) over their floor, or sunk deeper than
    `STRUCTURE_DEPTH` (4). The watchtower was written by script (the `structures` skill).
  - **Placement** (INV-2, INV-3) is per type on a grid of square cells: a hash of the seed, the
    type's name and the cell decides whether the cell tries for one (`chance`), its spot in the
    cell and its rotation (0–270°). A hilltop type climbs from its spot to the top of its hill in
    12-block steps, giving up if it would leave its cell or come within an eighth of a cell of its
    edge, so those in cells side by side stay apart. It then stands if the ground suits it,
    judged from the noise alone (the same heights the regions hold), so every level of detail
    places the same ones: its biome, its floor's height over the sea, and the ground under its
    footprint rising and falling no more than `max_rise`, none of it under water. A chunk or LOD
    node asks `RegionStore::structures_near` for those whose footprints overlap it or the trees
    near it, from its own cell and those its footprint can reach from. Placements are remembered
    by cell (`Placements`), since deciding one takes up to a hundred or so noise samples (about
    150 µs) and every chunk and node near it asks; most cells are ruled out by their hash
    without waiting on the others.
  - **Terrain fit:** a structure's floor is the middle height of the ground under its footprint,
    so it cuts in as much as it stands over. In its footprint the ground above the floor is
    cleared down to it, with whatever grew on it, and the ground below filled up to its lowest
    layer with its foundation block; then its own blocks go in. Trees whose crowns would come
    within 2 blocks of its footprint don't grow (`Tree::overhangs`, the same at every level), so
    it stands in a small clearing. Caves under the footprint stay 5 blocks under its floor or the
    ground, whichever is lower, as under ground with no mouth, so they never carve its floor away
    and its columns' roofs are as they were. Edges aren't blended: on the gentle hilltops it
    needs, its walls meet the ground within a few blocks.
  - **Level of detail (INV-5):** cells up to two blocks wide take the structure by coverage: a
    cell is the commonest of its blocks where at least half the cell is them, or air where at
    least half is them and the air it clears, so level 0 is exactly full detail. Coarser cells,
    out to 32 blocks wide (about 8 km), draw it as boxes of its commonest block, one for each
    cell's height of it, each as wide as its blocks there, over a foundation box down to the
    ground the node draws, from the node holding its corner, as trees' crowns are drawn. They
    take its exact floor height, so it doesn't shift between levels; a coarse node's ground can
    bury its foot by a few blocks. `known_empty` counts `STRUCTURE_HEIGHT` over the ground. A
    7×7 watchtower is a few pixels wide at 1.5 km and a pixel at 8 km; landmarks meant to be
    seen across the map come with M2.7.
  - **The watchtower** (`watchtower.toml`, written by `.claude/skills/structures/watchtower.py`):
    7×7 and 20 layers, a cobblestone footing sunk a layer, cobblestone rising into stone brick,
    a doorway, arrow slits, a stair of single blocks climbing once round the inside to a floor
    under a crenellated parapet, and a corner fallen away from halfway up, with a `loot` marker
    on the top floor. One cell in 640 blocks square in forest, foothills, taiga and savanna (the
    last two since M2.6, which took them from the first two), on hilltops 12 or more blocks over
    the sea, with ground under it rising no more than 4: about 3 per 10 km²
    round seed 3's spawn, the nearest 220 m from it.
  - **Cost** (INV-7): a chunk or node with no structure near pays a lookup of a few cells by
    hash, 0.2 µs. Building one into a chunk it touches costs about 5 µs (2–4% of generating
    it), into a level-1 node about 35 µs (3% of meshing it), and its boxes 6 µs. With the
    bench's code in one codegen unit, so the layout of unrelated code holds still, generation,
    chunk meshing and LOD meshing are within 1% of before, with and without a structure in the
    benched area; in the default build, chunk meshing shifts by up to 5% from functions
    inlining differently into other codegen units (perf shows `vertex_light` no longer
    inlined), though nothing on that path changed. `lod_sim` shows no holes.
  - **Determinism:** the golden checksum (§4.4) now samples the chunk with the floor of the
    structure nearest the spawn and the chunk over it. Its other samples hash as they did before
    structures; it changed only by those two chunks.
  - Later: blending the ground at the edges, several types competing for a spot, structures
    assembled from pieces (villages), lit windows, and bigger landmarks drawn further.
- **Built (M2.2): authoring in creative** (`src/authoring.rs`, `src/server/structures.rs`,
  `src/client/authoring.rs`).
  - **Selecting:** `/pos1` and `/pos2` set the corners of a box at the block outlined, or else
    the block the player stands in, or at coordinates typed (`~` as for `/tp`), and say its size.
    Commands rather than a wand item, as every other authoring step is one and there's no item
    to keep in the hotbar. The selection (`Selection`, required by `Player`) is the server's, as
    it saves from it; a client from elsewhere is sent it as `SelectionChanged` (protocol 12) and
    draws it as the host does, an orange box, its corners outlined, and the floor of the
    structure placed in it as a fainter rectangle. It isn't saved with the world.
  - **Structure voids and markers** are blocks (`structure_void`, `loot_marker`) found in the
    creative picker, never generated nor dropped, walked and seen through, broken at a touch and,
    as the world draws neither, shown only in creative, as small boxes drawn from the edit layer
    round the player (`AuthoringPlugin`), so a structure looks as it will while it's built. A
    void is built over as air is. Which block stands for which marker is one table,
    `authoring::MARKERS`, so markers to come (spawners, M3) add a row.
  - **`/structure save <name> [floor]`** writes the box as a template in the M2.1 format to
    `structures/<name>.toml` in the world's directory, on the server, which has the world, and
    says where; it can be committed to `assets/structures/` as it is. Air is `-`, voids `.` and
    markers `.` with a marker at them; the palette keys each block by the first letter of its
    name that's free. The floor is the layer given, or else that of the structure placed into the
    box, or else its bottom; the foundation is the commonest block of its bottom layer; the
    placement is that of the structure of the same name if there is one, so a reworked one keeps
    it, or else a start to edit (`Placement::default`). The text is read back with the loader
    before it's written, so a save that wouldn't load, such as a floor deeper than
    `STRUCTURE_DEPTH`, is refused. Boxes are at most `MAX_SIZE`, 64 by 31 by 64: as tall as a
    template can be and four chunks across, room for any one building as villages are made of
    pieces, while placing one stays a burst of at most 127,000 edits. All of the box must be
    loaded.
  - **`/structure place <name> [0|90|180|270]`** builds the world's own template of that name,
    or else the built-in one, as edits (sent as any are, a container overwritten leaving what was
    in it) with its floor on the block outlined, centred as generation centres one, and selects
    it with its floor. Cells it leaves as they are become voids and markers marker blocks, so
    saving it again gives the template back: the watchtower round-trips exactly, turned or not.
    `/structure list` lists both kinds. Templates are read and written as the command runs, a few
    kilobytes as rarely as an operator types one, rather than on the I/O pool (INV-7).
  - All of these are operators' commands (`USAGES`). Generation is untouched: the new blocks
    never generate, and the golden checksum is as it was.
  - Later: copying and pasting a box, and undo, if building in the game asks for them.

**Style guide for structures (M2.4b on).** Every structure is reviewed in screenshots from
several angles, near and far, before it lands. It should have:
- *A silhouette:* varied heights, roofs that overhang, towers, chimneys, never a flat-topped box.
- *Depth:* recessed windows and doors, pillars and beams standing out from the walls, walls
  thicker at the base, steps up to entrances.
- *A palette:* a main material, a trim and an accent (e.g. a stone base, log frame and plank
  walls; sandstone with a darker banding), chosen to suit its biome.
- *Weathering:* random mossy and cracked blocks, collapsed corners, vines, partly buried or
  overgrown where it's old; variety from seeded variation, so no two look identical.
- *An inside:* furnished and lit, with its loot somewhere that makes sense.

### 5.7 World and biomes

- More biomes as content needs them: jungle, plains, swamp, savanna, taiga, badlands, and
  others. Each is a module under `src/chunks/generate/biome/` with its climate, blocks, trees and
  structures.
- **Landmarks:** rare continent-scale features (a huge peak, a volcano, a giant tree, a fallen
  tower) placed far apart and visible from very far away.
- A map: a top-down view of explored land, drawn from the same terrain sampling as `world_map`.

Built (M2.6): jungle, savanna and taiga, chosen from where `world_map --biomes` showed lowland
the climate gave no biome of its own: hot ground of middling humidity between desert and forest
(about a sixth of all lowland, the biggest gap), and cold, damp ground between the glaciers and
the forests, which had been bare grass or snow with a few pines. The jungle takes the hot end of
the forests, for M2.4's jungle temples. Swamp and plains are left for later: a swamp's pools
need the ground shaped low and wet, and plains were most of the gap savanna now fills.

- **Climate niches.** `Biome::niche` is how well a climate suits a biome from the climate alone,
  and its strength by default. Jungle is hot (0.62–0.72) and wet (0.6–0.7); savanna as hot as
  desert, from where the desert gives way at the same humidity (0.35–0.45, so the two always sum
  to one and there's no gap) to where forest begins (0.66–0.54); taiga from the glaciers' edge
  (0.1–0.18) up to where pines give way (0.42–0.34), anywhere not dry (over 0.3–0.4). Biomes are
  listed the more particular first: where two hold equally the earlier owns the column, so
  jungle and taiga take over from forest, and the climate biomes from foothills. On seeds 3 and
  42 savanna is 22% and 7% of land, jungle 5% and 11%, taiga 6% and 9%.
- **Blending.** Heights blend as before. Tree density now moves towards each biome's own by its
  strength, the last listed first, rather than each adding its own, so a jungle over a forest
  is the jungle's density, not both piled up; `tree_bound` still bounds it for LODs ruling out
  trees from the climate. Grass colour (`Biomes::grass_colour`) is the climate's, drawn towards
  each biome's own by its niche, so it's as smooth as the climate and the same at every LOD,
  sampled at mesh corners as before; leaves take after it. Plants blend the same way.
- **Jungle:** deep green, 0.85 trees a cell: six in ten are jungle giants (`TreeKind::Jungle`,
  trunks 15–20 logs and all 2 × 2, a wide flattened crown with lobes off branches from high up,
  topping out at 24 blocks), the rest broadleaves under them. Thick tall grass with ferns, few
  flowers.
- **Savanna:** golden grass, 0.06 trees a cell, all acacias (`TreeKind::Acacia`: a 5–8 log
  trunk with limbs reaching out to flat pads of leaves level with or over its flat crown). Long
  grass all over, and flower patches in full bloom.
- **Taiga:** dark, cool green, 0.6 pines a cell. Its colder half is under snow (the base
  terrain's snow over grass below 0.25), and taiga trees take root in snow up to 140 blocks
  over the sea (`Biome::roots`), so it's a snowy pine forest there; the far LODs' forest tint
  darkens snow under it as it does grass. Ferns and thin grass.
- **Blocks:** one, the fern, a plant tinted like tall grass with a tile drawn by the
  `block-textures` skill. Jungle and acacia trees use the log and leaves, tinted by biome.
- **LODs (INV-5):** a tree's kind comes from the biome owning its trunk's column, which every
  level samples from the noise at that spot, so every level grows the same trees, as tests
  check over each new biome; level 0 matches full detail there. Coarse cells colour the ground
  by the same grass colour, and far forest tint follows tree density, so jungles read dark
  green, savannas gold and taiga dark green over snow from kilometres off.
- **Map:** `map::Relief` colours grass by `Biomes::grass_colour`, under leaves by tree density,
  and draws snow where the grass is under it, so the in-game map and `world_map` both show the
  new biomes.
- **Structures** name biomes in their placement; a test checks every name is a biome's. The
  watchtower stands in taiga and savanna too, as they took ground it had in forest and
  foothills.
- **Cost** (INV-7): a column's biomes cost three more climate ramps, and plants sample the
  climate for half as many columns again (the most any biome covers rose from 0.4 to 0.6).
  Counted in instructions, as the machine was too loaded to time, the bench is 7% (seed 1) and
  5% (seed 3) dearer overall and its LOD meshing 14% and 8%, nearly all of it noise sampled for
  the trees savanna, taiga and jungle grow where there was bare grass before, and in their
  leaves; chunk generation and meshing together are about 2% dearer. `tree_bound` still rules out savanna's
  empty cells from the climate alone, as a test checks.
- **Determinism:** the golden checksum changed, as the spawn of seed 3 is now savanna and its
  cold sample column taiga; a fourth column samples a jungle.
- Later: swamp, plains, badlands, vines on jungle giants. Regional ores by biome came with
  M2.5 (§5.4), reading `Biomes::name` the way structures do.

Built (M2.9): M opens the map (`src/map/`), a `GameState` like the inventory's, which frees the
cursor and stops the player but not the clock; M or Escape closes it. The client reveals 64-block
tiles whose middles are within 256 blocks of the local player, the full detail distance at the
default settings, so the map records where you've been rather than all you can see from there.
The explored tiles are kept by the client, not the server: the map shows nothing the seed
doesn't, which every client has (§4.4), so there's nothing for the server to guard (INV-4), and
it needs no protocol or save format change. They're written as a sorted postcard list to
`map-<seed>.bin` in the world's directory, or under `servers/<address>/` in the data directory
for a player who joined, on the IO pool every 30 seconds and as the game quits. Each explored
tile is drawn as a 32-pixel image (2 blocks a pixel) and, zoomed out past 1 block a pixel, four
by four tiles share a 64-pixel one (4 blocks a pixel) with what's unexplored left clear; both are
rendered on tasks by `map::relief`, the shaded relief, sea, biome and grass-tint colouring moved
out of `world_map`, which now draws with it too, so a tile matches `world_map` pixel for pixel
(tested). A tile takes about 0.8 ms (3.3 ms for the coarse ones), eight at once while the map's
open and two while it's closed, keeping those round the player ready; at most 768 images are
kept, the least recently wanted let go. The screen shows the player as an arrow the way they
face, other players with their names, the world's spawn and the player's bed, a scale bar and
the coordinates under the cursor; the wheel zooms about the cursor through four levels and
dragging pans. Edits don't show: it's the land as generated. Not yet: a minimap, a joined
player's bed set after joining (its `SpawnBed` isn't sent to the client), and revealing further
from high places.

### 5.8 Mobs and entities

- Passive animals (food, materials) and hostile mobs at night and in dark caves.
- Needs an entity system on the server, pathfinding over voxels, simple models and animation, and
  replication (§4.4). This is the biggest single system in the plan, so it comes after the core.
- Villagers come with villages.
- **Built (M3.1):** the entity system and pathfinding, shown with sheep.
  - **Kinds are data (INV-6):** each a row of the `creatures!` table (`src/creatures/mod.rs`):
    its stable name, its width and height (the box collision and pathfinding take), health,
    walking and running speeds, its model (boxes in sRGB colours, facing -z, each a leg that
    swings from its top or not), what it drops (items and how many, chosen evenly), where it
    spawns (`Spawns`: on which blocks, under the sky or not, by day, night or always, in groups
    of how many, and the cap round a player), its behaviours in order, and whether it keeps out
    of water. Biomes aren't a column yet: grass stands in for where grazing animals live, and a
    biome list can join the row when new biomes land (M2.6).
  - **Brains:** the behaviours are `Flee` (from what hurt it, for 5 s, at its running speed),
    `FollowLure(item)` (a living player within 8 blocks holding it, to within 2) and `Wander`
    (standing about 2 to 8 s, then walking somewhere within 8). Each frame the first in the row
    with something to do wins, as `Mind`'s `Doing`: `Idle`, `Wander`, `Flee { from }` or
    `Follow(player)`. `Follow` is what M3.3's chasing needs, with another way of choosing whom.
  - **Bodies:** a creature walks with the player's own step, `player::physics::walk`, which
    takes a body's box and the speed it wants across, so it falls under gravity, collides with
    blocks, swims slower, and jumps a block: where its way climbs, or it's walked into a wall.
    In water it always swims up, so it floats with its middle at the surface. A fall hurts it
    as one hurts a player (`fall_damage`, a point a block beyond 3), from the highest it's been
    since it stood or swam. A hit knocks it back 6 blocks a second, and up if it's standing,
    dying away to a fifth in a quarter of a second.
  - **Pathfinding** (`creatures::path::find_path`, for M3.3 and M3.4 to use): A* over the cells
    a body's feet can be in, standing on a solid block under some of its footprint (a body
    `ceil(width)` rounded up to odd across, `ceil(height)` tall) with nothing solid in it, or
    in water if it swims. Steps go a block each way on the level or down as far as it drops (3,
    which hurts nothing), up one where there's headroom to jump, and diagonally only where both
    sides are open, so it never cuts a corner. Water costs 2 more blocks' walk, so a swimmer goes
    round where it can; a kind that keeps out of water never paths into it. It looks through
    1,500 cells at most, 24 blocks across or up and down from where it starts, and gives back the
    way to wherever it got nearest if it can't arrive. Each search reads a `Blocks`, the chunks
    round it shared from `World` as `Arc`s and unpacked into a grid as it first reads them, so it
    runs on the compute pool (INV-7); taking the copy costs the main thread 2 µs. A creature
    walks its way's cells' middles in turn, finds another way if it gets no nearer for 1.5 s,
    and asks for at most one every half a second; following finds another as the player moves 2
    cells from where it was going.
  - **Spawning:** every 2 s (`SpawnRate`, none to turn it off, as tests do), round each player,
    a kind under its cap within 64 blocks across tries a column 24 to 48 blocks off, never within
    a radian of where they look, and appears in a group on its surface if the row allows it:
    the highest solid block within 16 of their feet, one of its blocks, with room for its body,
    open to the sky if it must be, by the right time of day. Sheep come in twos to fours, on
    grass by day, eight at most round a player.
  - **Sleeping and saving:** a creature more than 128 blocks from every player, or whose chunk
    the world no longer holds, goes to sleep: it's taken out of the world into `Dormant`, by
    region, and saved (§4.3), and woken where it was once a player comes within 96 and its
    ground is loaded. So creatures are simulated only near players and in chunks the server
    holds, cost nothing far off, and persist, as Minecraft's animals do; a sheep penned in stays
    penned. Nothing despawns by itself yet: M3.3's hostile mobs will want a row saying they go
    when nobody's near, and aren't saved.
  - **Hitting:** a left click on a creature in reach, nearer than the block behind it, hits it
    (§4.4) for what's held does, `Item::damage`: a hand or anything not a tool 1, a tool 1 more
    than its tier (5 for iron), wearing it a use. A creature at no health dies, dropping its
    drops scattered as a dead player's are, and despawns. Sheep run when they're hit.
  - **Drawing** (`client::creatures`, `DrawCreaturesPlugin`): each part's box a mesh made once
    per kind, with a material for each colour and a redder one it flashes to for 0.3 s when
    `Flinches` goes up; the legs swing with the distance walked, as players' do. Health isn't
    shown. Targeting is a ray through each seen creature's box (`collision::ray_distance`).
  - **Sheep:** 0.9 wide and 1.3 tall, 8 health, walking 1.4 blocks a second and running 3.4,
    following wheat, a body of wool on four legs with a face, a cap of wool and eyes. They drop 1
    or 2 mutton (food, 2), which a furnace cooks into cooked mutton (6). Wool waits for M3.2.
  - **Cost:** 100 sheep wandering round a player on seed 3's spawn cost the server's creature
    systems 0.16 ms a frame on the main thread (p99 0.31), against 0.04 ms for the same schedule
    with none, which is the schedule running empty systems; a world with no creatures near anyone
    pays a check of an empty map and empty queries. A way 20 blocks across that terrain takes
    0.04 ms (median; mean 0.13, the slowest, which look through all 1,500 cells and fail, 1 ms),
    on the compute pool. (Measured on a machine with a load average of 20.)
  - **Tests:** `creatures::path` (straight across, round a wall by its gap, up steps a block at a
    time but not two, down only as far as it drops, headroom and width, water, limits and the
    nearest way); `server::creatures` on a field (a fall that lands and hurts only past 3 blocks,
    following wheat up a step, floating, a hit that hurts once, knocks back and frightens, out of
    reach, a kill that drops mutton and wears the axe, sleeping and waking, spawning on grass out
    of sight up to the cap, and none at night or on sand); saving and restoring them; the
    multiplayer test (§4.4); `/summon`'s parsing; and `ray_distance`.
  - **Later:** hostile mobs, despawning and weapons (M3.3), villagers (M3.4); a creature's head
    turning to look, grazing, and a death that tips over rather than vanishing; creatures pushing
    each other and players apart.
- **Built (M3.2): passive animals,** shearing and breeding (`src/server/creatures/tending.rs`).
  - **Four kinds** in `creatures!`: sheep (0.9 × 1.3, 8 health), cows (0.9 × 1.4, 10, walking
    1.2 and running 3.0), pigs (0.8 × 0.9, 10) and chickens (0.45 × 0.75, 4). Each has its own
    box model in the sheep's style: a cow brown with pale patches, horns, a pink muzzle and an
    udder; a pig pink and low with a snout and ears; a chicken white on thin orange legs with
    wings, a tail, a beak, a comb and a wattle. Drops: sheep 1–2 mutton, cows 1–3 beef and 0–2
    leather (a range from 0 can drop none), pigs 1–3 pork, chickens a chicken and 0–2 feathers.
    Leather and feathers are only materials until armour, books and arrows want them.
  - **New columns:** `food`, the items a kind follows and is fed to breed (`FollowLure` reads
    it): wheat for sheep and cows, apples or wheat for pigs (there are no carrots), seeds for
    chickens; `fleece`, what shearing gives (sheep: 1–3 `wool`), parts marked `fleece` in the
    model being hidden once shorn; and `flutters`, a chicken's: it never falls faster than 1.5
    blocks a second (its speed held so the step's gravity takes it no further) and no fall hurts
    it.
  - **Spawning:** `Spawns` gains `biomes`, by name as structures name them (`base` for the base
    terrain), checked against the biome owning the column tried (`spawning::biome_at`, a noise
    sample). Every kind on grass, under the sky, by day: sheep in base grassland, foothills,
    savanna and taiga, in twos to fours with a cap of 8; cows in base, foothills, savanna and
    forest, twos to fours; pigs in forest, jungle and taiga, ones to threes; chickens everywhere
    but deserts and glaciers, twos to fours; each of these capped at 6, every kind counted on its
    own. None in deserts or on glaciers (which have no grass anyway).
  - **Sounds:** each kind calls and cries out through M-A3's `CreatureSound`: a cow's moo, a
    pig's grunts and a hen's clucks, synthesised by the sounds skill's `synth.py` (`moo`, `oink`,
    `cluck`, four takes each) beside the sheep's bleat.
  - **Life:** `Life` on each creature counts down, in seconds, how long it has to grow up, for its
    fleece to grow back, to be in love, and to rest after breeding, and `grow` keeps
    `Condition { young, shorn, in_love }`, replicated, in step. Saved creatures keep the first two
    (`SavedCreature::grows_up_in`, `regrows_in`), so the creatures format is 2, and format 1 files
    read as grown and unshorn. Love and rest aren't saved, being soon over.
  - **Using them:** right click on a creature sends `UseCreature`, which the server checks for
    reach as hits are (and the client doesn't eat while it's looking at one). Shears (a tool of
    kind `Shears` and no tier, so it hits as a hand does, 2 iron ingots at a table, quick at the
    wool block) on a grown, unshorn sheep drop 1–3 wool and wear them a use; the fleece grows back
    in 4–6 minutes. A grown animal's food, while it isn't in love or resting, puts it in love for
    30 s, taking one in survival.
  - **Breeding:** `Breed`, after `Flee` in an animal's behaviours, sends one in love to the
    nearest of its kind in love within 8 blocks (`Doing::Mate`), a cell from it; once two that
    want each other are within 1.6 blocks across, `breed` makes a young one between them and rests
    both for 5 minutes. A young one is half the size, body and model (`YOUNG_SCALE`), takes no
    food, can't be shorn, drops nothing, and grows up in 4 minutes. **Caps:** a kind with 24 of
    it within 16 blocks takes no food and breeds no more, which lets a farm grow and keeps one
    from growing without end; bred animals count towards spawning's caps, so a farm keeps wild
    ones from appearing round it.
  - **Wool:** an unshorn sheep killed drops a wool too. 4 make a `wool_block` by hand (soft, 1.2 s
    by hand, quickest with shears), and beds now take 3 wool and 3 planks. Grass beds are gone
    rather than kept beside them: one recipe per thing keeps the list short, wool beds give sheep
    a use from the first day, and a first night's bed is still three sheep away, needing no iron.
  - **Drawing:** the client hides fleece once shorn, draws the young at half size, and puts a
    small red heart over one in love. Targeting takes each creature's body as its condition has
    it. Sprites for the new items are drawn by `items.py`, the wool block's tile by `draw.py`.
  - `PROTOCOL_VERSION` is 16.
  - **Tests** (`server::creatures`): each kind appearing up to its own cap in base grassland and
    in forest, or not at all if it doesn't live there, and none at night, on sand or in deserts; cows' beef and sometimes-none leather, a sheep's wool unless shorn and a young one's
    nothing; shearing once, regrowing and shearing again, and not lambs or cows; two cows fed
    finding each other and making a calf, resting and refusing food, the calf refusing food and
    growing up, and breeding again once rested; a crowd taking no food; a chicken fluttering down
    10 blocks over 6 s unhurt. Also cooking each meat, the wool and shears recipes, saving a lamb
    and a shorn sheep and reading format 1, and shearing seen by a joining client.
  - **Later:** eggs, dyed wool, babies following their mothers, grazing that eats grass (which
    would want grass to spread back first).

### 5.9 Distribution

- Release builds for Windows, macOS and Linux, published on GitHub, that run without installing
  Rust. A dedicated server binary alongside.
- CI that builds and tests every PR (there is none yet).

Built: `.github/workflows/release.yml` builds `--release` on every `v*` tag for Linux x86_64,
Windows x86_64 and macOS Apple Silicon (Intel macOS is skipped), packs each as
`rustcraft-<version>-<target>/` holding the binary, `assets/`, the README and both licences
(`.tar.gz`, or `.zip` on Windows), and attaches them to a GitHub release. Run by hand, it uploads
the archives as workflow artifacts instead. The binary finds `assets/` next to itself
(`settings::asset_path`, the same rule as Bevy's `AssetPlugin`), so double-clicking works, and
Windows release builds open no console, so their logs go nowhere. macOS builds are not notarised.
The dedicated server (§4.4) is built beside it for each platform, with `--no-default-features` so
it needs no GPU or display, and packed alone as `rustcraft-server-<version>-<target>/` with the
README and licences but no assets, which it doesn't read.

### 5.10 Sound

- Footsteps by what's walked on, digging and building by material, the player's own noises,
  other players heard where they are, ambience by place and time of day, and a little music,
  rarely. Calm and grounded rather than busy, in the spirit of packs like Enhanced Audio, but
  every sound made for rustcraft (INV-8, D-10). Only the client plays any.
- **Built (M-A3):** `src/client/sound/`, from `SoundPlugin` in the game's App.
  - **Audio:** Bevy's `bevy_audio` and `vorbis` (lewton) features, in the `client` feature, so
    the dedicated server links no audio crate, which CI checks with `cargo tree`. With no audio
    device Bevy plays nothing and the game runs on.
  - **Sounds:** all synthesised by the sounds skill's `synth.py` (`.claude/skills/sounds/`) into
    `assets/sounds/`: 110 Ogg Vorbis files, 3.5 MB. Footsteps and breaking for nine surfaces
    (grass, dirt, sand, gravel, stone, wood, snow, leaves, ice), each four takes, and four of
    swimming, splashing, picking up, eating, being hurt and a sheep-like bleat; dying, a chest
    opening and closing and a click; seven ambience loops and three pieces of music. `Surface::of`
    names a few blocks and goes by the breaking tool for the rest (pickaxe stone, axe wood,
    shovel dirt, plants leaves), so a new block sounds right unasked. `analyse.py` checks levels,
    clipping and loop seams and draws spectrograms, as they can't be listened to here.
  - **Playing:** `PlaySound { kind, at }` plays a `SoundKind`, a random take at a random pitch in
    its range. With `at` it's Bevy's spatial audio from there, which pans it and falls off as the
    square of the distance from full within 6 blocks; beyond 32 it isn't played. With none it's
    the player's own. At most 24 one-off sounds play at once; more are dropped. Bevy loads the
    files on its I/O threads and decodes them on the audio thread (INV-7).
  - **What plays:** footsteps every 1.7 blocks walked on the block underfoot (`Gait`), none
    flying, a step pushing off to jump and a heavier one landing from over 0.6 blocks; strokes
    swimming and a splash falling in; blows every 0.25 seconds while breaking, then the break,
    and a lighter, higher break placing; chewing while eating; hurt and dying; picking up; chests
    opening and closing; a click on every button. Other players' footsteps are heard from where
    they're drawn, and their edits where they are: on a joiner from the server's `BlockChanged`
    before it's put in the world, on the host from the joiners' requests before its server makes
    them, and this player's own from its requests.
  - **Ambience:** loops of wind, high wind, the sea, birds, crickets, a cave (a hum and drips
    in a reverberant room) and being under water, faded over 2.5 seconds to what's wanted, which
    is looked at twice a second from the eyes (`mix`, about 6 µs): under water only that, in a cave
    (under ground and 3 blocks below the generated surface) only it, and otherwise wind rising
    with height, the high wind on peaks and in the cold, the sea where land round about is under
    sea level, birds by day where it's mild, more among trees, and crickets by night, both thinning
    up the mountains. Biomes come in by climate and trees, so new ones need nothing here. Loops
    faded out are paused.
  - **Music:** the first piece 1½ to 4 minutes in, then 8 to 18 minutes of quiet between.
  - **Volume:** master, effects, ambience and music, from − and + rows in the pause menu, kept in
    `volume.toml` in the data directory beside `worlds/`, written off the main thread. **Since
    M-U1** they're sliders in Settings' Audio tab, kept with the player's other settings
    (`Preferences::audio`), which take them from `volume.toml` until they've been saved there;
    `Volumes` follows them as `Bindings` does. Every button's press clicks, as before.
  - **Creatures** (M3.1) are heard where they're drawn: footsteps as players' are, a call now and
    then (every 25 seconds or so each), and a higher, sharper one as `Flinches` counts a hit;
    sheep bleat. Each goes through `CreatureSound { kind, hurt, at }`, the hook for whatever
    else wants a creature heard, and `SoundKind::sound` names each `CreatureKind`'s calls, so a
    new kind (M3.2) won't build until it's given some.
  - **Later:** furnaces crackling while lit, which the client only knows of the one open;
    other players' blows while breaking, hurt and eating, which they aren't told of; creatures
    dying; occlusion, so a cave next door is quieter through rock; rain (M3.5).

---

## 6. Roadmap

Each milestone is a PR-sized piece of work with a test of when it's done. Do them in order within
a phase unless they're independent; mark them done here in the PR that finishes them.

Status: ☐ not started, ◐ in progress, ☑ done.

### Phase 0: foundations

Done when: **two players on one world see each other and each other's edits, and the world saves
and loads.**

- ☑ **M0.1 Block registry.** Declare every block once, in one table with stable names (§4.1,
  D-9). No change in what's drawn. Tests cover lookup by name and generation output
  staying identical.
- ☑ **M0.2 Breaking and placing.** Raycast, outline, break and place in creative style, with
  remeshing and relighting of the edited chunk and its neighbours (§5.1). The edit layer holds the
  changes (§4.2).
- ☑ **M0.3 Saving and loading.** World directory, edits and player position saved and loaded
  (§4.3). Edits survive a restart.
- ☑ **M0.4 Game modes.** Survival and creative, `/gamemode`, creative block picker (§4.5).
- ☑ **M0.5 Client/server split.** Integrated server in-process; game state flows through it
  (§4.4). Singleplayer plays the same as before. In steps, each playable with tests green:
  - ☑ **M0.5a Game state off the camera.** A `PlayerLook { yaw, pitch }` on the player that the
    camera follows; movement and saving read it; the movement step as a pure function; the clock
    split from the sun and moon. A headless App runs saving, game modes and the clock.
  - ☑ **M0.5b Replicon and edits through it.** `bevy_replicon` with no transport, `protocol.rs`,
    `RequestEdit` / `BlockChanged` / `EditRejected`, the server validating edits.
  - ☑ **M0.5c Players owned by the server.** Spawned from saves by the server; the client puts
    the camera on its `LocalPlayer`.
  - ☑ **M0.5d Commands and game modes on the server.**
  - ☑ **M0.5e Time on the server**, with `TimeSync`.
  - ☑ **M0.5f Server chunk residency**, headless, with shared holders in `World`.
- ☑ **M0.6 Networked multiplayer.** Join by address, determinism handshake, players and edits
  replicated, simple player models, chat (§4.4). In steps:
  - ☑ **M0.6a Transport.** `bevy_replicon_renet2`, hosting and joining (`RC_HOST`, `RC_JOIN`),
    two-App tests over the memory transport. `EditAllowances` forgets a client when it
    disconnects.
  - ☑ **M0.6b Determinism handshake**, with golden checksums and a CI matrix of Windows, macOS
    and Linux (D-3).
  - ☑ **M0.6c Edit sync**: `ChunkEdits` on joining and moving, `BlockChanged` by area, client
    prediction with rollback.
  - ☑ **M0.6d Other players**: names, interpolated poses, simple models, plausibility checks with
    `Teleport` corrections, a save per player.
  - ☑ **M0.6e Chat and operators**, an `ops` list in `world.toml` with the host as operator,
    `/op`, `/deop`, `/list` and `/msg`.
- ☑ **M0.7 CI.** Build, test, clippy and fmt on every PR.

### Phase 1: the survival core

Done when: **you can start with nothing and make iron tools.**

- ☑ **M1.1 Hotbar, inventory and items.** Data-driven items, stacks, dropped items (§5.2).
- ☑ **M1.2 Survival breaking.** Break time from hardness and tool (§5.1). Drops came with M1.1;
  M1.2 makes some need the right tool.
- ☑ **M1.3 Caves.** 3D carving underground, cave mouths, buried-chunk skipping updated (§5.4).
- ☑ **M1.4 Ores.** Coal, copper, iron by depth, in veins (§5.4).
- ☑ **M1.5 Crafting.** Recipes as data, crafting by hand and at a crafting table; wood, stone,
  copper and iron tools (§5.3).
- ☑ **M1.6 Furnace.** Smelting ores, with containers (§5.2).
- ☑ **M1.7 Block light.** Torches, light across chunk borders, updated on edits (§5.5).
- ☑ **M1.8 Health and hunger.** Fall damage, drowning, death and respawn, beds, the food bar
  (§5.5).
- ☑ **M1.9 Plants and farming** (§5.5a).
  - ☑ **M1.9a Plant blocks.** The cross render style, tall grass and flowers in the world.
  - ☑ **M1.9b Farming.** Hoes, farmland, wheat that grows, bread and apples.

### Phase 2: exploration

Done when: **there's something worth walking 5 km to see, and you can see it from where you
start.**

- ☑ **M2.1 Structure system.** Placement, templates, terrain fit, LODs (§5.6).
- ☑ **M2.2 Structure authoring.** `/structure save` and loading in creative mode (§5.6).
- ☑ **M2.3 Chests and loot tables.** Chests as containers, loot tables rolled from the seed for
  chests the world generates (§5.2, §5.6).
- ☑ **M2.4a Building blocks.** What structures and players need to build things that look good:
  - **Block state:** blocks that face a direction or have a small state (facing, top/bottom half,
    open/closed, axis), stored compactly in `ChunkData`'s runs, saved, synced and meshed, placed
    facing the player. Logs placed sideways and a furnace with a front come with it.
  - **New shapes,** each a render style: stairs, slabs, fences and walls (joining their
    neighbours), doors and trapdoors (open and close), glass and glass panes, ladders (climbable),
    lanterns (hanging or standing, giving light), and a few decorative ones (bookshelves, barrels).
  - **Weathered and coloured variants:** mossy cobblestone, cracked and mossy stone bricks, vines
    (climbable, growing down walls), polished stone, coloured terracotta, darker and lighter
    planks; textures by script (INV-8).
  - Collision, raycasts, light (glass and panes let light through), LODs (shapes drawn as their
    nearest cube far away) and the creative picker all know the shapes.
  - Built: §4.1 (state, shapes, light, LODs, cost), §4.3 (format 5), §4.4 (protocol 15), §5.1
    (placing, doors, climbing, stepping up) and §5.3 (recipes).
- ☐ **M2.4b First structures,** to the style guide in the `structures` skill (§5.6): desert
  temple, jungle temple, forest ruin, glacier outpost, mineshaft, dungeon, alongside the
  watchtower, each reworked with the new blocks (§5.6).
- ☑ **M2.5 Regional ores and higher tiers.** (§5.3, §5.4)
- ☑ **M2.6 New biomes.** Starting with the ones structures need (§5.7).
- ☐ **M2.7 Landmarks.** (§5.7)
- ☑ **M2.8 Edits in LODs.** Players' builds seen from far away (§4.2, §4.4).
  - ☐ Follow-up: draw builds as boxes in coarse LODs, as structures are, so towers and houses
    narrower than half a cell stay visible beyond ~1 km (§4.2, Built M2.8).
  - ☐ Follow-up: edits clear what coarse nodes draw as boxes, so a demolished structure or a
    felled forest disappears at a distance too.
  - ☐ Follow-up: resend `EditReach` as the drawn distance grows when flying high, and a batch-edit
    path for big pastes (`LodEdits::set` bottom-up).
- ☑ **M2.9 Map.** (§5.7)

### Phase 3: life

Done when: **the world feels inhabited, and night is dangerous.**

- ☑ **M3.1 Entity system and pathfinding.** (§5.8)
- ☑ **M3.2 Passive animals.** Sheep, cows, pigs and chickens, shearing, wool and breeding
  (§5.8).
- ☐ **M3.3 Hostile mobs and combat.**
- ☐ **M3.4 Villages and villagers.**
- ☐ **M3.5 Weather.**

### Throughout

- ☑ **M-R1 Release builds** for Windows, macOS and Linux, and a dedicated server (§5.9, §4.4).
  - ☐ Follow-up: make `TimeOfDay` an `f64` of days, with `TimeSync`, `world.toml` and crops'
    planting times, so a client's clock runs smoothly however many days have passed, as the
    server's now does (§4.4, Built M-R1).
- ☐ **M-A1 Art pass, 32px.** Block and item tiles redrawn at 32×32 in a richer, natural style —
  inspired by the look of packs like Patrix (soft, detailed, naturally lit surfaces), never copied
  (INV-8) — by our own drawing scripts, with the atlas, shaders, LOD colours and map relief following.
  16px stays available as a setting if it's cheap to keep.
- ☐ **M-A2 Living creatures.** Creatures and players feel alive: idle breathing, blinking, heads
  that turn to look, smooth limb swing and easing — inspired by packs like Fresh Animations,
  built on our own box models (INV-8). With or after M3.2.
- ☑ **M-A3 Sound.** Bevy's audio switched on, with footsteps by surface, breaking and placing,
  ambience (wind, water, birds by day, cave drips and echo underground), creature sounds and a
  little music — inspired by packs like Enhanced Audio, from CC0 sources or made by our own
  scripts (INV-8), credited in `assets/CREDITS.md`. A volume setting; the dedicated server stays
  silent and audio-free. Built: all synthesised, §5.10.
- ☑ **M-U1 Menus.** The game opens on a title screen instead of straight into a world, and
  every setting lives in menus rather than environment variables:
  - **Title screen:** Singleplayer, Multiplayer, Settings, Quit, over a slowly panning view of a
    world in the distance (the horizon is the game's best feature, P1).
  - **Worlds:** a list of saved worlds (name, seed, game mode, last played, a small map preview
    from M2.9 if cheap); create a world (name, seed or random, game mode), play, rename,
    duplicate, delete with a confirmation. Replaces `RC_WORLD` and `RC_SEED` for players
    (they stay for development).
  - **Multiplayer:** a saved list of servers (name, address) with a ping and player count where
    the protocol allows; join by address; your player name (replacing `RC_NAME`). Replaces
    `RC_JOIN`.
  - **Settings**, saved per player, applied live where possible:
    - *Video:* render distance and full-detail distance (`settings.toml`'s renderer options),
      field of view, fullscreen/windowed, vsync and a frame cap, UI scale, shadows, clouds, and a
      low/medium/high preset.
    - *Controls:* mouse sensitivity and inversion, and rebindable keys for every action.
    - *Audio:* the volumes from M-A3.
    - *Accessibility:* view bobbing and hurt tilt off, larger text, a crosshair choice.
  - **Pause menu:** Back to Game, Settings, Open to Friends (with the address to share and who's
    joined), Save and Quit to Title.
  - **Death and loading screens** in the same style: a loading screen with progress while the
    world around the spawn generates, instead of hanging in the air.
  - Consistent look across all of them (one set of shared widgets: buttons, sliders, toggles,
    text fields, lists, tabs), keyboard and mouse navigation, and Escape always going back.
  - **Built (M-U1):**
    - **Widgets** (`src/menu/widgets.rs`) are Bevy 0.20's headless `bevy_ui_widgets` styled as
      the pause menu was: buttons, choices (a button cycling "Name: Option", for switches and
      short lists), sliders (`Slider`, rounded to their step, showing "Name: value"), text fields
      (`TextInput`, so a cursor, selection, typing, paste and a filter, selecting all on focus),
      scrolling lists (`ListBox`; arrow keys select, Enter or a double click is `RowChosen`),
      tabs as buttons, a `scroller` (a column that scrolls past a height, with the wheel or the
      arrow and Page keys while hovered or focused; the inventory's crafting list is in one, as
      a crafting table's recipes overflow short screens), and confirmations and notices as
      pages. Tab, Shift-Tab and Up/Down move
      the focus (`TabNavigationPlugin` and `move_focus_with_arrows`), Enter and Space press, and
      the focus lights up only once the keyboard's used. Text is sized in rem, so Larger Text is
      `RemSize`; UI Scale is `UiScale`. All of Fira Mono (OFL, `assets/fonts/`, 170 KB, built
      in) takes the place of Bevy's subset of it as the default font, which lacked ° and ×.
    - **Pages** (`src/menu/`) are a stack in `Menu`, the top drawn afresh as it changes, each by
      a system of its own (`run_system_cached_with`). Escape and every Back button run `back`:
      a page down, out of the pause menu into the game, out of the loading screen to the title,
      never off the title screen, and not while a key's being bound, which Escape cancels.
      `GameState` gained `Title` (the default) and `Loading`; UI marked `InWorld` (hotbar, bars,
      crosshair, command bar) is hidden outside a world.
    - **Title screen** (`menu/title.rs`): the scenery is a world of its own, `World::joined` with
      seed 7, drawn by the real terrain renderer to 384 chunks with 4 at full detail (or the
      player's own, if nearer), seen from 90 blocks over its spawn, turning 0.015 rad/s. It
      loads in a few seconds and costs what a near render distance does; the camera hangs from a
      `Viewpoint` entity, as `gather_chunks` wants a parented camera, and terrain waits for one.
    - **Worlds** (`src/save/worlds.rs`): a world's name is its directory's (characters no
      platform refuses, up to 32); listed with `world.toml`'s seed and game mode and its
      modified time as last played, or why it can't be read. `world.toml` gained
      `new_player_mode`, the mode chosen as it's made, written only when it isn't survival, so
      the format stays 4. No map preview: the relief would be cheap, but a list of worlds
      reads well without it, so it's left for later.
    - **Sessions** (`src/session.rs`): `Session::Singleplayer` or `Session::Join` started from the
      menus (or `Session::from_env`, for `RC_WORLD`, `RC_JOIN` and `RC_HOST`, which start in
      `Loading`, skipping the title) opens the world in `OnEnter(Loading)`. The loading screen
      plays once the chunks 2 across, 2 down and 1 up round the player are drawn
      (`ChunkLoader::is_drawn`), saying meanwhile whether it's joining or checking the
      server's world. `open_world` runs in `PreUpdate` whenever there's a `WorldToOpen`, and a
      world that can't be opened is `WorldUnopened`, shown as a notice (the dedicated server
      exits on it). `LeftServer` carries why a client's left its server (unreachable, another
      version, another world, the connection lost), which ends the session with a notice.
      Ping and player counts aren't shown: netcode can't answer without joining, so a server's
      reachability shows as it's joined.
    - **Leaving** is the `LeaveWorld` schedule, in `LeaveWorldSystems` order: `Disconnect` (the
      server sees off its joiners, keeping their states, and stops hosting; a client drops its
      `RenetClient`, so the server sees it go at once; the camera's taken down from the
      player), `Save` (the last save, on the main thread, as on quit) and `Clear` (each plugin
      puts its own away: the server its players, drops and resources, the client its players,
      replicated entities, `World` and its edit state, the terrain its chunks, groups and LOD
      nodes with fresh loaders at the current settings, the map its file, tiles and view, the
      command bar its lines). The title screen's scenery is left the same way. Save and Quit to
      Title runs `session::leave`. `is_authority` now also needs a `World` served here
      (`World::is_served_here`), so the server does nothing in the menus, nor in a joined
      world as its connection drops. Edits waiting for a joined client's world moved from
      `Local`s into `WaitingEdits`, which leaving clears.
    - **Settings** (`src/preferences.rs`) are `Preferences`, kept in `settings.toml` under the
      data directory over `assets/settings.toml`'s distances: read by merging the file's table
      over the defaults, so a missing or unreadable value keeps its default and an unknown one
      is kept, and written a second after they stop changing. Applied live: FOV, fullscreen,
      vsync, a frame cap (sleeping out the frame in `Update`), UI scale, larger text, shadows,
      clouds (their tiles hidden; their shadows on the land stay), sensitivity, inversion, hurt
      tilt, the crosshair (cross, dot or none, new) and key bindings; render and full-detail
      distance redraw the terrain as Settings closes (`redraw_for_settings`), or wait for the
      next world on the title screen. There's no view bobbing to turn off. Presets set the
      distances, shadows and clouds. The player's name is `Preferences::name`, or `RC_NAME` and
      the login name when it's empty.
    - **Keys** (`src/controls.rs`): every action is a row of `actions!` with its name, label and
      default; `Bindings` holds a key or mouse button for each, saved by name, and `Controls`
      reads them, used by movement, interaction, eating, the hotbar, the inventory, the map,
      the command bar, the clock and the debug overlay. Escape, Enter and the text keys stay
      fixed.
    - **Pause and death:** the pause menu's Open to Friends shows the address and who's joined
      (`Hosting`), and joined clients get Disconnect in place of Save and Quit. The death screen
      has Respawn and Quit to Title.
    - Tests: the widgets (text fields' filter and length, sliders, list selection, tabs, Escape
      and Back), rebinding a key, settings round trips and live application, presets, the
      worlds library on a temp dir, the server list, opening, quitting to the title and opening
      another world in one App with the entity count back where it was and the first world
      saved and reopened where it was left (`tests/session.rs`), unopenable worlds and
      unreachable servers ending in notices, the environment variables skipping the title, and
      a joined client and a host leaving their worlds (`tests/multiplayer.rs`).
    - **Known gaps:** a server list with ping and players needs a query the protocol doesn't
      have; render distance changes in a world redraw everything at once; the death screen's
      Quit to Title saves the dead player as dead, to respawn on return.

---

## 7. Decisions

### Open

None right now.

### Decided

- **D-10 Other packs are inspiration, never material.** Minecraft resource packs the project
  admires (Patrix 32x, Fresh Animations, Enhanced Audio) are All Rights Reserved or forbid
  redistribution and use outside resource packs, so no file, texture, model, animation or sound
  from them, edited or not, goes into rustcraft (INV-8). Their look and feel guide M-A1 to M-A3.
  Decided 2026-10-18.

- **D-3 Same-version determinism is enough, and CI guards it.** On 2026-10-18 the golden
  checksum test (§4.4, M0.6b) passed on `ubuntu-latest`, `windows-latest` and `macos-latest`
  alike, so generation is byte-identical across the three platforms at the same game version, and
  no deterministic-maths rewrite of the noise is needed. CI's `determinism` job runs it on every
  push: a platform that ever disagrees fails there, and the join handshake refuses the mismatch
  rather than letting players see different worlds. Decided 2026-10-18.

- **D-4 Crafting is ingredient lists at stations** (§5.3), not Minecraft's shaped grid: a recipe
  is a list of items and counts making an output, by hand, at a crafting table, or smelted at a
  furnace (M1.6). It's simpler to read and to use, as a recipe shows everything it takes and
  needs no pattern learnt, and it's less of a copy. Decided 2026-10-18.

- **D-5 Hunger is in, kept simple** (§5.5): a food bar, eating to refill it, healing when it's
  full enough and hurting when it's empty. Farming joins the plan as M1.9 to feed it.
  Decided 2026-10-18.

- **D-1 Networking: `bevy_replicon` (=0.45.2), with `bevy_replicon_renet2` (=1.0.0) as the
  transport from M0.6, and `bevy_replicon_quinnet` as the fallback.** On 2026-10-17 replicon was
  the only replication crate with a release for Bevy 0.20; its listen-server mode is the integrated
  server; it encodes with postcard, like saves (D-6); and its transports swap cheaply. `lightyear`
  had no Bevy 0.20 release, and its rollback prediction isn't needed while movement is decided by
  clients (§4.4); revisit it if mobs need predicted entities. Versions are pinned exactly and
  updated deliberately with each Bevy release. Decided 2026-10-17.

- **D-2 Multiplayer architecture:** client/server with an integrated server, terrain generated by
  clients from the seed, server-authoritative state (§4.4). Decided 2026-10-17.
- **D-6 Save format encoding:** postcard (with serde) for edit region files, TOML for
  `world.toml` (§4.3). Postcard's wire format is specified and stable since 1.0, it's compact
  (variable-length integers, so most edits take a few bytes), and it's small with no default
  features. bincode was the alternative, but its maintenance has ended. Decided 2026-10-17.
- **D-7 Direction:** survival and exploration first, with a creative mode; mining-driven
  progression; multiplayer; structures to find; an open, easy-to-play hobby project (§1). Decided
  2026-10-17.
- **D-8 Blocks are added as needed** by whatever feature needs them, as long as they go through
  the registry (§4.1) and get textures that match the atlas (§4.1). Decided 2026-10-17.
- **D-9 Blocks are a compile-time table**, not runtime ids loaded from data files: the enum is
  matched on in the hottest generation and meshing loops, and a table in code keeps that zero-cost
  and checked by the compiler (§4.1). Decided 2026-10-17.

---

## 8. Working on rustcraft

For agents and contributors:

- **Start here:** find the milestone, read the sections it cites, check the invariants. If the
  work doesn't fit a milestone, add one or say why in the PR.
- **Branches and PRs:** one milestone (or a clear part of one) per PR. Mark it ◐ when you start
  and ☑ when it's done, and record any decision it settles in §7.
- **Tests:** `cargo test`. Generation changes need tests that the output is deterministic and,
  where it applies, that full-detail chunks and LODs agree (see `src/chunks/lod/tests.rs` and
  `generator/tests.rs`). A change to what's generated also fails the golden checksum test in
  `src/chunks/generate/checksum.rs`: update its value deliberately, and say so in the PR, as
  older games can no longer join and saved edits sit on new terrain.
- **Quality:** `cargo fmt` and `cargo clippy` clean.
- **Performance:** for anything touching generation, meshing, LODs or per-frame systems, measure
  with the `profile` skill and `examples/lod_sim.rs`, and report the numbers in the PR (INV-7).
- **Textures:** new block textures with the `block-textures` skill.
- **Sounds:** new or changed sounds with the `sounds` skill.
- **Style:** match the surrounding code. Comments are plain prose explaining why, in British
  spelling ("colour", "metre"), and doc comments say what a thing is or does.
