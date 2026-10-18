#!/usr/bin/env python3
"""Draw the hand-made tiles (those with `drawn=True` in TILES) into out/<Tile>.png, for pack.py. Usage: draw.py [tile ...] [--seed N]

Built blocks, ores and plants need hard edges (mortar, board seams, ore deposits, petals) or cut-outs that SD can't
draw at 16px, so they're drawn here instead: each tile is a map of indices into a few shades, the way the generated tiles end up after
their 4-colour quantise. Shades come from tiles already in the atlas, so the new ones sit beside them."""
import argparse

import numpy as np
from PIL import Image

import flora
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


def boards(rng, wood, seam):
    """Four boards across the tile in three shades of `wood`, darkest first, with `seam` between them."""
    grain = levels(noise(rng, (0, 4)) + np.repeat(rng.random(4) * 0.4, 4)[:, None], 3)  # streaks along each board
    out = np.asarray(wood, float)[grain]
    out[3::4] = seam  # four boards, four rows each
    for board, x in enumerate([4, 12, 1, 9]):  # board ends staggered, as laid
        out[board * 4:board * 4 + 3, x] = seam
    return out


def planks(rng):
    return boards(rng, shades("LogEnd")[:3], shades("LogBark")[-1])  # the bark's lightest brown, dark against the boards


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


def gold_ore(rng):
    # fat nuggets of gold, the far ores' deposits bigger than the common ones'
    return ore(rng, (255, 238, 150), (234, 190, 56), (166, 118, 30), [rng.integers(10, 14), rng.integers(5, 8), rng.integers(3, 5), 2])


def jade_ore(rng):
    return ore(rng, (156, 226, 170), (72, 172, 112), (34, 110, 70), [rng.integers(10, 14), rng.integers(5, 8), rng.integers(2, 4)])


def frost_crystal_ore(rng):
    """The Stone tile with pale blue crystals jutting out of it, each a few texels along one diagonal or the
    other, lit at its upper end and with a glint at its tip, so they read as shards rather than lumps."""
    out = source("Stone").copy()
    ice = [(86, 146, 196), (150, 210, 240), (214, 242, 255)]
    for cy, cx, length in [(3, 3, 5), (4, 11, 4), (11, 6, 5), (12, 13, 3), (8, 1, 3)]:
        cy, cx = cy + rng.integers(-1, 2), cx + rng.integers(-1, 2)
        dx = rng.choice([-1, 1])
        for i in range(length):  # from its root up
            y, x = (cy - i) % 16, (cx + dx * i) % 16
            out[y, x] = ice[2] if i == length - 1 else ice[1]
            out[(y + 1) % 16, x] = ice[0]  # its shaded underside, the width of the shard
    return out


def ember_ore(rng):
    """Darker, scorched stone with glowing flecks in it, bright at their hearts, as it gives off a little light."""
    out = source("Stone") * 0.62
    centres = []
    for size in [rng.integers(5, 8), 4, 3, 3, 2, 2]:
        while True:
            c = rng.integers(0, 16, 2)
            if all((np.abs((c - o + 8) % 16 - 8) > 3).any() for o in centres):
                break
        centres.append(c)
        cells = blob(rng, size)
        for i, (y, x) in enumerate(cells):  # the first texel of a fleck its heart, the last its cooling edge
            colour = (255, 226, 120) if i == 0 else (176, 54, 22) if i >= len(cells) - 1 > 1 else (244, 124, 40)
            out[(c[0] + y) % 16, (c[1] + x) % 16] = colour
    return out


def bevel(out, lit, shade, width=1):
    """A raised rim round the tile, lit along the top and left and shaded along the bottom and right."""
    out[:width] = lit
    out[:, :width] = lit
    out[16 - width:] = shade
    out[:, 16 - width:] = shade
    return out


def gold_block(rng):
    gold = np.array([(196, 148, 40), (226, 182, 60), (240, 204, 86)], float)
    out = gold[levels(noise(rng, (0, 3)), 3)]  # brushed along the rows
    y, x = np.mgrid[:16, :16]
    out[(x + y) % 16 == 11] = (255, 236, 150)  # a glint across it
    out[(x + y) % 16 == 12] = (250, 222, 120)
    return bevel(out, (255, 236, 146), (150, 104, 26))


