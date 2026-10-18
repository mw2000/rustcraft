"""Writes assets/structures/mineshaft.toml: old timbered workings deep under the ground.

Two levels of tunnels three blocks wide and three high, eight layers apart, each held up every
four blocks by a frame of fence posts and a beam (dark planks or planks, chosen for each
mineshaft), its floor patched with boards, cobble and gravel. A hub with a ladder joins the
levels, and a second ladder shaft stands at a far corner; side rooms off the tunnels hold
barrels, a crafting table, ore heaps and the chests, by lanterns on the posts. The tunnels
leave the rock round them as it is, so where a cave crosses them they open into it, and their
boards bridge it.

    python3 .claude/skills/structures/mineshaft.py
"""

import random

from build import AIR, Build

SIZE, LAYERS = 48, 13
LOWER, UPPER = 1, 9
HEIGHT = 3

POST = "dark_planks_fence/planks_fence"
BEAM = "dark_planks/planks"
BOARDS = "planks*3|cobblestone|gravel|stone*3"
rand = random.Random(5)

b = Build(SIZE, LAYERS, SIZE, floor=0)


def tunnel(y, start, end, along, at):
    """A tunnel on the level whose floor is under layer `y`, from `start` to `end` along x or
    z, centred across it at `at`, framed every four blocks."""
    for i in range(min(start, end), max(start, end) + 1):
        for d in (-1, 0, 1):
            x, z = (i, at + d) if along == "x" else (at + d, i)
            b.fill((x, y, z), (x, y + HEIGHT - 1, z), AIR)
            if b.get(x, y - 1, z) is None:
                b.set(x, y - 1, z, BOARDS)
        if i % 4 == 0:
            for d in (-1, 1):
                x, z = (i, at + d) if along == "x" else (at + d, i)
                b.fill((x, y, z), (x, y + 1, z), POST)
                if rand.random() < 0.25:
                    b.set(x, y + 1, z, "lantern")
            for d in (-1, 0, 1):
                x, z = (i, at + d) if along == "x" else (at + d, i)
                b.set(x, y + HEIGHT - 1, z, BEAM)
        elif rand.random() < 0.06:
            x, z = (i, at) if along == "x" else (at, i)
            b.set(x, y, z, "gravel*2|air")


def room(y, x0, z0, x1, z1, things):
    """A side room on a level, with `things` set at points in it."""
    b.fill((x0, y, z0), (x1, y + HEIGHT, z1), AIR)
    b.fill((x0, y - 1, z0), (x1, y - 1, z1), "planks*3|dark_planks|cobblestone")
    b.corners((x0, y, z0), (x1, y + HEIGHT, z1), "log")
    for x in range(x0, x1 + 1):
        b.set(x, y + HEIGHT, z0, "log[axis=x]")
        b.set(x, y + HEIGHT, z1, "log[axis=x]")
    for (x, dy, z), block in things:
        b.set(x, y + dy, z, block)


# the lower level
tunnel(LOWER, 2, 45, "x", 23)
tunnel(LOWER, 4, 44, "z", 9)
tunnel(LOWER, 6, 40, "z", 37)
# the upper level, crossing it the other way
tunnel(UPPER, 2, 45, "z", 23)
tunnel(UPPER, 6, 42, "x", 11)
tunnel(UPPER, 12, 44, "x", 37)

# the hub, open from the lower level's floor to the upper level's roof, a ladder up its wall
# and a platform round the upper level
b.fill((20, LOWER, 20), (26, UPPER + HEIGHT - 1, 26), AIR)
b.fill((20, UPPER - 1, 20), (26, UPPER - 1, 26), "dark_planks")
b.fill((21, UPPER - 1, 21), (25, UPPER - 1, 25), AIR)
b.corners((20, LOWER, 20), (26, UPPER + HEIGHT - 1, 26), "log")
b.fill((19, LOWER - 1, 19), (27, LOWER - 1, 27), "cobblestone*2|gravel|stone")
b.fill((23, LOWER, 20), (23, UPPER + 1, 20), "dark_planks")
b.ladder(23, 21, LOWER, UPPER, "south")
b.set(21, LOWER, 25, "barrel[facing=up]")
b.set(22, LOWER, 25, "barrel[facing=up]|air")
b.set(25, LOWER, 25, "crafting_table")
b.set(21, UPPER + 1, 21, "lantern")
b.set(20, UPPER, 23, "dark_planks_fence")
b.set(25, LOWER + 2, 23, "lantern[hanging=true]")
b.set(25, LOWER + 3, 23, "log[axis=x]")

# a second shaft at the lower level's east end, up to a tunnel joining the upper level
tunnel(UPPER, 22, 37, "z", 43)
b.fill((44, LOWER, 23), (44, UPPER + 2, 23), AIR)
b.fill((45, LOWER, 23), (45, UPPER + 2, 23), "dark_planks")
b.ladder(44, 23, LOWER, UPPER + 1, "west")

# side rooms: a store with the chest, a tool room, and a heap of what was dug
room(LOWER, 4, 1, 10, 5, [
    ((5, 0, 2), "barrel[facing=up]"), ((5, 1, 2), "barrel[facing=up]|air"),
    ((6, 0, 2), "barrel[facing=up]"), ((10, 0, 4), "lantern"),
    ((4, 0, 5), "crafting_table"),
])
b.mark("loot", 7, LOWER, 1)
room(LOWER, 34, 41, 40, 45, [
    ((35, 0, 45), "coal_ore"), ((36, 0, 45), "coal_ore|cobblestone"),
    ((35, 1, 45), "coal_ore|air"), ((36, 0, 44), "copper_ore|gravel"),
    ((40, 0, 45), "iron_ore|gravel"), ((39, 0, 41), "barrel[facing=up]"),
    ((40, 1, 41), "lantern"), ((40, 0, 41), "barrel[facing=up]"),
])
b.mark("loot", 38, LOWER, 45)
room(UPPER, 2, 8, 6, 14, [
    ((2, 0, 9), "barrel[facing=up]"), ((2, 0, 13), "furnace[facing=east]"),
    ((2, 1, 10), "lantern"), ((2, 0, 10), "cobblestone"),
])
b.mark("loot", 2, UPPER, 12)

b.write(
    "mineshaft",
    {
        "cell": 384,
        "chance": 0.5,
        "biomes": ["forest", "base", "foothills", "taiga", "savanna", "jungle", "desert", "glacier"],
        "min_above_sea": 4,
        "max_rise": 40,
        "hilltop": False,
        "depth": [24, 64],
    },
    foundation="stone",
    about="Old timbered mine workings deep under the ground, on two levels.",
    script="mineshaft.py",
)
