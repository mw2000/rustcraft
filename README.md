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

## Menus

The game opens on the title screen, over a view of a world's horizon:

- **Singleplayer** lists your worlds, the last played first, with each one's seed, game mode and when you last played it. Double-click one, or select it and press Enter or Play Selected World, to play it. Create New World makes one, with a name, a seed (or leave it blank for a random one) and the game mode new players start in; Rename, Duplicate and Delete (which asks first) look after the rest. A loading screen shows while the land round you loads.
- **Multiplayer** lists the servers you've saved: Add Server saves one by its name and address, Edit and Remove change them, and double-click or Join Server joins one. Or type an address under the list to join it once. Your name, shown over your head to others, is at the top. Whether a server's up shows as you join it: if it can't be joined, or the connection's lost, the menu says why.
- **Settings**, saved for you under your data directory (`settings.toml`), in tabs:
  - *Video:* a Low, Medium or High preset, how far the terrain's drawn and how far at full detail (which apply as you close Settings), field of view, UI scale, a frame cap, fullscreen, vsync, shadows, clouds, and ambient life (butterflies, fireflies and birds).
  - *Controls:* mouse sensitivity, inverting the mouse, and a key for every action: click one and press the key or mouse button it should be. Keys bound twice show in red, and Reset Keys puts them all back.
  - *Audio:* the volume, and the effects, ambience and music each on their own.
  - *Accessibility:* the view's tilt when you're hurt, larger text, and the crosshair's style.
  - *Skin:* how others see you. Type the path of a skin, a 64×64 or 64×32 PNG in Minecraft's layout of up to 32 KB, or keep yours in the `skins` folder under your data directory and pick one from the list; it shows on a turning model beside them. Classic or slim arms are set from the skin and can be switched. Default Skin goes back to one of the game's own, chosen by your name.

Escape in the game opens the game menu: Back to Game, Settings, Open to Friends (see below), and Save and Quit to Title, which saves the world and goes back to the title screen, ready to play another. Escape goes back a page anywhere in the menus, Tab and the arrow keys move between buttons, and Enter presses them.

## Controls

These are the keys as they start; any can be changed in Settings, Controls.