def frost_crystal_block(rng):
    """Crystal faces packed together, the cells of jittered points as cobblestone's stones are, each a flat shade of
    pale blue, with bright edges where two faces meet."""
    blue = np.array([(120, 182, 222), (150, 206, 238), (176, 222, 246), (200, 236, 252)], float)
    points = (np.stack(np.meshgrid(np.arange(3), np.arange(3)), -1).reshape(-1, 2) + rng.uniform(0.2, 0.8, (9, 2))) * 16 / 3
    yx = np.stack(np.mgrid[:16, :16], -1)[:, :, None] + 0.5
    dist = np.linalg.norm((yx - points + 8) % 16 - 8, axis=-1)
    order = np.sort(dist, -1)
    out = blue[rng.integers(0, 4, 9)[dist.argmin(-1)]]
    out[order[..., 1] - order[..., 0] < 0.9] = (232, 248, 255)
    return out


def polished_jade(rng):
    jade = np.array([(46, 132, 86), (62, 152, 102), (82, 172, 120)], float)
    out = jade[levels(noise(rng, (2, 2)), 3)]  # soft clouds in the stone
    vein = noise(rng, (1, 1))
    out[(vein > 0.47) & (vein < 0.53)] = (140, 210, 166)  # a pale vein winding through it
    return bevel(out, (120, 200, 150), (28, 92, 58))


def bright_steel_block(rng):
    """Four plates of pale steel with a cool sheen, each bevelled, riveted at the corners where they meet."""
    steel = np.array([(170, 182, 196), (192, 204, 216), (212, 222, 232)], float)
    out = steel[levels(noise(rng, (0, 2)), 3)]
    y, x = np.mgrid[:16, :16]
    out[(y % 8 == 0) | (x % 8 == 0)] = (240, 246, 252)  # each plate's lit top and left edge
    out[(y % 8 == 7) | (x % 8 == 7)] = (120, 130, 146)  # and its shaded bottom and right
    for ry, rx in [(1, 1), (1, 6), (6, 1), (6, 6)]:  # rivets in each plate's corners
        out[(y % 8 == ry) & (x % 8 == rx)] = (232, 214, 150)  # warm, from the far metal blended in
    return out


def ember_lamp(rng):
    """Embers glowing behind four panes of frost crystal, hottest at the middle, in a pale crystal frame."""
    y, x = np.mgrid[:16, :16]
    heat = 1 - np.hypot(y - 7.5, x - 7.5) / 8 + noise(rng) * 0.3
    glow = np.array([(214, 96, 30), (244, 150, 52), (255, 204, 96), (255, 240, 170)], float)
    out = glow[levels(heat, 4)]
    out[(x == 7) | (x == 8) | (y == 7) | (y == 8)] = (196, 230, 248)  # the frame between the panes
    return bevel(out, (214, 242, 255), (110, 160, 200), width=2)


def cells(rng, n=3, jitter=(0.15, 0.85)):
    """Jittered points on an n x n grid, wrapped so the tile is seamless: each texel's nearest point, and how much
    nearer it is than the next, small where two cells meet."""
    points = (np.stack(np.meshgrid(np.arange(n), np.arange(n)), -1).reshape(-1, 2) + rng.uniform(*jitter, (n * n, 2))) * 16 / n
    yx = np.stack(np.mgrid[:16, :16], -1)[:, :, None] + 0.5
    dist = np.linalg.norm((yx - points + 8) % 16 - 8, axis=-1)
    order = np.sort(dist, -1)
    return dist.argmin(-1), order[..., 1] - order[..., 0]


def basalt_side(rng):
    """Dark columns standing side by side, four texels wide, each lit down its left edge, cracked across now and
    then, with dark joints between them."""
    rock = np.array([(58, 56, 62), (70, 68, 74), (82, 80, 86), (98, 96, 102)], float)
    y, x = np.mgrid[:16, :16]
    out = rock[1:3][levels(noise(rng, (2, 0)), 2)]  # streaked up and down the columns
    out[x % 4 == 1] = rock[3]
    out[x % 4 == 0] = rock[0] * 0.75
    for column in range(4):  # a crack across each, at its own height
        out[rng.integers(0, 16), column * 4 + 1:column * 4 + 4] = rock[0]
    return out


def basalt_top(rng):
    """The ends of the columns, cut across: rough polygons of dark rock with dark joints."""
    rock = np.array([(60, 58, 64), (72, 70, 76), (84, 82, 88)], float)
    nearest, gap = cells(rng, 3, (0.25, 0.75))
    out = rock[rng.integers(0, 3, 9)[nearest]]
    out[gap < 1.0] = rock[0] * 0.7
    return out


