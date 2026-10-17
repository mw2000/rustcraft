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
| World | 1024 blocks tall, sea level 448, 16³ chunks stored run-length encoded, seeded with `RC_SEED` | `src/world.rs`, `src/chunks/chunk.rs`, `src/chunks/generate/mod.rs` |
| Terrain | Heightmap terrain from noise, sampled per 256-block region, or per coarse cell for far LODs. No caves or overhangs. | `src/chunks/generate/` (`noise.rs`, `region*.rs`, `columns.rs`) |
| Biomes | Climate-driven desert, forest, foothills, glacier; tinted grass and leaves | `src/chunks/generate/biome/` |
| Trees | Placed on a deterministic 10-block grid, the same trees at every LOD level | `src/chunks/generate/trees.rs`, `generator/canopy.rs` |
| Rendering | Greedy meshing, an octree of LOD levels that cross-fade, out to 1024 chunks (2048 from high up); the land is bent around a round world | `src/chunks/lod/`, `generator/greedy.rs`, `src/chunks/fade.rs`, `src/curvature.rs`, `src/horizon.rs` |
| Light | Sky light (0–15) spread through each chunk; no block light | `src/chunks/generate/light.rs` |
| Sky | 3D clouds, sun and moon, night sky, sun glare, water reflections, tonemapping | `src/clouds.rs`, `src/sky*.rs`, `src/tonemap.rs` |
| Player | Flying, or walking under gravity with block collision; swimming | `src/player.rs`, `src/collision.rs` |
| UI | Pause menu (Escape), command bar with `/tp`, `/seed`, `/help` (T or /), debug overlay (F1) | `src/menu.rs`, `src/commands.rs`, `src/debug.rs` |
| Settings | Render distance, LOD scale, shadows | `assets/settings.toml`, `src/settings.rs` |
| Tools | `bench`, `lod_sim` and `world_map` examples; `profile` and `block-textures` skills | `examples/`, `.claude/skills/` |

Blocks today: Air, Stone, Grass, Sand, Water, Snow, Log, Leaves, Dirt, Gravel, Ice, Sandstone, as a
Rust enum (`src/block.rs`).

Not there yet: breaking or placing blocks, saving, inventory, crafting, health, caves, ores, block
light, structures, mobs, networking.

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
- **INV-6 Content is data.** Blocks, items, recipes, loot tables and structures are defined as data
  files the game loads, not as code scattered across systems. Adding a block shouldn't mean
  touching ten `match` statements.
- **INV-7 The frame budget is protected.** Gameplay systems don't block the main thread on
  generation or I/O. Measure performance-sensitive changes with the `profile` skill and say in the
  PR what they cost.

---

## 4. Architecture

### 4.1 Block registry (proposed)

Replace the `BlockType` enum with a registry loaded from data (INV-6).

- A block is a numeric `BlockId` (`u16`) stored in `ChunkData`'s runs. Its properties live in a
  table: name, textures per face, solid, opacity and `light_opacity`, emitted light, hardness,
  best tool, what it drops, whether it's replaceable (like tall grass), and render style (cube,
  cross, transparent, liquid).
- Definitions live in `assets/blocks/*.toml` (or one file), keyed by a stable string name such as
  `rustcraft:oak_planks`. Saves and the network protocol use names or a palette mapped to names,
  never raw ids, so reordering the table can't corrupt a world.
- The generator keeps fast access to the blocks it places, resolved once at startup.
- Textures go into the atlas with the `block-textures` skill.

New blocks are added as features need them, and that's expected (§5).

### 4.2 Edit layer (proposed)

The world's state beyond the seed (INV-1).

- Edits are kept per chunk, as a sparse map from block position to `BlockId`: what a player put
  there, air included. A chunk's blocks are its generated blocks with its edits applied on top.
- An edit marks its chunk, and any neighbour it borders, for remeshing and relighting.
- Edits are what's saved (§4.3) and what's sent over the network (§4.4). Generated blocks are
  neither.
