"""Sketches a structure template from its four sides, and cut away at the layers given, as
isometric blocks coloured by the atlas's tiles, for a quick look before running the game.
Choices are made at random from the seed given; stairs and slabs are drawn as their nearest
boxes, thin blocks as posts and the sunk layers are left out but for the cutaways. Needs
Pillow and NumPy.

    python3 .claude/skills/structures/preview.py assets/structures/watchtower.toml /tmp/w.png 1 3,16
"""
import random
import re
import sys
import tomllib

import numpy as np
from PIL import Image, ImageDraw

from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
TILES = re.findall(r"^\s+(\w+),", open(f"{REPO}/src/block.rs").read().split("pub enum Tile {")[1].split("}")[0], re.M)
atlas = np.asarray(Image.open(f"{REPO}/assets/textures/blocks.png").convert("RGB")).astype(float)
MEAN = {t: atlas[:, i * 16:(i + 1) * 16].reshape(-1, 3).mean(0) for i, t in enumerate(TILES)}
GREEN = np.array([0.45, 0.75, 0.3])
for t in ("Leaves", "Vines", "GrassTop", "TallGrass", "Fern"):
    MEAN[t] = MEAN[t] * GREEN * 1.3

SPECIAL = {
    "log": ("LogBark", "LogEnd"), "sandstone": ("SandstoneSide", "SandstoneTop"),
    "grass": ("GrassSide", "GrassTop"), "snow": ("Snow", "Snow"), "furnace": ("FurnaceSide", "Stone"),
    "crafting_table": ("CraftingTableSide", "CraftingTableTop"), "bed": ("BedSide", "BedTop"),
    "chest": ("ChestSide", "ChestTop"), "barrel": ("BarrelSide", "BarrelTop"), "door": ("DoorBottom", "DoorTop"),
    "bookshelf": ("Bookshelf", "Planks"), "stone_brick": ("StoneBricks", "StoneBricks"),
    "dark_planks": ("DarkPlanks", "DarkPlanks"),
}


def camel(name):
    return "".join(p.capitalize() for p in name.split("_"))


def tiles(name):
    base = re.sub(r"_(stairs|slab|wall|fence)$", "", name)
    if base in SPECIAL:
        return SPECIAL[base]
    t = camel(base)
    if t not in MEAN:
        t = camel(base + "s") if camel(base + "s") in MEAN else "Stone"
    return (t, t)


def shape(name, state):
    if name.endswith("_slab"):
        return "top" if "half=top" in state else "slab"
    if name.endswith(("_fence", "_wall")) or name in ("glass_pane", "torch", "lantern", "ladder", "vines", "tall_grass", "fern", "poppy", "dandelion", "cornflower", "trapdoor"):
        return "thin"
    return "cube"


def options(choice):
    """A palette entry's blocks and weights, and whether it's chosen once for the whole."""
    whole = "/" in choice
    opts = []
    for o in choice.split("/" if whole else "|"):
        b, _, w = o.partition("*")
        opts.append((b, int(w or 1)))
    return opts, whole


def load(path, seed):
    t = tomllib.load(open(path, "rb"))["template"]
    sx, sy, sz = t["size"]
    rng = random.Random(seed)
    whole_draw = rng.random()
    grid = {}
    for y, layer in enumerate(t["layers"]):
        rows = [r for r in layer.split("\n") if r]
        for z, row in enumerate(rows):
            for x, k in enumerate(row):
                if k in ".-":
                    continue
                opts, whole = options(t["palette"][k])
                total = sum(w for _, w in opts)
                r = (whole_draw if whole else rng.random()) * total
                for b, w in opts:
                    if r < w:
                        break
                    r -= w
                if b == "air":
                    continue
                name, _, state = b.partition("[")
                grid[(x, y, z)] = (name, state)
    for kind, pts in t.get("markers", {}).items():
        for x, y, z in pts:
            grid[(x, y, z)] = ("chest", "") if kind == "loot" else ("loot_marker", "")
    return grid, (sx, sy, sz), t["floor"]


