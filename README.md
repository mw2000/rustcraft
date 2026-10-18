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
- Right click places the hotbar's selected block against the block you're looking at, or opens it if it's a crafting table or a furnace. 1–9 or the mouse wheel select a slot, and middle click selects the slot holding the block you're looking at, or in creative picks it into the selected slot.
- In survival, placing a block uses it up, and what you break drops an item: walk over it to pick it up. Q throws one of the held item, and Ctrl+Q the whole stack.
- E opens the inventory in survival: left click picks up or puts down a stack, right click picks up half or puts down one, and shift-click moves a stack between the hotbar and the rest. In creative E opens the block picker instead: click a block to put a stack of it in the selected slot. E or Escape closes either.
- Beside the inventory are the recipes you can make by hand, bright when you have what they take: click one to make it, or shift-click to make as many as you can. A log makes planks, planks make sticks and a crafting table, a stick and a coal (from coal ore) make torches, and right-clicking a crafting table opens the inventory with its recipes too: tools, a furnace and building blocks.
- Right-clicking a furnace opens it beside the inventory: put copper or iron ore (or cobblestone) in the top slot and fuel in the bottom one, and it smelts an ingot (or stone) every 10 seconds into the slot on the right. Coal burns for 8 items, logs and planks for 1½ and sticks for half. The flame shows how much fuel's left and the arrow how far the item's got. Furnaces carry on while you're away, and break into what was in them. Shift-click moves ore and fuel straight into the furnace.
- `[` and `]` move the sun, and F1 shows the debug overlay.

## Commands

Press T, or / to start with the slash typed, to open the command bar. Enter runs the command and Escape closes it.

- `/tp <x> <y> <z>` teleports there. `~` means where you are, and `~10` means ten blocks on from it.
- `/tp <x> <z>` teleports to just above the ground there.
- `/gamemode survival` or `/gamemode creative` (`s` or `c` for short) switches game mode. Survival can't fly, places only what it carries, and breaks blocks over time.
- `/seed` prints the world's seed. Run with `RC_SEED=<seed>` and a new world's name to get the same world again.
- `/help` lists the commands.

## Worlds

Worlds are saved every 30 seconds and when you quit, in a directory each under your data directory: `~/.local/share/rustcraft/worlds` on Linux, `~/Library/Application Support/rustcraft/worlds` on macOS and `%APPDATA%\rustcraft\worlds` on Windows. Run with `RC_WORLD=<name>` to play another world, which is made if it doesn't exist yet; the one played otherwise is called `world`.

## Playing together

To let friends join the world you're playing, choose Open to Friends in the menu (Escape), which shows the address to give them, or run with `RC_HOST=<port>` to open it as the game starts (`RC_HOST=` for the default port, 25600). Friends join with `RC_JOIN=<address>`, such as `RC_JOIN=192.168.1.20:25600`; the port can be left off if it's the default. Everyone needs the same version of rustcraft. Over the internet, the host forwards UDP port 25600 on their router.

Everyone plays by a name, shown over their head to the others: `RC_NAME=<name>`, or else the name you're logged in as. A name already taken gets a number after it. The host's world keeps each friend's place, look and inventory under their name, so they come back where they left. A friend's game decides how they move, but the host puts them back if they go faster than they can or walk into a wall.

It's early days: there's no chat yet, and connections aren't encrypted nor players' names checked, so play with people you trust.

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