def magma(rng):
    """Molten rock glowing between plates of dark crust, yellow-white where it's hottest, so it reads as glowing
    orange from far off."""
    glow = np.array([(206, 64, 18), (240, 110, 28), (255, 162, 48), (255, 222, 120)], float)
    crust = np.array([(58, 30, 26), (82, 42, 30)], float)
    nearest, gap = cells(rng, 2, (0.1, 0.9))
    out = glow[levels(noise(rng) + np.clip(2.0 - gap, 0, 2.0), 4)]
    plate = (gap > 2.4) & (noise(rng, (2, 2)) > 0.3)  # crusted over in plates, short of the cracks
    out[plate] = crust[(noise(rng) > 0.5).astype(int)][plate]
    return out


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
    """The furnace's sides and back: cobblestone with the dressed stone panel its front has round the mouth."""
    out = cobblestone(rng)
    stone = shades("Stone", 1.6)
    out[6:15, 3:13] = stone[3]  # a dressed stone panel, lit round its rim
    out[7:14, 4:12] = stone[2]
    return out


def furnace_front(rng):
    """The side's panel with the furnace's dark mouth in it."""
    out = furnace_side(rng)
    out[8:13, 5:11] = (26, 25, 25)  # the dark mouth, with a sooty floor
    out[12, 5:11] = (44, 40, 38)
    return out


def chest_top(rng):
    out = planks(rng)
    rim = shades("LogBark")
    out[[0, 15]] = rim[1]  # the lid's rim, darker than its boards
    out[:, [0, 15]] = rim[1]
    return out


def chest_side(rng):
    out = planks(rng)
    rim = shades("LogBark")
    out[[0, 15]] = rim[1]
    out[:, [0, 15]] = rim[1]
    out[7] = rim[0]  # where the lid shuts on the box, along a seam between boards
    # an iron latch over the seam, since blocks have no facing yet to keep it to a front
    iron = sprite(["lggd", "gddd", "gdkd", "gddd", "dddd"], dict(l=(206, 206, 210), g=(168, 168, 174), d=(118, 118, 126), k=(46, 44, 48)))
    for dy, dx, c in iron:
        out[5 + dy, 6 + dx] = c
    return out


# A bed's blanket, darkest first, and its sheet and pillow.
BLANKET = np.array([(132, 26, 30), (166, 36, 38), (196, 54, 52)], float)
LINEN = np.array([(206, 204, 194), (232, 230, 222)], float)


def bed_top(rng):
    out = BLANKET[levels(noise(rng, (1, 2)), 3)]
    out[2:6, 3:13] = LINEN[levels(noise(rng)[2:6, 3:13], 2)]  # the pillow at the head
    out[2, 3:13] = out[2:6, 3] = LINEN[1]  # lit along its upper left
    out[6, 2:14] = BLANKET[0]  # the blanket's turned-down edge under it
    frame = shades("Planks")
    out[:, [0, 15]] = frame[1]
    out[[0, 15]] = frame[1]
    return out


def bed_side(rng):
    out = planks(rng)  # the frame
    out[0:2] = LINEN[1]  # the sheet's edge
    out[2:9] = BLANKET[levels(noise(rng, (0, 3))[2:9], 3)]  # the blanket hanging over the side
    out[8] = BLANKET[0]
    out[13:16, 3:13] = shades("Planks")[0] * 0.6  # in the shade under it, between the legs
    return out


WOOL = np.array([(186, 180, 166), (210, 206, 194), (228, 225, 215), (244, 242, 234)], float)


def wool(rng):
    """Felted wool: soft, fine noise in cream, with tight curls scattered over it, each a lit arc over a shaded one,
    wrapping round the edges so it tiles."""
    out = WOOL[1 + levels(noise(rng, (1, 1)), 2)]
    for _ in range(16):
        y, x = rng.integers(0, 16, 2)
        for dy, dx, shade in [(0, 0, 3), (0, 1, 3), (1, -1, 2), (1, 2, 0), (2, 0, 0), (2, 1, 0)]:
            out[(y + dy) % 16, (x + dx) % 16] = WOOL[shade]
    return out


def clear():
    """A tile with nothing in it yet, as RGBA, for plants drawn on transparency."""
    return np.zeros((16, 16, 4))


def paint(out, y, x, colour):
    out[y, x] = (*colour, 255)


