#!/usr/bin/env python3
"""Draw the hand-made tiles (those with `drawn=True` in TILES) into out/<Tile>.png, for pack.py. Usage: draw.py [tile ...] [--seed N]

Built blocks, ores and plants need hard edges (mortar, board seams, ore deposits, petals) or cut-outs that SD can't
draw at 16px, so they're drawn here instead: each tile is a map of indices into a few shades, the way the generated tiles end up after
their 4-colour quantise. Shades come from tiles already in the atlas, so the new ones sit beside them."""
import argparse

import numpy as np
from PIL import Image

from generate import HERE, LUM, OUT, TILES

ATLAS = HERE / "../../../assets/textures/blocks.png"


def source(name):
    """A tile of the atlas as a float RGB array."""
    i = [t["name"] for t in TILES].index(name)
    return np.asarray(Image.open(ATLAS).convert("RGB"), float)[:, i * 16:(i + 1) * 16]


def shades(name, contrast=1.0):
    """A tile's colours, darkest first, spread `contrast` times further from their mean."""
    c = np.unique(source(name).reshape(-1, 3), axis=0)
    c = c[np.argsort(c @ LUM)]
    return c.mean(0) + (c - c.mean(0)) * contrast


def noise(rng, blur=(1, 1)):
    """Seamless 16x16 noise scaled to 0..1, box-blurred `blur` times along (y, x) with wrap-around."""
    a = rng.random((16, 16))
    for axis, n in enumerate(blur):
        for _ in range(n):
            a = (a + np.roll(a, 1, axis) + np.roll(a, -1, axis)) / 3
    return (a - a.min()) / (a.max() - a.min())


def levels(a, n):
    """Noise to n levels with equal counts, as the quantised tiles have."""
    return np.searchsorted(np.quantile(a, np.linspace(0, 1, n + 1)[1:-1]), a)


def sprite(rows, colours):
    """Paint a small picture given as strings, one char per pixel, '.' left clear: a list of (y, x, colour)."""
    return [(y, x, colours[ch]) for y, row in enumerate(rows) for x, ch in enumerate(row) if ch != "."]


def planks(rng):
    wood = shades("LogEnd")[:3]
    seam = shades("LogBark")[-1]  # the bark's lightest brown, dark against the boards
    grain = levels(noise(rng, (0, 4)) + np.repeat(rng.random(4) * 0.4, 4)[:, None], 3)  # streaks along each board
    out = wood[grain]
    out[3::4] = seam  # four boards, four rows each
    for board, x in enumerate([4, 12, 1, 9]):  # board ends staggered, as laid
        out[board * 4:board * 4 + 3, x] = seam
    return out


def cobblestone(rng):
    stone = shades("Stone", 1.8)
    # rounded stones are the cells of jittered points, with mortar where two cells meet
    points = (np.stack(np.meshgrid(np.arange(3), np.arange(3)), -1).reshape(-1, 2) + rng.uniform(0.15, 0.85, (9, 2))) * 16 / 3
    yx = np.stack(np.mgrid[:16, :16], -1)[:, :, None] + 0.5
    d = (yx - points + 8) % 16 - 8  # wrapped offset to every point, so the tile is seamless
    dist = np.linalg.norm(d, axis=-1)
    order = np.sort(dist, -1)
    nearest = np.take_along_axis(d, dist.argmin(-1)[..., None, None], 2)[:, :, 0]
    # each stone is lit from the upper left and darker towards its rim, so it looks rounded
    gap = order[..., 1] - order[..., 0]
    light = -(nearest[..., 0] + nearest[..., 1]) / 4 + np.minimum(gap, 4) / 3 + noise(rng) * 0.5 + rng.random(9)[dist.argmin(-1)] * 0.5
    out = stone[1:][levels(light, 3)]
    out[gap < 1.2] = stone[0] * 0.75
    return out


