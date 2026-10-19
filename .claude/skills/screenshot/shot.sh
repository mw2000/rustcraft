#!/usr/bin/env bash
# Runs the release build on whatever display it's given, waits, captures the whole screen and
# stops the game. Run it under `xvfb-run -a` with software Vulkan when there's no GPU:
#   nix-shell --arg headless true --run "xvfb-run -a -s '-screen 0 1280x720x24' \
#     .claude/skills/screenshot/shot.sh \$PWD out.png 75 RC_SEED=7 RC_WORLD=shots"
# usage: shot.sh <repo dir> <out.png> <seconds> [NAME=value ...]
# The binary is taken from CARGO_TARGET_DIR when it is set, as when worktrees share one.
set -u
REPO=$1; OUT=$2; SECS=$3; shift 3
LOG=${RC_LOG:-$(dirname "$OUT")/rc-game.log}
# Mesa's software Vulkan (lavapipe), from the Nix store or the system's Vulkan drivers
ICD=$(ls /nix/store/*mesa*/share/vulkan/icd.d/lvp_icd.x86_64.json /usr/share/vulkan/icd.d/lvp_icd.x86_64.json 2>/dev/null | head -1)
cd "$REPO"
# assets beside the repo, not the binary; a world of its own, not the player's; running unfocused
env "$@" BEVY_ASSET_ROOT="$REPO" ${ICD:+VK_ICD_FILENAMES=$ICD} XDG_DATA_HOME=${XDG_DATA_HOME:-$(dirname "$OUT")/xdg} \
  RC_STATS=1 WGPU_BACKEND=vulkan "${CARGO_TARGET_DIR:-./target}/release/rustcraft" > "$LOG" 2>&1 &
PID=$!
sleep "$SECS"
import -window root "$OUT"
kill $PID; wait $PID 2>/dev/null
tail -5 "$LOG"
