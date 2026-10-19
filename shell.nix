# The libraries Bevy builds against and loads at run time, for building and running rustcraft on
# NixOS: `nix-shell --run 'cargo run --release'`. `nix-shell --arg headless true` adds a virtual
# display and software Vulkan for screenshots with no GPU, see `.claude/skills/screenshot`.
{ pkgs ? import <nixpkgs> {}, headless ? false }:
let
  libs = with pkgs; [ udev alsa-lib vulkan-loader libxkbcommon libx11 libxcursor libxi libxrandr ];
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config clang mold ]
    ++ pkgs.lib.optionals headless [ mesa xvfb-run imagemagick ];
  buildInputs = libs;
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libs;
}
