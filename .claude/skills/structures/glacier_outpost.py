"""Writes assets/structures/glacier_outpost.toml: a lodge on the ice, half buried in snow.

A thick stone base standing out a block from timber walls of planks (dark, plain or light,
chosen for each lodge) in a frame of logs, under a steep roof of dark plank stairs patched
with snow and capped by a snowy ridge, with a stone chimney at its west end and a lean-to
woodshed of stacked logs at its east. The door on the south has steps up to it and a lantern
by it. Inside are two beds, a furnace in the hearth, a crafting table, barrels, a bookshelf,
a table and the chest, lit by lanterns. Snow has drifted up its north wall.

    python3 .claude/skills/structures/glacier_outpost.py
"""

from build import AIR, Build, slab, stairs

W, LAYERS, D = 19, 20, 15
FLOOR = 2
# the stone base, the timber walls inset a block on it, and the eaves
BX0, BX1, BZ0, BZ1 = 2, 16, 2, 11
X0, X1, Z0, Z1 = 3, 15, 3, 10
EAVES = 8

STONE = "stone_bricks*4|cobblestone*2|mossy_stone_bricks|cracked_stone_bricks"
TIMBER = "dark_planks/planks/light_planks"
DRIFT = "snow*2|air"

b = Build(W, LAYERS, D, floor=FLOOR)

# the base: a footing, a plank floor, stone walls to the sills with a ledge of slabs
b.fill((BX0, 0, BZ0), (BX1, FLOOR - 1, BZ1), STONE)
b.fill((X0 + 1, FLOOR - 1, Z0 + 1), (X1 - 1, FLOOR - 1, Z1 - 1), "dark_planks")
for y in (FLOOR, FLOOR + 1):
    b.walls((BX0, y, BZ0), (BX1, y, BZ1), STONE)
    b.fill((X0 + 1, y, Z0 + 1), (X1 - 1, y, Z1 - 1), AIR)
    b.fill((X0, y, Z0), (X1, y, Z1), STONE, lambda x, y, z: x in (X0, X1) or z in (Z0, Z1))
for x in range(BX0, BX1 + 1):
    b.set(x, FLOOR + 2, BZ0, slab("stone_bricks"))
    b.set(x, FLOOR + 2, BZ1, slab("stone_bricks"))
for z in range(BZ0 + 1, BZ1):
    b.set(BX0, FLOOR + 2, z, slab("stone_bricks"))
    b.set(BX1, FLOOR + 2, z, slab("stone_bricks"))

# timber walls in a log frame, a band of windows
for y in range(FLOOR + 2, EAVES):
    b.walls((X0, y, Z0), (X1, y, Z1), TIMBER)
    b.fill((X0 + 1, y, Z0 + 1), (X1 - 1, y, Z1 - 1), AIR)
for x in range(X0, X1 + 1, 4):
    for z in (Z0, Z1):
        b.fill((x, FLOOR + 2, z), (x, EAVES - 1, z), "log")
for z in (Z0, Z1):
    b.fill((X0, FLOOR + 2, z), (X0, EAVES - 1, z), "log")
    b.fill((X1, FLOOR + 2, z), (X1, EAVES - 1, z), "log")
for x in range(X0 + 1, X1):
    for z in (Z0, Z1):
        if b.get(x, EAVES - 1, z) != "log":
            b.set(x, EAVES - 1, z, "log[axis=x]")
        if (x - X0) % 4 in (1, 3):
            b.set(x, FLOOR + 3, z, "glass_pane")
for z in range(Z0 + 1, Z1):
    b.set(X1, EAVES - 1, z, "log[axis=z]")
b.set(X1, FLOOR + 3, 6, "glass_pane")
b.set(X1, FLOOR + 3, 7, "glass_pane")