def stone_bricks(rng):
    stone = shades("Stone", 1.6)
    out = stone[1:3][levels(noise(rng), 2)]
    y, x = np.mgrid[:16, :16]
    bx = (x + y // 8 * 8) % 16  # alternate rows of bricks are offset by half a brick
    out[(y % 8 == 0) | (bx == 0)] = stone[3]  # a lit top and left edge to each brick
    out[(y % 8 == 6) & (bx != 0)] = stone[0]  # its shaded underside
    out[(y % 8 == 7) | (bx == 15)] = stone[0] * 0.7  # mortar
    return out


def bricks(rng):
    clay = np.array([(146, 78, 62), (136, 70, 56), (126, 64, 52)], float)
    mortar = np.array([(150, 143, 134), (138, 131, 123)], float)
    y, x = np.mgrid[:16, :16]
    shifted = x + (y // 4) % 2 * 4  # alternate rows of bricks are offset by half a brick
    brick = (y // 4) * 2 + shifted % 16 // 8  # which of the 8 bricks a texel is in
    out = clay[np.clip(rng.integers(0, 2, 8)[brick] + (noise(rng) > 0.75), 0, 2)]
    joint = (y % 4 == 3) | (shifted % 8 == 7)
    out[joint] = mortar[(noise(rng) > 0.6).astype(int)][joint]
    return out


STEPS = [(-1, 0), (1, 0), (0, -1), (0, 1)]


def blob(rng, size):
    """A lumpy cluster of `size` texels, grown from one by adding neighbours, more often where they'd fill a nook
    than stick out, so it's a lump rather than a worm."""
    cells = [(0, 0)]
    while len(cells) < size:
        y, x = cells[rng.integers(len(cells))]
        dy, dx = STEPS[rng.integers(4)]
        new = (y + dy, x + dx)
        touching = sum((new[0] + sy, new[1] + sx) in cells for sy, sx in STEPS)
        if new not in cells and rng.random() < touching / 2:
            cells.append(new)
    return cells


def ore(rng, lit, main, shade, sizes, accent=None):
    """The Stone tile with mineral deposits of these sizes in it, at random spots kept apart so they read as separate
    lumps. Each is lit on its upper-left texels and shaded on its lower-right ones, so it sits in the rock."""
    out = source("Stone").copy()
    centres = []
    for size in sizes:
        while True:  # wrapped distance, so deposits don't run together across the seams either
            c = rng.integers(0, 16, 2)
            if all((np.abs((c - o + 8) % 16 - 8) > 4).any() for o in centres):
                break
        centres.append(c)
        cells = set(blob(rng, size))
        # verdigris: a patch on a big copper deposit, not a deposit of its own
        teal = set(blob(rng, rng.integers(1, 4))) & cells if accent is not None and size > 4 else set()
        for y, x in cells:
            colour = main
            if (y - 1, x) not in cells and (y, x - 1) not in cells:
                colour = lit
            elif (y + 1, x) not in cells and (y, x + 1) not in cells:
                colour = shade
            if (y, x) in teal:
                colour = accent
            out[(c[0] + y) % 16, (c[1] + x) % 16] = colour
    return out


def coal_ore(rng):
    return ore(rng, (62, 62, 66), (36, 36, 38), (22, 22, 24), [rng.integers(10, 14), rng.integers(4, 7), rng.integers(2, 4)])


def copper_ore(rng):
    # orange copper, with patches gone teal with verdigris
    return ore(rng, (226, 146, 88), (190, 104, 60), (138, 72, 46), [rng.integers(8, 12), rng.integers(3, 6), rng.integers(2, 4)], accent=(82, 164, 140))


def iron_ore(rng):
    return ore(rng, (232, 204, 184), (204, 164, 140), (150, 114, 98), [rng.integers(7, 11), rng.integers(3, 6), 2, rng.integers(2, 4)])


def crafting_table_top(rng):
    wood = shades("LogEnd")
    frame = shades("LogBark")
    out = wood[1:][levels(noise(rng, (0, 3)), 3)]
    y, x = np.mgrid[:16, :16]
    out[(x % 5 == 0) | (y % 5 == 0)] = frame[2]  # a 3x3 grid of work squares
    out[(x == 0) | (x == 15) | (y == 0) | (y == 15)] = frame[1]  # the table's rim
    return out


def crafting_table_side(rng):
    out = planks(rng)
    frame = shades("LogBark")
    out[0:2] = shades("LogEnd")[2:][levels(noise(rng, (0, 3))[0:2], 2)]  # the edge of the top
    out[2] = frame[1]
    grey, dark = (150, 150, 152), (96, 96, 100)
    handle = frame[0]
    # a saw hanging on the left and a hammer on the right
    saw = sprite(["hh......", "hhgggggg", "hhgggggg", "hh.gggg.", "...d.d.d"], dict(h=handle, g=grey, d=dark))
    hammer = sprite(["ggggg", "gggdd", "..h..", "..h..", "..h..", "..h..", "..h.."], dict(h=handle, g=grey, d=dark))
    for dy, dx, c in saw:
        out[6 + dy, 1 + dx] = c
    for dy, dx, c in hammer:
        out[5 + dy, 10 + dx] = c
    return out


def furnace_side(rng):
    out = cobblestone(rng)
    stone = shades("Stone", 1.6)
    out[6:15, 3:13] = stone[3]  # a dressed stone surround
    out[7:14, 4:12] = stone[2]
    out[8:13, 5:11] = (26, 25, 25)  # the dark mouth, with a sooty floor
    out[12, 5:11] = (44, 40, 38)
    return out


def clear():
    """A tile with nothing in it yet, as RGBA, for plants drawn on transparency."""
    return np.zeros((16, 16, 4))


def paint(out, y, x, colour):
    out[y, x] = (*colour, 255)


def tall_grass(rng):
    """Greyscale blades, tinted per climate like the grass top, whose shades they take: a darker one
    down each blade's middle, the lightest at its tips, and the darkest near the ground."""
    grey = shades("GrassTop", 1.4)
    out = clear()
    for x0 in [1, 3, 5, 7, 8, 10, 12, 14]:
        height = rng.integers(7, 15)
        lean = rng.uniform(-0.35, 0.35)
        for i in range(height):  # from the ground up
            x = int(round(x0 + lean * i * i / height))
            if not 0 <= x < 16:
                break
            shade = grey[0] if i < 2 else grey[-1] if i >= height - 2 else grey[1 + (i + x0) % (len(grey) - 2)]
            paint(out, 15 - i, x, shade)
    return out


STEM = [(58, 98, 30), (76, 124, 40), (98, 148, 52)]  # untinted green, darkest first


def flower(rng, head, colours, stem_height=8, leaves=((11, -1), (13, 1))):
    """A stem with a leaf or two, under a head drawn as a picture of `colours` keyed by its chars."""
    out = clear()
    x = 7 + rng.integers(0, 2)
    for y in range(16 - stem_height, 16):
        paint(out, y, x, STEM[1])
    for y, side in leaves:  # a leaf sticking out from the stem, two texels long and rising
        paint(out, y, x + side, STEM[2])
        paint(out, y - 1, x + 2 * side, STEM[2])
        paint(out, y + 1, x + side, STEM[0])
    top = 16 - stem_height - len(head)
    for dy, dx, c in sprite(head, colours):
        paint(out, top + dy, x - len(head[0]) // 2 + dx, c)
    return out


def poppy(rng):
    return flower(rng, [".rrr.", "rRrrr", "rrkrr", "rrrrd", ".ddd."],
                  dict(R=(236, 84, 64), r=(204, 40, 32), d=(150, 24, 22), k=(40, 30, 28)))


def dandelion(rng):
    return flower(rng, [".y.y.", "yYyyy", ".yyy.", "..g.."], dict(Y=(255, 236, 110), y=(240, 200, 40), g=STEM[0]),
                  stem_height=7, leaves=((13, -1), (14, 1)))


def cornflower(rng):
    return flower(rng, ["b.b.b", ".BbB.", "bbdbb", ".bbb.", "..g.."], dict(B=(130, 160, 255), b=(70, 100, 220), d=(40, 50, 140), g=STEM[0]),
                  stem_height=7)


DRAW = dict(Planks=planks, Cobblestone=cobblestone, StoneBricks=stone_bricks, Bricks=bricks, CoalOre=coal_ore, CopperOre=copper_ore,
            IronOre=iron_ore, CraftingTableTop=crafting_table_top, CraftingTableSide=crafting_table_side, FurnaceSide=furnace_side,
            TallGrass=tall_grass, Poppy=poppy, Dandelion=dandelion, Cornflower=cornflower)
assert set(DRAW) == {t["name"] for t in TILES if t.get("drawn")}, "DRAW and the drawn rows of TILES differ"

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("tiles", nargs="*", help="tile names, default all drawn tiles")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    for name in a.tiles or DRAW:
        rgb = DRAW[name](np.random.default_rng([a.seed, list(DRAW).index(name)]))
        # plants are drawn as RGBA, on transparency, and every other tile as RGB
        Image.fromarray(np.clip(np.asarray(rgb).round(), 0, 255).astype("uint8")).convert("RGBA").save(OUT / f"{name}.png")
        print("drew", name)