- WASD to move, Shift to sprint, mouse to look.
- Space to jump, or to swim up in water.
- You start in survival, with nothing: punch a tree for logs, make planks and a crafting table, and work up from a wooden pickaxe. `/gamemode c` switches to creative, where you fly: Space and Ctrl move up and down, you pass through blocks, and F switches to walking and back.
- In survival, hold left click on a block to break it: cracks spread over it as it goes, and letting go or looking away starts again. Harder blocks take longer, and the right tool (a pickaxe for stone and ores, an axe for wood, a shovel for dirt, sand and gravel) is quicker. Stone and ores need a good enough pickaxe to drop anything: wood for stone and coal, stone for copper, copper for iron. The best tools need travel: gold deep under deserts and savannas, frost crystal under glaciers and taiga, and jade under jungles need an iron pickaxe, and any of them blended with an iron ingot at a crafting table smelts into bright steel, whose pickaxe mines the glowing embers at the very bottom of the world. Tools wear out as they're used, shown by the bar under them. In creative, left click breaks at once.
- Right click places the hotbar's selected block against the block you're looking at, or opens it if it's a crafting table, a furnace or a chest. 1–9 or the mouse wheel select a slot, and middle click selects the slot holding the block you're looking at, or in creative picks it into the selected slot.
- In survival, placing a block uses it up, and what you break drops an item: walk over it to pick it up. Q throws one of the held item, and Ctrl+Q the whole stack.
- E opens the inventory in survival: left click picks up or puts down a stack, right click picks up half or puts down one, and shift-click moves a stack between the hotbar and the rest. In creative E opens the block picker instead: click a block to put a stack of it in the selected slot. E or Escape closes either.
- Beside the inventory are the recipes you can make by hand, bright when you have what they take: click one to make it, or shift-click to make as many as you can. A log makes planks, planks make sticks and a crafting table, a stick and a coal (from coal ore) make torches, and right-clicking a crafting table opens the inventory with its recipes too: tools, a furnace, a chest and building blocks.
- Right-clicking a furnace opens it beside the inventory: put copper or iron ore (or cobblestone) in the top slot and fuel in the bottom one, and it smelts an ingot (or stone) every 10 seconds into the slot on the right. Coal burns for 8 items, logs and planks for 1½ and sticks for half. The flame shows how much fuel's left and the arrow how far the item's got. Furnaces carry on while you're away, and break into what was in them. Shift-click moves ore and fuel straight into the furnace.
- Building blocks: stairs face the way you look and slabs go in the half of a side you click, both turned over against a ceiling, logs lie along the face you click, and furnaces face you. There are stairs, slabs and walls of planks, dark planks, cobblestone, stone bricks, bricks and sandstone, fences that join, glass and panes, doors (right-click to open; they're two blocks tall), trapdoors, ladders and vines to climb (walk into them or jump), lanterns standing or hanging, and weathered, polished and coloured blocks. You step up onto slabs and stairs without jumping. Their recipes are at the crafting table; glass, cracked stone bricks and terracotta come from the furnace.
- A chest (8 planks at a crafting table), or a barrel, holds 27 stacks: right-click it to open it over your inventory, and shift-click to move stacks between the two. Several players can have one open at once, and breaking it leaves it and everything in it. Chests found in the world's structures hold loot, rolled from the world's seed the first time anyone opens them, so it's the same for everyone.
- In survival you have ten hearts and a food bar over the hotbar. Falling more than three blocks hurts, unless it's into water, and so does running out of breath under water, shown by the bubbles. The food bar empties slowly, faster as you sprint and jump: while it's nearly full you heal, and when it's empty you go hungry down to half a heart. Hold right click with food selected to eat it; apples sometimes drop from leaves.
- When you die you drop everything you carried, and Respawn brings you back at the world's spawn, or at your bed. A bed is made at a crafting table from 3 planks and 3 wool: right-click it to respawn there, and at night to sleep in it. When everyone's asleep, the night passes.
- Farming: right-click dirt or grass with a hoe (2 planks, cobblestone or ingots and 2 sticks, at a crafting table) to till it into farmland, and right-click farmland with seeds, which tall grass sometimes drops, to plant wheat. It grows over two or three days, quicker with water within four blocks, and a field you've left has grown as it should by the time you're back. Break it ripe and golden for wheat and more seeds; 3 wheat make bread at a crafting table, which fills more of the food bar than an apple.
- Animals live on the grass and come out by day: sheep, cows, pigs and chickens. Hit them for meat (mutton, beef, pork, chicken), which fills more cooked in a furnace, and cows also give leather and chickens feathers, for later. Chickens flutter down from any height unhurt. They follow you while you hold their food (wheat for sheep and cows, apples or wheat for pigs, seeds for chickens): right-click two grown ones with it and they find each other and have a young one, which grows up in about four minutes, and they rest five minutes before they will again. A crowd of 24 of a kind takes no more food.
- The world has small life of its own: butterflies over the flowers by day, fireflies glowing by water and in the woods at night, starlings wheeling over open land at dusk, and geese passing high over in a V. They're only decoration, never in your way, and each player sees their own.
- Animals breathe, blink, turn their heads to look at you when you're near, and sheep and cows graze; when one dies it tips over and fades. Other players' heads follow where they look, and their arms swing as they break and place.
- Sheep killed drop a wool; shears (2 iron ingots at a crafting table) shear 1–3 wool off with a right-click instead, and the fleece grows back in a few minutes. 4 wool make a wool block by hand.
- Night is dangerous, and so are dark caves. Monsters come out only where it's dark: gloamers stalk the open at night and leap the last few blocks at you, fading at sunrise; lurkers hang from cave ceilings and drop on whoever walks under them, but run from torchlight and from a torch in your hand; slingers keep their distance and throw stones, so rush them or get behind something; and barrow brutes guard the deep caves, slow but hitting very hard, raising their arms a full second before they swing, which is your moment to step back. Dungeons hold spawners that keep bringing monsters out while you're near, until you light them with a torch or break them. Monsters go once you're far away, and aren't saved.
- Villages, a couple in every few square kilometres of temperate country, savanna, desert and taiga, are built in the country's own style: timber-framed and thatched cottages in the green lands, adobe houses with roof terraces and stepped towers in the hot ones, and dark-timbered lodges in the snowy pines. Streets run out from a well past houses, an inn or hall, a smithy, fields of wheat, a pen of animals and a bell tower, lit by lamp posts at night. Their villagers, robed and hatted by their calling, work by day (farmers in the fields, smiths at the forge, traders at the inn, shepherds by the pen, lamplighters at the tower), meet at the well at noon and in the evening, go home through their doors at dusk and run from monsters. One killed stays dead.
- Trading: right-click a villager in survival to see what they'll trade today, three of their calling's trades, each a few times a day. They pay coins for what you bring (wheat, coal, iron, wool, leather) and take them for what they make: food, tools and armour, light, glass, and from traders gold, frost crystal and jade from far away. Click a trade to make it.
- Building blocks from villages: plaster (sand and stone), adobe (dirt and wheat) and thatch (wheat), with adobe and thatch stairs and slabs, at a crafting table.
- Staying safe: nothing ever appears where a torch or lantern lights the ground, so a lit base is safe, and walls and a roof keep out what comes from further off. Light your way through caves.
- Fighting: swords (2 of a material and a stick at a crafting table, in wood, stone, copper, iron and bright steel) hit hardest and quickest, axes nearly as hard but slower. Hitting while you're coming down from a jump is a critical hit, half as hard again. Armour, a helmet, chestplate, leggings and boots of leather, iron or bright steel, goes in the four slots beside the inventory and takes part of every blow, wearing as it does; the bar over your hearts shows how much you wear. When you're hit the view shakes and a red mark round the crosshair shows where it came from.
- M opens the map of the land you've explored, centred on you: the arrow is you and the way you face, with the world's spawn, your bed and your friends marked. Scroll to zoom, drag to look around, and M or Escape closes it. It fills in as you go, a few hundred blocks around you, and it's kept on your computer with the world, or under the server's address when you join one.
- F5 goes round first person, third person from behind, and third person from in front looking back at you.
- `[` and `]` move the sun, and F1 shows the debug overlay.

## Commands

Press T or Enter, or / to start with the slash typed, to open the command bar, which shows what's been said lately. Enter runs the command, or says what you typed to everyone if it doesn't start with /, and Escape closes it.

- `/tp <x> <y> <z>` teleports there. `~` means where you are, and `~10` means ten blocks on from it.
- `/tp <x> <z>` teleports to just above the ground there.
- `/tp <player>` teleports to a player, and `/tp <player> <x> <y> <z>` teleports them.
- `/gamemode survival` or `/gamemode creative` (`s` or `c` for short) switches game mode, and `/gamemode <mode> <player>` switches someone else's. Survival can't fly, places only what it carries, and breaks blocks over time.
- `/seed` prints the world's seed. Give it as the seed of a new world to get the same world again.
- `/list` lists the players here, and `/msg <player> <text>` says something only they see.
- `/op <player>` and `/deop <player>` let a player run the commands that change things, or stop them (see below).
- `/help` lists the commands you may run.

### Building structures

In creative, operators can build structures and save them as templates, the way the game's own are made:

- `/pos1` and `/pos2` mark two opposite corners of a box, at the block you're looking at, where you stand, or at `<x> <y> <z>`. The box is drawn in orange.
- `/structure save <name> [floor]` saves the box as `structures/<name>.toml` in the world's directory. `floor` is the layer, counting from 0 at the bottom, that stands at ground level. Air in the box is cleared where the structure's built; a structure void (in the block picker) leaves what's there, and a loot marker marks where a chest of loot goes. Both show only in creative.
- `/structure place <name> [0|90|180|270]` builds a structure, turned, on the block you're looking at and selects it, to try it out or rework it, and `/structure list` lists those there are.

## Worlds

Worlds are saved every 30 seconds, when you quit to the title screen and when you quit the game, in a directory each, named as the world is, under your data directory: `~/.local/share/rustcraft/worlds` on Linux, `~/Library/Application Support/rustcraft/worlds` on macOS and `%APPDATA%\rustcraft\worlds` on Windows. Your settings (`settings.toml`) and saved servers (`servers.toml`) are kept beside them, in `rustcraft`.

## Playing together

To let friends join the world you're playing, choose Open to Friends in the game menu (Escape), which shows the address to give them and who's joined. Friends join it from Multiplayer, by that address, such as `192.168.1.20:25600`; the port can be left off if it's the default, 25600. Everyone needs the same version of rustcraft. Over the internet, the host forwards UDP port 25600 on their router.

Everyone plays by a name, shown over their head to the others: the one in Multiplayer, or else the name you're logged in as. A name already taken gets a number after it. The host's world keeps each friend's place, look and inventory under their name, so they come back where they left. A friend's game decides how they move, but the host puts them back if they go faster than they can or walk into a wall.

Anything typed into the command bar without a / is said to everyone, by your name; saying a lot at once is slowed to a line a second.

Commands that change the world or other players, `/tp`, `/gamemode`, `/op`, `/deop` and those for building structures, and winding the clock with `[` and `]`, are for operators: the host, always, and whoever they make one with `/op <name>`. The world keeps its operators by name in `world.toml` (`ops = ["alex"]`), and `/deop <name>` takes it away again, except from the host. Anyone may `/seed`, `/list`, `/msg` and chat.

It's early days: connections aren't encrypted nor players' names checked, so anyone who can reach your world can join under any name, an operator's included, and take their place. Only open your world to friends you trust.

## Running a server

To keep a world open on an always-on machine, with no screen or graphics card, run the dedicated server, `rustcraft-server`, from its own archive on the [Releases page](../../releases), or build it yourself without the game's window and rendering:

```
cargo build --release --bin rustcraft-server --no-default-features
./target/release/rustcraft-server --world <name> --port 25600
```

- `--world <name>` hosts that world, made if it doesn't exist yet, from the same worlds directory as the game (or give a path); `--port <port>` takes players on that UDP port, 25600 if it's left off; `--seed <seed>` gives a new world that seed. `RC_WORLD`, `RC_HOST` and `RC_SEED` do the same. `--help` prints these.
- Friends join it from Multiplayer as above. Over the internet, forward UDP port 25600 to the server.
- It logs to the terminal it's run in. Type into it to run any command, as an operator, with or without the /: `op <name>` makes the first operator, `list` names who's on, `say <text>` says something to everyone as Server, `help` lists the rest, and `stop`, or Ctrl-C, saves the world and stops. A command that acts on you, such as `tp 1 2 3`, needs a player named instead, as the server has none.
- It saves every 30 seconds, as a player leaves and as it stops, the same way the game does, so a world can be copied between the game and a server. The time of day runs on while nobody's on.

The same warning as above applies, more so: anyone who can reach the server can join under any name, an operator's included. Only run one for friends you trust, until names are checked.

## Developer options

For development, environment variables skip the menus: `RC_WORLD=<name>` plays that world straight away, made if it doesn't exist yet, with `RC_SEED=<seed>` if it's new; `RC_HOST=<port>` plays it (or `world`) open to friends on that port (`RC_HOST=` for the default); `RC_JOIN=<address>` joins a server. `RC_NAME=<name>` plays by that name when the settings have none, and `RC_TIME=<days>` starts the clock at that time of day.

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