# the steep roof: two layers to each step in, a stair over a block, along x, snow lying on it
def near(layer):
    return Z0 - 1 + max(0, (layer - EAVES + 1) // 2)


top = EAVES
for layer in range(EAVES, LAYERS):
    lo = near(layer)
    hi = Z0 + Z1 - lo
    if lo >= hi:
        break
    upper = layer == EAVES or (layer - EAVES) % 2 == 0
    for x in range(X0 - 1, X1 + 2):
        for z, facing in ((lo, "south"), (hi, "north")):
            if upper:
                b.set(x, layer, z, stairs("dark_planks", facing) + "*6|snow")
            else:
                b.set(x, layer, z, "dark_planks")
        # the gable ends, the west one of stone round the chimney
        if x in (X0, X1):
            for z in range(lo + 1, hi):
                b.set(x, layer, z, "planks" if x == X1 else STONE)
    top = layer
b.fill((X0 - 1, top + 1, near(top)), (X1 + 1, top + 1, Z0 + Z1 - near(top)), "snow")
b.set(X1, EAVES + 2, 6, "glass_pane")
b.set(X1, EAVES + 2, 7, "glass_pane")

# the chimney, out of the west gable, and the hearth with the furnace in it
b.fill((1, 0, 6), (2, top + 2, 7), "cobblestone*3|stone_bricks")
b.set(1, top + 3, 6, "snow")
b.set(X0, FLOOR, 6, "furnace[facing=east]")
b.set(X0, FLOOR, 7, "furnace[facing=east]")
b.set(X0 + 1, FLOOR, 5, "stone_bricks")
b.set(X0 + 1, FLOOR, 8, "stone_bricks")
b.set(X0 + 1, FLOOR + 1, 5, "lantern")

# the door, the steps up to it under a little porch roof, a lantern by it
DX = 9
b.door(DX, FLOOR, Z1, "south")
b.fill((DX - 1, FLOOR, BZ1), (DX + 1, FLOOR + 1, BZ1), AIR)
b.fill((DX - 1, FLOOR - 1, BZ1 + 1), (DX + 1, FLOOR - 1, BZ1 + 1), stairs("stone_bricks", "north"))
b.set(DX - 2, FLOOR, BZ1 + 1, "dark_planks_fence")
b.set(DX + 2, FLOOR, BZ1 + 1, "dark_planks_fence")
b.fill((DX - 2, FLOOR + 1, BZ1 + 1), (DX - 2, FLOOR + 2, BZ1 + 1), "dark_planks_fence")
b.fill((DX + 2, FLOOR + 1, BZ1 + 1), (DX + 2, FLOOR + 2, BZ1 + 1), "dark_planks_fence")
for x in range(DX - 2, DX + 3):
    b.set(x, FLOOR + 3, BZ1 + 1, slab("dark_planks"))
    b.set(x, FLOOR + 3, BZ1, slab("dark_planks"))
    b.set(x, FLOOR + 4, BZ1 + 1, "snow|air")
b.set(DX + 1, FLOOR + 2, BZ1, "lantern[hanging=true]")

# the woodshed against the east end: a slab roof on posts over stacked logs
b.fill((BX1 + 1, FLOOR - 1, 4), (BX1 + 2, FLOOR - 1, 9), "cobblestone")
for z in (4, 9):
    b.fill((BX1 + 2, FLOOR, z), (BX1 + 2, FLOOR + 2, z), "dark_planks_fence")
b.fill((BX1 + 1, FLOOR, 5), (BX1 + 2, FLOOR + 1, 8), "log[axis=z]")
b.fill((BX1 + 1, FLOOR + 1, 5), (BX1 + 2, FLOOR + 1, 8), "log[axis=z]*2|air")
b.fill((BX1 + 1, FLOOR + 3, 4), (BX1 + 2, FLOOR + 3, 9), slab("dark_planks"))
b.fill((BX1 + 1, FLOOR + 4, 4), (BX1 + 2, FLOOR + 4, 9), "snow*2|air")

# inside: beds, a crafting table, barrels, a bookshelf, a table, the chest and lanterns
b.set(X1 - 1, FLOOR, Z0 + 1, "bed")
b.set(X1 - 1, FLOOR, Z0 + 2, "bed")
b.set(X1 - 1, FLOOR, Z1 - 1, "bed")
b.set(X1 - 1, FLOOR, Z1 - 2, "bed")
b.set(X1 - 1, FLOOR, 6, "barrel[facing=up]")
b.set(X1 - 1, FLOOR + 1, 6, "lantern")
b.set(X0 + 1, FLOOR, Z0 + 1, "crafting_table")
b.set(X0 + 2, FLOOR, Z0 + 1, "barrel[facing=up]")
b.set(X0 + 3, FLOOR, Z0 + 1, "barrel[facing=up]|air")
b.set(X0 + 4, FLOOR, Z0 + 1, "bookshelf")
b.set(X0 + 4, FLOOR + 1, Z0 + 1, "bookshelf")
b.mark("loot", X0 + 5, FLOOR, Z0 + 1)
b.set(8, FLOOR, 6, "dark_planks_fence")
b.set(8, FLOOR + 1, 6, slab("dark_planks"))
b.set(7, FLOOR, 6, stairs("dark_planks", "east"))
b.set(9, FLOOR, 6, stairs("dark_planks", "west"))
# a tie beam across under the roof, with lanterns hung from it
b.fill((X0 + 1, EAVES, 6), (X1 - 1, EAVES, 6), "log[axis=x]")
for x in (6, 12):
    b.set(x, EAVES - 1, 6, "lantern[hanging=true]")

# snow drifted up the north wall and round the base
for x in range(0, W):
    for z in range(0, D):
        if b.get(x, FLOOR, z) is not None:
            continue
        if z < BZ0:
            depth = 3 - (BZ0 - 1 - z)
            b.fill((x, FLOOR, z), (x, FLOOR + depth - 1, z), "snow")
            b.set(x, FLOOR + depth, z, DRIFT)
        elif any(b.get(x + dx, FLOOR, z + dz) not in (None, AIR) for dx in (-1, 1) for dz in (-1, 1)):
            b.set(x, FLOOR, z, DRIFT)

b.write(
    "glacier_outpost",
    {
        "cell": 448,
        "chance": 0.9,
        "biomes": ["glacier", "taiga"],
        "min_above_sea": 4,
        "max_rise": 8,
        "hilltop": False,
    },
    foundation="stone_bricks",
    about="A lodge on the ice, half buried in snow, stocked for the cold.",
    script="glacier_outpost.py",
)