def plant(name):
    """A plant's tile, drawn by `flora.py` and greyscale ones levelled to their row's `grey` in TILES for the climate's
    tint. It's drawn from the seed alone, not its place here, see `__main__`."""
    grey = next(t for t in TILES if t["name"] == name).get("grey")
    return lambda rng: flora.sprite(name, rng, grey)


def torch(rng):
    """A stick under a flame, in the tile's two middle columns from row 6 down, which the torch's box shows
    (`generator::TORCH`): its lit side on the left, the flame bright at the top and charred at the stick."""
    out = clear()
    bark, end = shades("LogBark"), shades("LogEnd")
    flame = [((255, 246, 184), (255, 224, 120)), ((255, 200, 76), (248, 164, 48)), ((228, 112, 34), (176, 78, 26))]
    for y, (lit, shaded) in enumerate(flame, 6):
        paint(out, y, 7, lit)
        paint(out, y, 8, shaded)
    for y in range(9, 16):
        grain = rng.integers(0, 2)
        paint(out, y, 7, end[grain])
        paint(out, y, 8, bark[2 + grain])
    return out


def farmland_top(rng):
    """Dirt tilled into furrows across the tile, a little darker and damper than dirt, each ridge lit along its top
    and shaded down into the groove after it."""
    dirt = shades("Dirt", 1.2) * 0.85
    y = np.mgrid[:16, :16][0]
    ridge = np.array([2, 2, 1, 0])[y % 4]  # four furrows, four rows each: lit, lit, shaded, groove
    out = dirt[np.clip(ridge + (noise(rng, (0, 1)) > 0.75) - (noise(rng) > 0.8), 0, len(dirt) - 1)]
    out[y % 4 == 3] *= 0.8
    return out


GREEN = np.array([(58, 98, 30), (76, 124, 40), (98, 148, 52)], float)  # untinted, darkest first
GOLD = np.array([(164, 122, 40), (212, 170, 70), (240, 212, 124)], float)


def wheat(stage):
    """Wheat at `stage` of 8: stalks rising from the ground, taller each stage and turning from green to gold,
    leafy while young and with braided ears at their tops from stage 4, golden and bearded once ripe. Drawn on
    transparency and not tinted, as a crop's colour is its ripeness rather than the climate's."""
    def draw(rng):
        out = clear()
        ripeness = max(0, stage - 3) / 4
        stalk = GREEN + (GOLD - GREEN) * ripeness * 0.85
        grains = GREEN + (GOLD - GREEN) * min(1, ripeness * 1.4)
        height = 3 + stage * 12 // 7  # from 3 to 15 texels
        ear = 0 if stage < 4 else 3 + (stage >= 6)  # how many texels of ear top a stalk
        for x0 in [1, 3, 6, 8, 11, 13]:
            tall = height - rng.integers(0, 2 + (stage < 4))
            lean = rng.uniform(-0.3, 0.3)
            shade = stalk[1 + rng.integers(0, 2)]
            leaf = rng.integers(1, max(2, tall - ear - 1))  # where a leaf sticks out of a young stalk
            side = rng.choice([-1, 1])
            for i in range(tall):  # from the ground up
                x = int(round(x0 + lean * i * i / tall))
                if not 0 <= x < 16:
                    break
                y = 15 - i
                if i >= tall - ear:  # the ear: grains either side of the stalk, in turn
                    paint(out, y, x, grains[1])
                    grain = x + (1 if i % 2 else -1)
                    if 0 <= grain < 16:
                        paint(out, y, grain, grains[2] if grain < x else grains[0])
                    if stage == 7 and i == tall - 1 and y > 0:  # its beard, above the ear
                        paint(out, y - 1, x + (1 if lean > 0 else -1) if 0 < x < 15 else x, GOLD[2])
                else:
                    paint(out, y, x, stalk[0] if i == 0 else shade)
                    if stage < 5 and i == leaf and 0 <= x + side < 16:
                        paint(out, y, x + side, stalk[2])
                        if 0 <= x + 2 * side < 16 and y > 0:
                            paint(out, y - 1, x + 2 * side, stalk[2])
        return out
    return draw


def frame(colour, dashed):
    """A square outline round the tile's edge on transparency, dashed or not, for the blocks only seen as icons."""
    out = clear()
    for i in range(16):
        if not dashed or i % 4 in (0, 3):  # dashes of two, a corner of one each end
            for y, x in [(0, i), (15, i), (i, 0), (i, 15)]:
                paint(out, y, x, colour)
    return out


