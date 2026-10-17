# rustcraft

Another Minecraft clone, originally written using [specs](https://github.com/amethyst/specs) and [glium](https://github.com/glium/glium) but recently migrated to Bevy.

```
cargo run --release
```

![Image of rustcraft](images/readme.jpg)

## Controls

- WASD to move, Shift to sprint, mouse to look.
- Space to jump, or to swim up in water.
- You start flying: Space and Ctrl move up and down, and you pass through blocks. F switches to walking and back.
- Left click breaks the block you're looking at, and right click places a block against it. Middle click picks the block you're looking at to place, which starts as stone.
- `[` and `]` move the sun, and F1 shows the debug overlay.

## Commands

Press T, or / to start with the slash typed, to open the command bar. Enter runs the command and Escape closes it.

- `/tp <x> <y> <z>` teleports there. `~` means where you are, and `~10` means ten blocks on from it.
- `/tp <x> <z>` teleports to just above the ground there.
- `/seed` prints the world's seed. Run with `RC_SEED=<seed>` and a new world's name to get the same world again.
- `/help` lists the commands.

## Worlds

Worlds are saved every 30 seconds and when you quit, in a directory each under your data directory: `~/.local/share/rustcraft/worlds` on Linux, `~/Library/Application Support/rustcraft/worlds` on macOS and `%APPDATA%\rustcraft\worlds` on Windows. Run with `RC_WORLD=<name>` to play another world, which is made if it doesn't exist yet; the one played otherwise is called `world`.

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
