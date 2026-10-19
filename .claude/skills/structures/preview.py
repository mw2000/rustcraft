"""Sketches a structure template from its four sides, and cut away at the layers given, as
isometric blocks coloured by the atlas's tiles, for a quick look before running the game.
Choices are made at random from the seed given; stairs and slabs are drawn as their nearest
boxes, thin blocks as posts and the sunk layers are left out but for the cutaways. Needs
Pillow and NumPy. Village pieces (`assets/structures/village/`) are drawn with their ground
layer, the one under their floor, which they lay themselves, and their markers as coloured
dots in the cutaways: entrance red, bed blue, work orange, gather purple, animal pink.

    python3 .claude/skills/structures/preview.py assets/structures/watchtower.toml /tmp/w.png 1 3,16 [scale]
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
for t in ("Leaves", "Vines", "GrassTop", "TallGrass", "Fern", "ShortGrass", "WildGrass"):
    MEAN[t] = MEAN[t] * GREEN * 1.3

SPECIAL = {
    "log": ("LogBark", "LogEnd"), "sandstone": ("SandstoneSide", "SandstoneTop"),
    "grass": ("GrassSide", "GrassTop"), "snow": ("Snow", "Snow"), "furnace": ("FurnaceSide", "Stone"),
    "crafting_table": ("CraftingTableSide", "CraftingTableTop"), "bed": ("BedSide", "BedTop"),
    "chest": ("ChestSide", "ChestTop"), "barrel": ("BarrelSide", "BarrelTop"), "door": ("DoorBottom", "DoorTop"),
    "bookshelf": ("Bookshelf", "Planks"), "stone_brick": ("StoneBricks", "StoneBricks"),
    "dark_planks": ("DarkPlanks", "DarkPlanks"), "path": ("Dirt", "PathTop"),
    "farmland": ("Dirt", "FarmlandTop"), "wool_block": ("Wool", "Wool"),
}
MARKER = {"entrance": (230, 40, 40), "bed": (40, 90, 230), "work": (250, 150, 20),
          "gather": (160, 60, 200), "animal": (250, 120, 200)}
PLANTS = ("tall_grass", "short_grass", "wild_grass", "fern", "poppy", "dandelion", "cornflower",
          "ripe_wheat") + tuple(f"wheat_{i}" for i in range(7))


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
    if name in ("ladder", "vines") or (name == "trapdoor" and "open=true" in state):
        return "panel"
    if name == "trapdoor":
        return "plate" if "half=top" in state else "floorplate"
    if name in PLANTS:
        return "plant"
    if name.endswith(("_fence", "_wall")) or name in ("glass_pane", "torch", "lantern"):
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
    # as the game draws them: one number for every whole choice, taken modulo its total, so
    # choices with different totals vary apart
    whole_draw = rng.getrandbits(64)
    grid = {}
    for y, layer in enumerate(t["layers"]):
        rows = [r for r in layer.split("\n") if r]
        for z, row in enumerate(rows):
            for x, k in enumerate(row):
                if k in ".-":
                    continue
                opts, whole = options(t["palette"][k])
                total = sum(w for _, w in opts)
                r = whole_draw % total if whole else rng.random() * total
                for b, w in opts:
                    if r < w:
                        break
                    r -= w
                if b == "air":
                    continue
                name, _, state = b.partition("[")
                grid[(x, y, z)] = (name, state)
    marks = {}
    for kind, pts in t.get("markers", {}).items():
        for x, y, z in pts:
            if kind == "loot":
                grid[(x, y, z)] = ("chest", "")
            elif kind in MARKER:
                marks[(x, y, z)] = MARKER[kind]
            else:
                grid[(x, y, z)] = ("loot_marker", "")
    return grid, (sx, sy, sz), t["floor"], marks


def render(grid, size, s=10, rot=0, cut=None, ground=None, marks=None):
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
    dots = {}
    for (x, y, z), c in (marks or {}).items():
        if cut is None or y > cut + 1:
            continue
        tx, tz, w, d = x, z, sx, sz
        for _ in range(rot):
            tx, tz = d - 1 - tz, tx
            w, d = d, w
        dots[(tx, y, tz)] = c
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
        elif sh == "plant":
            x0, x1, z0, z1, y1 = x + 0.3, x + 0.7, z + 0.3, z + 0.7, y + 0.8
        elif sh in ("plate", "floorplate"):
            y0, y1 = (y + 0.8, y + 1) if sh == "plate" else (y, y + 0.2)
        elif sh == "panel":
            # against the side opposite its facing, turned with the view
            f = re.search(r"facing=(\w+)", state)
            side = {"north": "south", "south": "north", "east": "west", "west": "east"}[f.group(1) if f else "north"]
            for _ in range(rot):
                side = {"north": "east", "east": "south", "south": "west", "west": "north"}[side]
            if side == "north":
                z1 = z + 0.2
            elif side == "south":
                z0 = z + 0.8
            elif side == "west":
                x1 = x + 0.2
            else:
                x0 = x + 0.8
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
    for (x, y, z), c in dots.items():
        p = P(x + 0.5, y + 0.1, z + 0.5)
        dr.ellipse([p[0] - 4, p[1] - 4, p[0] + 4, p[1] + 4], fill=c, outline=(0, 0, 0))
    return im


if __name__ == "__main__":
    path, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    cuts = [int(c) for c in sys.argv[4].split(",")] if len(sys.argv) > 4 else []
    scale = int(sys.argv[5]) if len(sys.argv) > 5 else 10
    grid, size, floor, marks = load(path, seed)
    # a piece lays its own ground, so it's shown with that
    piece = "piece" in tomllib.load(open(path, "rb"))
    ground = floor - 1 if piece else floor
    views = [render(grid, size, s=scale, rot=r, ground=ground) for r in range(4)]
    views += [render(grid, size, s=scale, rot=0, cut=c, ground=ground, marks=marks) for c in cuts]
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