def structure_void(rng):
    """A dashed outline round nothing, as the cell a structure leaves as the world has it."""
    out = frame((120, 210, 255), dashed=True)
    for y, x, c in sprite(["ll", "ll"], dict(l=(120, 210, 255))):
        paint(out, 7 + y, 7 + x, c)
    return out


def loot_marker(rng):
    """A gold outline round a small chest, where a structure's loot goes."""
    out = frame((240, 200, 60), dashed=False)
    chest = sprite(["bbbbbbbb", "wwwwwwww", "kkkggkkk", "wwwggwww", "wwwwwwww", "dddddddd"],
                   dict(b=(150, 100, 48), w=(186, 132, 66), k=(96, 62, 30), g=(240, 200, 60), d=(110, 72, 34)))
    for y, x, c in chest:
        paint(out, 5 + y, 4 + x, c)
    return out


def glass(rng):
    """A clear pane in a thin pale frame, so a wall of glass shows a grid of them, with a glint or two on it. The
    shader cuts at half alpha, so every texel is either drawn or clear."""
    out = clear()
    for i in range(16):
        for y, x in [(0, i), (i, 0)]:
            paint(out, y, x, (226, 238, 240))  # lit along the top and left, shaded along the bottom and right
        for y, x in [(15, i), (i, 15)]:
            paint(out, y, x, (196, 214, 220))
    for y, x in [(5, 2), (4, 3), (3, 4), (2, 5), (7, 3), (6, 4), (12, 10), (11, 11), (10, 12)]:
        paint(out, y, x, (236, 246, 248))
    return out


def vines(rng):
    """Greyscale tendrils hanging down the tile, tinted per climate like tall grass, whose shades they take: each
    sways in a whole wave over the tile and puts out a leaf to alternate sides, so a wall of vines tiles both ways."""
    grey = shades("GrassTop", 1.4)
    stem = shades("GrassTop", 2.4)[0]  # darker than the leaves, so the tendrils show through them
    out = clear()
    for x0 in [2, 7, 12]:
        phase = rng.uniform(0, 2 * np.pi)
        xs = [int(round(x0 + 0.8 * np.sin(2 * np.pi * y / 16 + phase))) % 16 for y in range(16)]
        for y, x in enumerate(xs):  # a stem no more than a texel off the one above, so it reads as one
            paint(out, y, x, stem)
        start = rng.integers(0, 4)
        for n, y in enumerate(range(start, start + 16, 4)):  # a leaf every four rows, to one side then the other
            side = 1 if n % 2 else -1
            x = xs[y % 16]
            leaf = [(0, side, grey[-1]), (0, 2 * side, grey[-1]), (1, side, grey[2]), (1, 2 * side, grey[1]), (2, side, grey[1]),
                    (1, 0, grey[2])]  # the last where it joins the stem
            for dy, dx, shade in leaf:  # lit on top, drooping
                paint(out, (y + dy) % 16, (x + dx) % 16, shade)
    return out


DOOR_SEAMS = [5, 10]  # three boards across a door's face, between its frame's sides


def door_face(rng, seams, ends):
    """Vertical boards between `seams`, in a frame two texels wide down the tile's sides and across the `ends`
    ("top", "bottom") it has. A door's and a trapdoor's thin edges show their outer three rows and columns, so
    those keep to plain wood."""
    wood = shades("Planks")
    edge = shades("LogBark")
    board = np.searchsorted(seams, np.arange(16))
    out = wood[1:][levels(noise(rng, (4, 0)) + (rng.random(len(seams) + 1) * 0.4)[board][None, :], 3)]
    out[:, seams] = wood[0]
    out[:, [1, 14]] = edge[-1]
    out[:, [0, 15]] = edge[-2]  # the frame's outer edge, darker, so neighbouring doors part
    for end in ends:
        frame, outer, inner = ([1], 0, 2) if end == "top" else ([14], 15, 13)
        out[frame, 1:15] = edge[-1]
        out[outer] = edge[-2]
        out[inner, seams] = wood[2]  # the seams stop short of the frame, so the edge it shows is plain
    return out


def door_bottom(rng):
    """The lower half of a door: boards with a cross rail, and an iron latch at the top on the right, which is
    the door's middle."""
    out = door_face(rng, DOOR_SEAMS, ["bottom"])
    out[8:10, 3:13] = shades("LogBark")[-1]  # a cross rail, short of the sides the door's edges show
    out[8, 3:13] = shades("Planks")[2]
    iron = sprite(["lg", "gd", "kd", "dd"], dict(l=(206, 206, 210), g=(168, 168, 174), d=(118, 118, 126), k=(46, 44, 48)))
    for dy, dx, c in iron:
        out[1 + dy, 11 + dx] = c
    return out