- Chunks with heavy edits (a big build) can switch to storing their whole contents rather than a
  sparse map.
- **Edits in LODs (later, M2.x):** to keep INV-5 for big builds, LOD nodes take the edited chunks
  they cover into account, so a tower shows up from far away. Phase 0 needs only to keep this
  possible: keep edits queryable by area. Note that `src/chunks/lod/spawn.rs:108` assumes
  generated chunks only and must change when edits reach the LODs.

### 4.3 Saving (proposed)

- A world is a directory: `world.toml` (seed, game version, name, game mode, time of day),
  `players/` (position, inventory, health per player), and `edits/` holding region files of edits.
  Edit region files group 32×32 columns of chunks so a world isn't thousands of tiny files.
- Saves are small, because only edits are stored.
- Save on a timer and on quit, from a background task (INV-7).
- Saves carry a format version and a game version, and old saves keep loading or are migrated
  explicitly.

### 4.4 Multiplayer (decided: client/server with an integrated server; library open, D-1)

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

### 4.5 Game modes (proposed)

- **Survival:** health, hunger (D-5), breaking takes time and the right tool, finite blocks.
- **Creative:** flying, any block from a picker, instant breaking, no damage.
- **Spectator (later):** fly through blocks, no interaction.

Switched with `/gamemode`, which in multiplayer needs operator permission.

---

## 5. Game systems

### 5.1 Interaction

- Look at a block within reach (about 5 blocks) with a voxel raycast, and outline it.
- Left mouse breaks: instant in creative; in survival over a time set by hardness and tool, with
  cracks drawn on the block.
- Right mouse places the held block against the face looked at, unless it would overlap a player,
  or uses a block (opening a chest, a door, a crafting table).
- Mouse wheel and 1–9 pick the hotbar slot.

### 5.2 Inventory and items

- Items are data (INV-6): block items for every placeable block, plus tools, materials, food.
- Hotbar of 9 slots plus a main inventory, with stacks (64 by default, tools 1). Durability on
  tools.
- Dropped items exist as small entities that can be picked up.
- Containers (chests, furnaces) hold their own inventories, stored as block entity state in the
  edit layer.

### 5.3 Crafting and progression

- Recipes are data (INV-6). Proposed: a simpler crafting model than Minecraft's shaped grid, such as
  a list of ingredients and an output, crafted at a station (by hand, crafting table, furnace,
  anvil). Open, D-4.
- Tool tiers set what can be mined. Proposed tiers, to rename and tune freely:

  | Tier | Material | Mines | Where it's found |
  |---|---|---|---|
  | 0 | Hand | Dirt, sand, logs, leaves | Everywhere |
  | 1 | Wood | Stone | Everywhere |
  | 2 | Stone | Copper, coal | Everywhere, shallow |
  | 3 | Copper | Iron | Everywhere, mid-depth |
  | 4 | Iron | Regional ores | Specific regions (P3) |
  | 5 | Alloys from regional ores | The deepest ores | Deep, specific regions |

- **Progression needs travel (P3):** higher tiers need ores found only in certain climates (under
  glaciers, deep under deserts, in mountain cores), so moving up means going somewhere.

### 5.4 Underground: caves and ores

- Caves are carved from 3D noise below the surface: tunnels and caverns, denser with depth. The
  world has 448 blocks below sea level for depth tiers and deep caverns.
- Cave mouths break the surface sometimes, so caves can be found from above.
- LOD impact is small: caves are hidden from far away, so distant LODs keep drawing the surface.
  Cave mouths should show up at mid-range LODs where they're big enough (INV-5). Buried-chunk
  skipping (`is_chunk_buried`, `lod/spawn.rs`) must account for caves, so a chunk is only
  skipped when it really is solid.
- Ores are placed by depth band and region, in veins, deterministically (INV-2, INV-3).
- Underground structures (mineshafts, dungeons) come with §5.6.

### 5.5 Survival

