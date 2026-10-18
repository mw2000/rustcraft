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
| Biomes | Climate-driven desert, forest, foothills, glacier; tinted grass and leaves | `src/chunks/generate/biome/` |
| Trees | Placed on a deterministic 10-block grid, the same trees at every LOD level | `src/chunks/generate/trees.rs`, `generator/canopy.rs` |
| Plants | Tall grass and flowers on the grass, by climate, drawn as crossed quads at full detail (§5.5a) | `src/chunks/generate/plants.rs`, `generator/mesh.rs` |
| Rendering | Greedy meshing, an octree of LOD levels that cross-fade, out to 1024 chunks (2048 from high up); the land is bent around a round world | `src/chunks/lod/`, `generator/greedy.rs`, `src/chunks/fade.rs`, `src/curvature.rs`, `src/horizon.rs` |
| Light | Sky light (0–15) spread through each chunk; no block light | `src/chunks/generate/light.rs` |
| Sky | A day's clock, 3D clouds, sun and moon, night sky, sun glare, water reflections, tonemapping | `src/clock.rs`, `src/clouds.rs`, `src/sky*.rs`, `src/tonemap.rs` |
| Player | Flying, or walking under gravity with block collision; swimming; survival and creative modes | `src/player/`, `src/collision.rs`, `src/game_mode.rs` |
| UI | Pause menu (Escape), command bar with `/tp`, `/seed`, `/gamemode`, `/help` (T or /), hotbar (1–9, wheel, Q to drop), inventory screen in survival and block picker in creative (E), debug overlay (F1) | `src/menu.rs`, `src/commands.rs`, `src/hotbar.rs`, `src/inventory/`, `src/picker.rs`, `src/debug.rs` |
| Items | Every placeable block as an item, stacks, a 36-slot inventory per player, blocks that drop items when broken in survival, dropped items to pick up (§5.2) | `src/items.rs`, `src/inventory/`, `src/server/drops.rs`, `src/client/drops.rs` |
| Settings | Render distance, LOD scale, shadows | `assets/settings.toml`, `src/settings.rs` |
| Tools | `bench`, `lod_sim` and `world_map` examples; `profile` and `block-textures` skills | `examples/`, `.claude/skills/` |

Blocks are declared in the `blocks!` table in `src/block.rs`: the terrain's own (stone, grass, sand,
water, snow, logs, leaves, dirt, gravel, ice, sandstone), and ones for building and mining (planks,
cobblestone, stone bricks, bricks, coal, copper and iron ore, crafting table, furnace), and plants
(tall grass, poppy, dandelion, cornflower).

Blocks can be broken and placed, kept in an edit layer over the generated world (`src/edits.rs`,
`src/interaction.rs`), as requests the server checks (`src/protocol.rs`, `src/server/`,
`src/client/`). Worlds are saved as their edits and the player (`src/save/`).

Not there yet: crafting, breaking times, health, block light, structures, mobs, networking.

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

---

## 4. Architecture

### 4.1 Block registry

Every block is declared once, in the `blocks!` table in `src/block.rs` (INV-6, D-9).

- A row declares a `BlockType` variant with its properties: its name, whether it's solid,
  whether it's replaceable (a block placed into it takes its place: air, water, plants), its
  `light_opacity` and occlusion weight, its render style, and its tiles per face and for LODs.
  Properties are added to the row as the features that use them land (hardness, emitted light).
  Since M1.1 a row says what the block `drops` when broken in survival: `Nothing`, `Itself`
  (its own block item) or another block's item (§5.2).
- Render styles (`RenderStyle`): `None` (air), `Cube` (faces, culled against opaque neighbours
  and greedy-merged), `Liquid` (water's own mesh) and `Cross` (two diagonal quads, double-sided
  and alpha cut out, at full detail only). A `Cross` block is a plant: not solid, lets all light
  through, hides no faces, and stands on the block below.
- `BlockType` stays a one-byte enum, stored in `ChunkData`'s runs, so generation and meshing can
  match on it and its properties are plain array lookups.
- Each block has a stable snake_case name such as `"sandstone"` (`BlockType::name` and
  `from_name`). Saves and the network protocol store names, or a palette mapped to names, never
  discriminants, so reordering the table can't corrupt a world.
- Textures go into the atlas with the `block-textures` skill: generated with ComfyUI, or drawn by
  its `draw.py` from the existing tiles' shades where they need hard edges (bricks, ores, crafted
  blocks) or ComfyUI isn't set up.

New blocks are added as features need them, and that's expected (§5).

### 4.2 Edit layer (proposed)

The world's state beyond the seed (INV-1).

- Edits are kept per chunk, as a sparse map from block position to `BlockType`: what a player put
  there, air included. A chunk's blocks are its generated blocks with its edits applied on top.