def door_top(rng):
    """The upper half of a door: boards with a window of four small clear panes in their upper middle."""
    out = np.dstack([door_face(rng, DOOR_SEAMS, ["top"]), np.full((16, 16), 255)])
    edge = shades("LogBark")
    out[3:11, 3:13, :3] = edge[-1]  # the window's frame and mullions
    for y, x in [(4, 4), (4, 9), (8, 4), (8, 9)]:
        out[y:y + 2, x:x + 3] = 0
    out[3, 3:13, :3] = edge[-2]  # its frame lit along the top and shaded at the sill
    out[10, 3:13, :3] = edge[-2]
    return out


def trapdoor(rng):
    """A wooden hatch seen from above: boards in a plain frame, with a few small square holes through them."""
    out = np.dstack([door_face(rng, [6, 9], ["top", "bottom"]), np.full((16, 16), 255)])
    out[2, 2:14, :3] = out[13, 2:14, :3] = out[2:14, 2, :3] = out[2:14, 13, :3] = shades("Planks")[1]  # the frame's lit inner edge
    for y in (4, 10):
        for x in (4, 10):
            out[y:y + 2, x:x + 2] = 0
    return out


def ladder(rng):
    """Two rails up the tile, at the columns a ladder's box shows, with a rung every four rows, on transparency."""
    wood = shades("Planks")
    out = clear()
    for y in range(16):
        grain = rng.integers(0, 2)
        for x, shade in [(2, wood[2 + grain]), (3, wood[1]), (12, wood[2 + grain]), (13, wood[1])]:  # lit on the left
            paint(out, y, x, shade)
    for y in (1, 5, 9, 13):
        for x in range(4, 12):
            paint(out, y, x, wood[3])  # lit on top, shaded underneath
            paint(out, y + 1, x, wood[0])
    return out


IRON = [(38, 38, 44), (58, 58, 66), (84, 84, 92), (116, 116, 124)]  # dark worked iron, darkest first


def lantern(rng):
    """An iron lantern unwrapped onto the boxes the mesher builds it from, each in its own rectangle (x0, y0)-(x1, y1),
    x right and y down, half-open: the body's sides (0, 9)-(6, 16), a frame round glowing glass, drawn at full
    brightness like the torch's flame; its top and bottom (0, 2)-(6, 8); the cap's top (6, 10)-(10, 14) and sides
    (6, 14)-(10, 16); and the chain it hangs by (11, 0)-(14, 16)."""
    out = clear()

    def box(x0, y0, x1, y1, inside, rim):
        for y in range(y0, y1):
            for x in range(x0, x1):
                paint(out, y, x, rim if x in (x0, x1 - 1) or y in (y0, y1 - 1) else inside)

    box(0, 9, 6, 16, IRON[1], IRON[1])
    glow = sprite(["oyyo", "yffy", "yFFy", "yffy", "oyyo"], dict(o=(232, 146, 52), y=(250, 188, 78), f=(255, 216, 112), F=(255, 242, 176)))
    for dy, dx, c in glow:  # the flame bright in the middle of the glass
        paint(out, 10 + dy, 1 + dx, c)
    box(0, 2, 6, 8, IRON[0], IRON[2])
    box(6, 10, 10, 14, IRON[1], IRON[2])
    box(6, 14, 10, 16, IRON[1], IRON[1])
    # the chain's links in turn face on, a ring, and edge on, a bar, lit on the left
    for dy, dx, c in sprite([".l.", "l.d", "l.d", ".l.", ".l.", ".d.", ".d.", ".l."] * 2, dict(l=IRON[3], d=IRON[1])):
        paint(out, dy, 11 + dx, c)
    return out


MOSS = np.array([(62, 84, 40), (80, 104, 48), (100, 124, 58)], float)  # not tinted, darkest first


def mossy(rng, out, gaps, amount):
    """Moss grown over `out`, in most of its `gaps` and in patches over the rest, which `amount` widens."""
    grow = noise(rng)
    moss = (gaps & (grow > 0.5 - amount)) | (grow > 1 - amount)
    out[moss] = MOSS[levels(noise(rng), 3)][moss]
    return out


def mossy_cobblestone(rng):
    """Cobblestone with moss grown over it, most thickly in the mortar between the stones."""
    out = cobblestone(rng)
    return mossy(rng, out, (out @ LUM) < (out @ LUM).min() + 1, 0.2)