- Health with fall damage, drowning, and later mob damage. Death drops your inventory where you
  died and respawns you at your spawn point (a bed sets it).
- Hunger is open (D-5).
- Day and night already exist; night brings hostile mobs (§5.8).
- Block light from torches and other light blocks, alongside sky light, so caves and nights can be
  lit. Needs light to spread across chunk borders and update when blocks change.

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
- **First structures, proposed:** desert temple, forest ruin, glacier outpost, mountain
  watchtower, mineshaft, dungeon, then villages.
- New biomes come with the structures that need them, such as a jungle for jungle temples.

### 5.7 World and biomes

- More biomes as content needs them: jungle, plains, swamp, savanna, taiga, badlands, and
  others. Each is a module under `src/chunks/generate/biome/` with its climate, blocks, trees and
  structures.
- **Landmarks:** rare continent-scale features (a huge peak, a volcano, a giant tree, a fallen
  tower) placed far apart and visible from very far away.
- A map: a top-down view of explored land, drawn from the same terrain sampling as `world_map`.

### 5.8 Mobs and entities (later)

- Passive animals (food, materials) and hostile mobs at night and in dark caves.
- Needs an entity system on the server, pathfinding over voxels, simple models and animation, and
  replication (§4.4). This is the biggest single system in the plan, so it comes after the core.
- Villagers come with villages.

### 5.9 Distribution

- Release builds for Windows, macOS and Linux, published on GitHub, that run without installing
  Rust. A dedicated server binary alongside.
- CI that builds and tests every PR (there is none yet).

---

## 6. Roadmap

Each milestone is a PR-sized piece of work with a test of when it's done. Do them in order within
a phase unless they're independent; mark them done here in the PR that finishes them.

Status: ☐ not started, ◐ in progress, ☑ done.

### Phase 0: foundations

Done when: **two players on one world see each other and each other's edits, and the world saves
and loads.**

- ☐ **M0.1 Block registry.** Replace `BlockType` with registry-backed `BlockId` and block data
  files (§4.1). No change in what's drawn. Tests cover lookup by name and generation output
  staying identical.
- ☐ **M0.2 Breaking and placing.** Raycast, outline, break and place in creative style, with
  remeshing and relighting of the edited chunk and its neighbours (§5.1). The edit layer holds the
  changes (§4.2).
- ☐ **M0.3 Saving and loading.** World directory, edits and player position saved and loaded
  (§4.3). Edits survive a restart.
- ☐ **M0.4 Game modes.** Survival and creative, `/gamemode`, creative block picker (§4.5).
- ☐ **M0.5 Client/server split.** Integrated server in-process; game state flows through it
  (§4.4). Singleplayer plays the same as before.
- ☐ **M0.6 Networked multiplayer.** Join by address, determinism handshake, players and edits
  replicated, simple player models, chat (§4.4).
- ☐ **M0.7 CI.** Build, test, clippy and fmt on every PR.

### Phase 1: the survival core

Done when: **you can start with nothing and make iron tools.**

- ☐ **M1.1 Hotbar, inventory and items.** Data-driven items, stacks, dropped items (§5.2).
- ☐ **M1.2 Survival breaking.** Break time from hardness and tool, drops (§5.1).
- ☐ **M1.3 Caves.** 3D carving underground, cave mouths, buried-chunk skipping updated (§5.4).
- ☐ **M1.4 Ores.** Coal, copper, iron by depth, in veins (§5.4).
- ☐ **M1.5 Crafting.** Recipes as data, crafting by hand and at a crafting table; wood, stone,
  copper and iron tools (§5.3).
- ☐ **M1.6 Furnace.** Smelting ores, with containers (§5.2).
- ☐ **M1.7 Block light.** Torches, light across chunk borders, updated on edits (§5.5).
- ☐ **M1.8 Health.** Fall damage, drowning, death and respawn, beds (§5.5).

### Phase 2: exploration

Done when: **there's something worth walking 5 km to see, and you can see it from where you
start.**

