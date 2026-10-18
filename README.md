# rustcraft

Another Minecraft clone, originally written using [specs](https://github.com/amethyst/specs) and [glium](https://github.com/glium/glium) but recently migrated to Bevy.

## Download

Get the archive for your system from the [Releases page](../../releases), unpack it and run `rustcraft` (`rustcraft.exe` on Windows). Keep the `assets` folder next to it.

- macOS (Apple Silicon): the build isn't notarised, so the first time, right-click `rustcraft` and choose Open.
- Linux: you need a Vulkan driver.
- Windows release builds open no console window, so their logs go nowhere.

To build it yourself instead:

```
cargo run --release
```

![Image of rustcraft](images/readme.jpg)

## Controls

- WASD to move, Shift to sprint, mouse to look.
- Space to jump, or to swim up in water.
- You start in survival, with nothing: punch a tree for logs, make planks and a crafting table, and work up from a wooden pickaxe. `/gamemode c` switches to creative, where you fly: Space and Ctrl move up and down, you pass through blocks, and F switches to walking and back.
- In survival, hold left click on a block to break it: cracks spread over it as it goes, and letting go or looking away starts again. Harder blocks take longer, and the right tool (a pickaxe for stone and ores, an axe for wood, a shovel for dirt, sand and gravel) is quicker. Stone and ores need a good enough pickaxe to drop anything: wood for stone and coal, stone for copper, copper for iron. Tools wear out as they're used, shown by the bar under them. In creative, left click breaks at once.
- Right click places the hotbar's selected block against the block you're looking at, or opens it if it's a crafting table, a furnace or a chest. 1–9 or the mouse wheel select a slot, and middle click selects the slot holding the block you're looking at, or in creative picks it into the selected slot.
- In survival, placing a block uses it up, and what you break drops an item: walk over it to pick it up. Q throws one of the held item, and Ctrl+Q the whole stack.
- E opens the inventory in survival: left click picks up or puts down a stack, right click picks up half or puts down one, and shift-click moves a stack between the hotbar and the rest. In creative E opens the block picker instead: click a block to put a stack of it in the selected slot. E or Escape closes either.
- Beside the inventory are the recipes you can make by hand, bright when you have what they take: click one to make it, or shift-click to make as many as you can. A log makes planks, planks make sticks and a crafting table, a stick and a coal (from coal ore) make torches, and right-clicking a crafting table opens the inventory with its recipes too: tools, a furnace, a chest and building blocks.
- Right-clicking a furnace opens it beside the inventory: put copper or iron ore (or cobblestone) in the top slot and fuel in the bottom one, and it smelts an ingot (or stone) every 10 seconds into the slot on the right. Coal burns for 8 items, logs and planks for 1½ and sticks for half. The flame shows how much fuel's left and the arrow how far the item's got. Furnaces carry on while you're away, and break into what was in them. Shift-click moves ore and fuel straight into the furnace.
- A chest (8 planks at a crafting table) holds 27 stacks: right-click it to open it over your inventory, and shift-click to move stacks between the two. Several players can have one open at once, and breaking it leaves it and everything in it. Chests found in the world's structures hold loot, rolled from the world's seed the first time anyone opens them, so it's the same for everyone.
- In survival you have ten hearts and a food bar over the hotbar. Falling more than three blocks hurts, unless it's into water, and so does running out of breath under water, shown by the bubbles. The food bar empties slowly, faster as you sprint and jump: while it's nearly full you heal, and when it's empty you go hungry down to half a heart. Hold right click with food selected to eat it; apples sometimes drop from leaves.
- When you die you drop everything you carried, and Respawn brings you back at the world's spawn, or at your bed. A bed is made at a crafting table from 3 planks and 3 tall grass: right-click it to respawn there, and at night to sleep in it. When everyone's asleep, the night passes.
- Farming: right-click dirt or grass with a hoe (2 planks, cobblestone or ingots and 2 sticks, at a crafting table) to till it into farmland, and right-click farmland with seeds, which tall grass sometimes drops, to plant wheat. It grows over two or three days, quicker with water within four blocks, and a field you've left has grown as it should by the time you're back. Break it ripe and golden for wheat and more seeds; 3 wheat make bread at a crafting table, which fills more of the food bar than an apple.
- `[` and `]` move the sun, and F1 shows the debug overlay.

## Commands

Press T or Enter, or / to start with the slash typed, to open the command bar, which shows what's been said lately. Enter runs the command, or says what you typed to everyone if it doesn't start with /, and Escape closes it.

- `/tp <x> <y> <z>` teleports there. `~` means where you are, and `~10` means ten blocks on from it.
- `/tp <x> <z>` teleports to just above the ground there.
- `/tp <player>` teleports to a player, and `/tp <player> <x> <y> <z>` teleports them.
- `/gamemode survival` or `/gamemode creative` (`s` or `c` for short) switches game mode, and `/gamemode <mode> <player>` switches someone else's. Survival can't fly, places only what it carries, and breaks blocks over time.
- `/seed` prints the world's seed. Run with `RC_SEED=<seed>` and a new world's name to get the same world again.
- `/list` lists the players here, and `/msg <player> <text>` says something only they see.
- `/op <player>` and `/deop <player>` let a player run the commands that change things, or stop them (see below).
- `/help` lists the commands you may run.

## Worlds

Worlds are saved every 30 seconds and when you quit, in a directory each under your data directory: `~/.local/share/rustcraft/worlds` on Linux, `~/Library/Application Support/rustcraft/worlds` on macOS and `%APPDATA%\rustcraft\worlds` on Windows. Run with `RC_WORLD=<name>` to play another world, which is made if it doesn't exist yet; the one played otherwise is called `world`.

## Playing together

To let friends join the world you're playing, choose Open to Friends in the menu (Escape), which shows the address to give them, or run with `RC_HOST=<port>` to open it as the game starts (`RC_HOST=` for the default port, 25600). Friends join with `RC_JOIN=<address>`, such as `RC_JOIN=192.168.1.20:25600`; the port can be left off if it's the default. Everyone needs the same version of rustcraft. Over the internet, the host forwards UDP port 25600 on their router.

Everyone plays by a name, shown over their head to the others: `RC_NAME=<name>`, or else the name you're logged in as. A name already taken gets a number after it. The host's world keeps each friend's place, look and inventory under their name, so they come back where they left. A friend's game decides how they move, but the host puts them back if they go faster than they can or walk into a wall.

Anything typed into the command bar without a / is said to everyone, by your name; saying a lot at once is slowed to a line a second.

Commands that change the world or other players, `/tp`, `/gamemode`, `/op` and `/deop`, and winding the clock with `[` and `]`, are for operators: the host, always, and whoever they make one with `/op <name>`. The world keeps its operators by name in `world.toml` (`ops = ["alex"]`), and `/deop <name>` takes it away again, except from the host. Anyone may `/seed`, `/list`, `/msg` and chat.

It's early days: connections aren't encrypted nor players' names checked, so anyone who can reach your world can join under any name, an operator's included, and take their place. Only open your world to friends you trust.

## Running a server

To keep a world open on an always-on machine, with no screen or graphics card, run the dedicated server, `rustcraft-server`, from its own archive on the [Releases page](../../releases), or build it yourself without the game's window and rendering:

```
cargo build --release --bin rustcraft-server --no-default-features
./target/release/rustcraft-server --world <name> --port 25600
```

- `--world <name>` hosts that world, made if it doesn't exist yet, from the same worlds directory as the game (or give a path); `--port <port>` takes players on that UDP port, 25600 if it's left off; `--seed <seed>` gives a new world that seed. `RC_WORLD`, `RC_HOST` and `RC_SEED` do the same. `--help` prints these.
- Friends join with `RC_JOIN=<address>` as above. Over the internet, forward UDP port 25600 to the server.
- It logs to the terminal it's run in. Type into it to run any command, as an operator, with or without the /: `op <name>` makes the first operator, `list` names who's on, `say <text>` says something to everyone as Server, `help` lists the rest, and `stop`, or Ctrl-C, saves the world and stops. A command that acts on you, such as `tp 1 2 3`, needs a player named instead, as the server has none.
- It saves every 30 seconds, as a player leaves and as it stops, the same way the game does, so a world can be copied between the game and a server. The time of day runs on while nobody's on.

The same warning as above applies, more so: anyone who can reach the server can join under any name, an operator's included. Only run one for friends you trust, until names are checked.

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

All contributions are welcome. Feel free to open issues for fixes or discussing changes.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