- An edit marks its chunk, and any neighbour it borders, for remeshing and relighting.
- **Built (M0.2):** `Edits` (`src/edits.rs`) is held by `World`, which puts a chunk's edits back
  as it's loaded (`insert_chunk`). The world only changes through an `EditBlock` message, made by
  `apply_edits`, so the server can take that over (INV-4). An edit changes its loaded chunk at
  once, for collision, then rebuilds the group meshes of every chunk whose mesh reads the block
  (`chunks_meshed_with`): those whose sky light takes it in, which reaches 8 blocks sideways and a
  chunk down, and covers faces, ambient occlusion and shores across borders. Light is worked out
  per mesh from the chunk and the one above, so a shaft or a roof changes light that far down.
  Rebuilds mesh from the world's blocks on the compute pool, ahead of loading work, and swap the
  new mesh onto the same entity with no fade (`group::rebuild_groups`, `load_groups`). Since
  M0.5b the server makes edits and the client queues the rebuilds (§4.4).
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
- **Built (M0.3):** `src/save/`. Worlds live under the platform's data directory
  (`~/.local/share/rustcraft/worlds/<name>/` on Linux, from the `dirs` crate), picked with
  `RC_WORLD` (default `world`). A missing world is made with `RC_SEED` or a random seed, and its
  `world.toml` written at once; an existing one keeps its seed, and a disagreeing `RC_SEED` is
  ignored with a log line.
  - `world.toml` holds the format version (`FORMAT_VERSION`, now 2), the game version that wrote
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
  `RequestEdit { block, to, slot, seq }`, `RunCommand`, `Chat`, the inventory's requests
  (`InventoryClick`, `CloseInventory`, `DropItem`, `PickBlock`, §5.2), and from M0.6
  `PlayerState` (unreliable, ~20 Hz). Server to client: `Welcome { game_version, protocol, seed, palette, samples, player,
  time_of_day }`, `ChunkEdits` (edits of chunks near the player, on joining and as they move),
  `BlockChanged { block, to, by }`, `EditRejected { seq, block, actual }`, `TimeSync`, `Teleport`,
  `CommandOutput`, `ChatLine`. Players are replicated entities, and so are dropped items; a
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
    Not synced yet (edits came with M0.6c): other players,
    whom nobody sees (M0.6d); `/tp`, which moves only the server's copy of a client's player,
    and `/gamemode`, which the client's own movement doesn't follow (M0.6d's `Teleport`, and
    the mode to come with it); a client's player is never saved and always arrives at the
    spawn (M0.6d); a client leaving while winding leaves the clock wound. Poses aren't checked
    for plausibility (M0.6d), and `REACH_SLACK` hasn't been raised for a pose's delay, so a
    client flying fast can have an edit at the edge of reach refused.
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
- **Layout:** `src/protocol.rs`; `src/server/` (`mod.rs` with `ServerPlugin` and `is_authority`,
  `edits.rs`, `chunks.rs`, `commands.rs`, `clock.rs`, `host.rs`, `players.rs`, `sync.rs`); `src/save/`
  under the server; `src/client/` (`mod.rs` with `ClientPlugin` and today's scene setup,
  `edits.rs`, `player.rs`, `join.rs`).
  Shared modules stay where they are. Later a default-on `client` cargo feature keeps wgpu and
  winit out of the dedicated server binary.

### 4.5 Game modes (proposed)

- **Survival:** health, hunger (D-5), breaking takes time and the right tool, finite blocks.
- **Creative:** flying, any block from a picker, instant breaking, no damage.
- **Spectator (later):** fly through blocks, no interaction.

Switched with `/gamemode`, which in multiplayer needs operator permission.

- **Built (M0.4):** `GameMode` (`src/game_mode.rs`) is a component on the player, so each player
  can have their own, saved with them (§4.3). It's **creative by default for now**, since
  survival has no breaking times, crafting or health yet: with only instant breaking and
  nothing to make, a new world in survival would be a slower creative. Everything the modes
  change asks `GameMode`'s methods (`can_fly`, `has_every_block`, `drops_broken_blocks`), so
  M1.2 adds its own there. Creative flies (F toggles), breaks at once, has the block picker and
  never uses blocks up. Survival can't fly, and switching to it drops a flying player into
  walking; it breaks at once until M1.2, but **since M1.1** places only what's in the hotbar
  slot held, using it up, and what it breaks drops its item (§5.2). Middle click only selects
  a block already in the hotbar. `/gamemode survival|creative` (or `s`/`c`) switches, keeping
  the inventory as it is.

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
    variant for each row of the `items!` table, which is empty until crafting and smelting
    bring sticks, tools, coal and ingots. A row has a stable snake_case name and columns
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
  (`World::edit`), so a shaft dug into a cave lights it. Cave interiors are dark until block
  light (M1.7).
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
  every sixty. Regional ores (M2.5) join the same table, by climate as well as depth.
- Underground structures (mineshafts, dungeons) come with §5.6.

### 5.5 Survival