- ☐ **M2.1 Structure system.** Placement, templates, terrain fit, LODs (§5.6).
- ☐ **M2.2 Structure authoring.** `/structure save` and loading in creative mode (§5.6).
- ☐ **M2.3 Chests and loot tables.** (§5.6)
- ☐ **M2.4 First structures.** Temple, ruin, outpost, watchtower, mineshaft, dungeon (§5.6).
- ☐ **M2.5 Regional ores and higher tiers.** (§5.3)
- ☐ **M2.6 New biomes.** Starting with the ones structures need (§5.7).
- ☐ **M2.7 Landmarks.** (§5.7)
- ☐ **M2.8 Edits in LODs.** Players' builds seen from far away (§4.2).
- ☐ **M2.9 Map.** (§5.7)

### Phase 3: life

Done when: **the world feels inhabited, and night is dangerous.**

- ☐ **M3.1 Entity system and pathfinding.** (§5.8)
- ☐ **M3.2 Passive animals.**
- ☐ **M3.3 Hostile mobs and combat.**
- ☐ **M3.4 Villages and villagers.**
- ☐ **M3.5 Weather.**

### Throughout

- ☐ **M-R1 Release builds** for Windows, macOS and Linux, and a dedicated server (§5.9).

---

## 7. Decisions

### Open

- **D-1 Networking library.** Candidates: `lightyear`, `bevy_replicon` (with `renet` or
  `quinn`), or a small protocol of our own over QUIC. Must support Bevy 0.20, or we pin and
  follow it. Decide before M0.5; recommend whichever has Bevy 0.20 support and prediction for
  player movement.
- **D-3 Cross-platform determinism.** Is same-version determinism enough, or does generation need
  to avoid platform-dependent float functions (`sin`, `exp` and the like differ across libm
  implementations) so Windows, macOS and Linux agree? Test early, in M0.6, by comparing chunk
  checksums across platforms. If they differ, switch the noise to deterministic math.
- **D-4 Crafting model.** Minecraft-style shaped grid, or ingredient lists at stations. Leaning
  towards ingredient lists: simpler, clearer, and less of a copy.
- **D-5 Hunger.** Include it, simplify it, or leave it out. Decide before M1.8.
- **D-6 Save format encoding.** A compact binary format (such as `bincode` or `postcard`) for edit
  regions; TOML for `world.toml`. Decide in M0.3.

### Decided

- **D-2 Multiplayer architecture:** client/server with an integrated server, terrain generated by
  clients from the seed, server-authoritative state (§4.4). Decided 2026-10-17.
- **D-7 Direction:** survival and exploration first, with a creative mode; mining-driven
  progression; multiplayer; structures to find; an open, easy-to-play hobby project (§1). Decided
  2026-10-17.
- **D-8 Blocks are added as needed** by whatever feature needs them, as long as they go through
  the registry (§4.1) and get textures through the `block-textures` skill. Decided 2026-10-17.

---

## 8. Working on rustcraft

For agents and contributors:

- **Start here:** find the milestone, read the sections it cites, check the invariants. If the
  work doesn't fit a milestone, add one or say why in the PR.
- **Branches and PRs:** one milestone (or a clear part of one) per PR. Mark it ◐ when you start
  and ☑ when it's done, and record any decision it settles in §7.
- **Tests:** `cargo test`. Generation changes need tests that the output is deterministic and,
  where it applies, that full-detail chunks and LODs agree (see `src/chunks/lod/tests.rs` and
  `generator/tests.rs`).
- **Quality:** `cargo fmt` and `cargo clippy` clean.
- **Performance:** for anything touching generation, meshing, LODs or per-frame systems, measure
  with the `profile` skill and `examples/lod_sim.rs`, and report the numbers in the PR (INV-7).
- **Textures:** new block textures with the `block-textures` skill.
- **Style:** match the surrounding code. Comments are plain prose explaining why, in British
  spelling ("colour", "metre"), and doc comments say what a thing is or does.