def render(grid, size, s=10, rot=0, cut=None, ground=None):
    sx, sy, sz = size
    cells = {}
    for (x, y, z), v in grid.items():
        if cut is not None and y > cut:
            continue
        if cut is None and ground is not None and y < ground:
            continue
        tx, tz = x, z
        w, d = sx, sz
        for _ in range(rot):
            tx, tz = d - 1 - tz, tx
            w, d = d, w
        cells[(tx, y, tz)] = v
    W = (sx + sz) * s + 40
    H = (sx + sz) * s // 2 + sy * s + 40
    im = Image.new("RGB", (W, H), (150, 185, 225))
    dr = ImageDraw.Draw(im)
    ox, oy = sz * s + 20 if rot % 2 == 0 else sx * s + 20, sy * s + 20

    def P(x, y, z):
        return (ox + (x - z) * s, oy + (x + z) * s / 2 - y * s)

    if ground is not None:
        g = (95, 130, 70)
        for x in range(-3, max(sx, sz) + 3):
            for z in range(-3, max(sx, sz) + 3):
                dr.polygon([P(x, ground, z), P(x + 1, ground, z), P(x + 1, ground, z + 1), P(x, ground, z + 1)], fill=g, outline=(85, 118, 62))
    opaque = lambda c: c in cells and shape(*cells[c]) == "cube"
    for (x, y, z) in sorted(cells, key=lambda c: (c[0] + c[2] + c[1], c[1])):
        name, state = cells[(x, y, z)]
        side, top = tiles(name)
        cs, ct = MEAN[side], MEAN[top]
        sh = shape(name, state)
        y0, y1 = (y, y + 0.5) if sh == "slab" else (y + 0.5, y + 1) if sh == "top" else (y, y + 1)
        x0, x1, z0, z1 = x, x + 1, z, z + 1
        if sh == "thin":
            x0, x1, z0, z1 = x + 0.35, x + 0.65, z + 0.35, z + 0.65
        col = lambda c, k: tuple(int(min(255, v * k)) for v in c)
        if not opaque((x, y + 1, z)) or sh != "cube":
            dr.polygon([P(x0, y1, z0), P(x1, y1, z0), P(x1, y1, z1), P(x0, y1, z1)], fill=col(ct, 1.0))
        if not opaque((x + 1, y, z)) or sh != "cube":
            dr.polygon([P(x1, y0, z0), P(x1, y1, z0), P(x1, y1, z1), P(x1, y0, z1)], fill=col(cs, 0.8))
        if not opaque((x, y, z + 1)) or sh != "cube":
            dr.polygon([P(x0, y0, z1), P(x1, y0, z1), P(x1, y1, z1), P(x0, y1, z1)], fill=col(cs, 0.62))
        if name in ("lantern", "torch"):
            c = P(x + 0.5, y + 0.5, z + 0.5)
            dr.ellipse([c[0] - 3, c[1] - 3, c[0] + 3, c[1] + 3], fill=(255, 220, 90))
    return im


if __name__ == "__main__":
    path, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    cuts = [int(c) for c in sys.argv[4].split(",")] if len(sys.argv) > 4 else []
    grid, size, floor = load(path, seed)
    views = [render(grid, size, rot=r, ground=floor) for r in range(4)]
    views += [render(grid, size, rot=0, cut=c, ground=floor) for c in cuts]
    W = sum(v.width for v in views[:4])
    rows = [views[:4], views[4:]]
    H = sum(max((v.height for v in r), default=0) for r in rows)
    sheet = Image.new("RGB", (max(W, sum(v.width for v in views[4:])), H), (255, 255, 255))
    y = 0
    for r in rows:
        x = 0
        for v in r:
            sheet.paste(v, (x, y))
            x += v.width
        y += max((v.height for v in r), default=0)
    sheet.save(out)