- Health with fall damage, drowning, and later mob damage. Death drops your inventory where you
  died and respawns you at your spawn point (a bed sets it).
- Hunger (D-5): a food bar that drains slowly, faster while sprinting, jumping or healing.
  Eating refills it; a fuller bar heals over time and an empty one hurts, down to half a heart.
  No saturation or exhaustion arithmetic beyond that.
- Day and night already exist; night brings hostile mobs (§5.8).
- Block light from torches and other light blocks, alongside sky light, so caves and nights can be
  lit. Needs light to spread across chunk borders and update when blocks change.

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
  once they exist (§5.8). Food items say how much hunger they fill.
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
  goes, dropping nothing until seeds from tall grass come with farming.

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

Built: `.github/workflows/release.yml` builds `--release` on every `v*` tag for Linux x86_64,
Windows x86_64 and macOS Apple Silicon (Intel macOS is skipped), packs each as
`rustcraft-<version>-<target>/` holding the binary, `assets/`, the README and both licences
(`.tar.gz`, or `.zip` on Windows), and attaches them to a GitHub release. Run by hand, it uploads
the archives as workflow artifacts instead. The binary finds `assets/` next to itself
(`settings::asset_path`, the same rule as Bevy's `AssetPlugin`), so double-clicking works, and
Windows release builds open no console, so their logs go nowhere. macOS builds are not notarised.
Still to come: the dedicated server binary (M0.5/M0.6), which gets a second `--bin` and archive
entry in the workflow.

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
- ◐ **M0.6 Networked multiplayer.** Join by address, determinism handshake, players and edits
  replicated, simple player models, chat (§4.4). In steps:
  - ☑ **M0.6a Transport.** `bevy_replicon_renet2`, hosting and joining (`RC_HOST`, `RC_JOIN`),
    two-App tests over the memory transport. `EditAllowances` forgets a client when it
    disconnects.
  - ☑ **M0.6b Determinism handshake**, with golden checksums and a CI matrix of Windows, macOS
    and Linux (D-3).
  - ☑ **M0.6c Edit sync**: `ChunkEdits` on joining and moving, `BlockChanged` by area, client
    prediction with rollback.
  - ☐ **M0.6d Other players**: names, interpolated poses, simple models, plausibility checks with
    `Teleport` corrections, a save per player.
  - ☐ **M0.6e Chat and operators**, an `ops` list in `world.toml` with the host as operator.
- ☑ **M0.7 CI.** Build, test, clippy and fmt on every PR.

### Phase 1: the survival core

Done when: **you can start with nothing and make iron tools.**

- ☑ **M1.1 Hotbar, inventory and items.** Data-driven items, stacks, dropped items (§5.2).
- ☐ **M1.2 Survival breaking.** Break time from hardness and tool (§5.1). Drops came with M1.1;
  M1.2 makes some need the right tool.
- ☑ **M1.3 Caves.** 3D carving underground, cave mouths, buried-chunk skipping updated (§5.4).
- ☑ **M1.4 Ores.** Coal, copper, iron by depth, in veins (§5.4).
- ☐ **M1.5 Crafting.** Recipes as data, crafting by hand and at a crafting table; wood, stone,
  copper and iron tools (§5.3).
- ☐ **M1.6 Furnace.** Smelting ores, with containers (§5.2).
- ☐ **M1.7 Block light.** Torches, light across chunk borders, updated on edits (§5.5).
- ☐ **M1.8 Health and hunger.** Fall damage, drowning, death and respawn, beds, the food bar
  (§5.5).
- ☐ **M1.9 Plants and farming** (§5.5a).
  - ☑ **M1.9a Plant blocks.** The cross render style, tall grass and flowers in the world.
  - ☐ **M1.9b Farming.** Hoes, farmland, wheat that grows, bread and apples.

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

- ◐ **M-R1 Release builds** for Windows, macOS and Linux, and a dedicated server (§5.9). Release workflow done; the server binary is to come.

---

## 7. Decisions

### Open

- **D-3 Cross-platform determinism.** Is same-version determinism enough, or does generation need
  to avoid platform-dependent float functions (`sin`, `exp` and the like differ across libm
  implementations) so Windows, macOS and Linux agree? Test early, in M0.6, by comparing chunk
  checksums across platforms. If they differ, switch the noise to deterministic math. Since
  M0.6b CI's `determinism` job runs the golden checksum test (§4.4) on `ubuntu-latest`,
  `windows-latest` and `macos-latest`: if they all pass, same-version determinism holds there
  for now and this can be decided; a platform that fails shows where it doesn't.
- **D-4 Crafting model.** Minecraft-style shaped grid, or ingredient lists at stations. Leaning
  towards ingredient lists: simpler, clearer, and less of a copy.

### Decided

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
- **Style:** match the surrounding code. Comments are plain prose explaining why, in British
  spelling ("colour", "metre"), and doc comments say what a thing is or does.