def cracked_stone_bricks(rng):
    """Stone bricks with cracks running across them and a corner or two chipped away into the mortar."""
    out = stone_bricks(rng)
    mortar = shades("Stone", 1.6)[0] * 0.7
    for top in (0, 8, rng.choice([0, 8])):  # a crack or two across each course of bricks
        y, x, step = top + rng.integers(2, 5), rng.integers(0, 16), rng.choice([-1, 1])
        for _ in range(rng.integers(5, 8)):  # wandering sideways, up and down a little, within the course
            out[y, x % 16] = mortar
            y, x = np.clip(y + rng.integers(-1, 2) * (rng.random() < 0.5), top + 1, top + 5), x + step
    # a brick's top right corner and another's bottom left, knocked off
    for y, x, dy, dx in [(0, 14, 1, -1), (14, 8, -1, 1)]:
        out[y, x] = out[y + dy, x] = out[y, x + dx] = mortar
    return out


def mossy_stone_bricks(rng):
    """Stone bricks with moss in their mortar lines and a patch or two over the bricks."""
    out = stone_bricks(rng)
    return mossy(rng, out, (out @ LUM) < (out @ LUM).min() + 1, 0.15)


def polished_stone(rng):
    """Smooth dressed stone, lighter and flatter than stone, with a slightly darker bevel round each block."""
    stone = shades("Stone", 0.6) * 1.3
    out = stone[1:][levels(noise(rng, (3, 3)), 3)]
    out[[0, 15]] = out[:, [0, 15]] = stone[0] * 0.92
    return out


def terracotta(colour):
    """Baked clay of a colour: flat and matte, with faint mottling from the firing."""
    def draw(rng):
        return np.array(colour, float) * np.array([0.96, 1.0, 1.035])[levels(noise(rng), 3)][..., None]
    return draw


def dark_planks(rng):
    """Planks of a dark brown wood."""
    return boards(rng, [(74, 50, 30), (84, 57, 34), (94, 65, 40)], (50, 33, 20))


def light_planks(rng):
    """Planks of a pale wood, like birch."""
    return boards(rng, [(190, 170, 122), (202, 183, 134), (214, 196, 148)], (150, 128, 88))


BOOKS = [(118, 40, 36), (52, 70, 108), (62, 94, 52), (142, 108, 52), (88, 58, 98), (106, 72, 48), (58, 92, 94)]


def bookshelf(rng):
    """A bookshelf's side: shelves at the top, middle and bottom, and two rows of books of mixed colours and
    heights between them, the shelf's dark back showing above the shorter ones."""
    out = planks(rng)
    rim = shades("LogBark")
    for top in (0, 7, 14):
        out[top:top + 2] = shades("LogEnd")[1:3][levels(noise(rng, (0, 3))[top:top + 2], 2)]
        out[top + 1] = rim[-1]
    for top in (2, 9):  # five rows of books on each shelf
        out[top:top + 5] = rim[0]
        x = 0
        while x < 16:
            width = min(16 - x, rng.integers(1, 3))
            spine = np.array(BOOKS[rng.integers(len(BOOKS))], float)
            short = rng.integers(0, 3)
            out[top + short:top + 5, x:x + width] = spine
            out[top + short:top + 5, x + width - 1] = spine * 0.8  # shaded down its right
            out[top + 1 + short, x:x + width] = spine * 1.2  # a band across the spine
            x += width
    return out


def barrel_wood():
    """A barrel's staves and lid, in the planks' wood darkened, and the seams between them."""
    return shades("LogEnd")[:3] * 0.78, shades("LogBark")[1]


def barrel_end(rng):
    """A barrel's lid or base: boards inside the ends of its staves, with an iron band just inside them."""
    wood, seam = barrel_wood()
    out = boards(rng, wood, seam)
    out[[0, 15]] = out[:, [0, 15]] = wood[0] * 0.85
    out[1, 1:15] = out[1:15, 1] = IRON[2]  # lit along the top and left, shaded along the bottom and right
    out[14, 1:15] = out[1:15, 14] = IRON[1]
    return out


def barrel_top(rng):
    """The barrel's lid, with a small dark bung hole."""
    out = barrel_end(rng)
    out[6:9, 9:12] = IRON[1]
    out[7, 10] = (24, 18, 12)
    return out


