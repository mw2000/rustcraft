---
name: screenshot
description: Run the real game with no GPU (a virtual display and Mesa's software Vulkan) and capture screenshots, to check how something looks — UI, a structure, lighting, creatures. Use whenever a change is visual and needs seeing, since tests can't show it.
---

# Screenshots without a GPU

The game runs at 1–14 fps under `xvfb-run` with lavapipe (Mesa's software Vulkan), which is
slow but renders everything the real game does, so it's how visual work gets checked.

```
cargo build --release
nix-shell --arg headless true --run "xvfb-run -a -s '-screen 0 1280x720x24' \
  .claude/skills/screenshot/shot.sh \$PWD /tmp/shots/out.png 75 RC_SEED=7 RC_WORLD=shots"
```

Then read the PNG. Off NixOS, install Xvfb, ImageMagick and Mesa's Vulkan drivers
(`mesa-vulkan-drivers` on Debian/Ubuntu) and run the same `xvfb-run` line without `nix-shell`.

- `RC_WORLD` skips the title screen into that world; `RC_SEED` picks a new world's seed;
  `RC_TIME` sets the time of day in days (0 midnight, 0.5 noon). `RC_WEATHER` (`clear`, `rain` or
  `storm`) opens the world in that weather, which takes about 12 s to come on. The world is made under the
  screenshot's own folder (`XDG_DATA_HOME`), never the player's.
- `BEVY_ASSET_ROOT` must be the repo when running `target/release/rustcraft` directly, or Bevy
  can't find its shaders and the world draws as a flat tan plane.
- Always `xvfb-run -a`, so parallel runs get their own display; each run's log goes beside its
  PNG (`rc-game.log`), so check it for errors.
- Wait 60–90 s for terrain to load. You can't send input, so to look at something specific add
  a temporary hook (an env var read at startup that places the camera, opens a screen or spawns
  a creature) and remove it before committing.
- Two instances (a host with `RC_HOST=` and a joiner with `RC_JOIN=127.0.0.1`) show multiplayer.