def barrel_side(rng):
    """A barrel's side, standing upright: four staves up the tile and two iron hoops across them."""
    wood, seam = barrel_wood()
    x = np.arange(16)
    out = wood[levels(noise(rng, (4, 0)) + (rng.random(4) * 0.4)[x // 4][None, :], 3)]
    out[:, 3::4] = seam
    for top in (2, 12):
        out[top] = IRON[2]
        out[top + 1] = IRON[1]
    return out


def spawner(rng):
    """A dungeon's spawner: a cage of dark iron bars round a dark hollow with a faint smoulder
    of embers deep in it, bevelled as a solid block."""
    y, x = np.mgrid[:16, :16]
    heat = 1 - np.hypot(y - 7.5, x - 7.5) / 7 + noise(rng) * 0.5
    hollow = np.array([(14, 10, 12), (30, 16, 14), (66, 24, 14), (120, 40, 16)], float)
    out = hollow[levels(np.clip(heat, 0, None), 4)]
    out[(x % 5 == 0) | (y % 5 == 0)] = IRON[2]  # the bars, meeting at the edges so neighbours share them
    out[((x % 5 == 0) & (y % 5 == 1)) | ((y % 5 == 0) & (x % 5 == 1))] = IRON[1]  # each bar's shaded side
    out[(x % 5 == 0) & (y % 5 == 0)] = IRON[3]  # rivets where they cross
    return bevel(out, IRON[3], IRON[0])


DRAW = dict(Planks=planks, Cobblestone=cobblestone, StoneBricks=stone_bricks, Bricks=bricks, CoalOre=coal_ore, CopperOre=copper_ore,
            IronOre=iron_ore, CraftingTableTop=crafting_table_top, CraftingTableSide=crafting_table_side, FurnaceFront=furnace_front,
            TallGrass=plant("TallGrass"), Poppy=plant("Poppy"), Dandelion=plant("Dandelion"), Cornflower=plant("Cornflower"), Torch=torch, BedTop=bed_top,
            BedSide=bed_side, FarmlandTop=farmland_top, **{f"Wheat{stage}": wheat(stage) for stage in range(7)},
            RipeWheat=wheat(7), ChestTop=chest_top, ChestSide=chest_side, StructureVoid=structure_void,
            LootMarker=loot_marker, Fern=plant("Fern"), GoldOre=gold_ore, FrostCrystalOre=frost_crystal_ore, JadeOre=jade_ore,
            EmberOre=ember_ore, GoldBlock=gold_block, FrostCrystalBlock=frost_crystal_block, PolishedJade=polished_jade,
            BrightSteelBlock=bright_steel_block, EmberLamp=ember_lamp,
            FurnaceSide=furnace_side, Glass=glass, Vines=vines, DoorBottom=door_bottom, DoorTop=door_top,
            Trapdoor=trapdoor, Ladder=ladder, Lantern=lantern, MossyCobblestone=mossy_cobblestone,
            CrackedStoneBricks=cracked_stone_bricks, MossyStoneBricks=mossy_stone_bricks, PolishedStone=polished_stone,
            Terracotta=terracotta((152, 94, 67)), RedTerracotta=terracotta((143, 61, 47)), YellowTerracotta=terracotta((186, 133, 35)),
            BlueTerracotta=terracotta((70, 62, 94)), GreenTerracotta=terracotta((76, 83, 42)), DarkPlanks=dark_planks,
            LightPlanks=light_planks, Bookshelf=bookshelf, BarrelTop=barrel_top, BarrelSide=barrel_side, BarrelBottom=barrel_end,
            Wool=wool, BasaltSide=basalt_side, BasaltTop=basalt_top, Magma=magma,
            ShortGrass=plant("ShortGrass"), WildGrass=plant("WildGrass"), Spawner=spawner)
assert set(DRAW) == {t["name"] for t in TILES if t.get("drawn")}, "DRAW and the drawn rows of TILES differ"

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("tiles", nargs="*", help="tile names, default all drawn tiles")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    for name in a.tiles or DRAW:
        # plants are drawn from the seed alone, as `flora.py` was before it joined this script, so the default draws
        # the tiles in the atlas
        rng = np.random.default_rng(a.seed if name in flora.DRAW else [a.seed, list(DRAW).index(name)])
        rgb = DRAW[name](rng)
        # plants, the torch and the other cut-outs are drawn as RGBA, on transparency, and every other tile as RGB
        Image.fromarray(np.clip(np.asarray(rgb).round(), 0, 255).astype("uint8")).convert("RGBA").save(OUT / f"{name}.png")
        print("drew", name)
