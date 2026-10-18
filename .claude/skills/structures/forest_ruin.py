"""Writes assets/structures/forest_ruin.toml: a cottage in the woods, long abandoned and falling in.

A log frame on a cobblestone footing, its walls cobblestone below and planks above, with a
stone chimney on its west end and a hearth inside. Its south-east corner has collapsed,
taking half the dark plank roof with it: what's left of the roof leans on the west gable,
and the rest lies in the room as rubble. Inside are a bookshelf, a barrel, a bed and the
chest; moss, vines, ferns and a bush have moved in, and a broken fence still rounds the yard.

    python3 .claude/skills/structures/forest_ruin.py
"""

import random

from build import AIR, Build, slab, stairs

W, LAYERS, D = 14, 12, 14
FLOOR = 1
X0, X1, Z0, Z1 = 2, 12, 1, 9
EAVES = 6

COBBLE = "cobblestone*3|mossy_cobblestone*3"
PLANKS = "planks*5|dark_planks|air"
LOG = "log"
rand = random.Random(11)

b = Build(W, LAYERS, D, floor=FLOOR)


def fallen(x, y, z):
    """Whether the block at x, y, z went down with the south-east corner: the higher and the
    nearer that corner, the likelier."""
    away = (X1 - x) + (Z1 - z)
    return y >= 2 + away // 2 + rand.choice((0, 0, 1, 2))


# the footing, and a floor of old boards with earth showing through
b.fill((X0, 0, Z0), (X1, 0, Z1), COBBLE)
b.fill((X0 + 1, 0, Z0 + 1), (X1 - 1, 0, Z1 - 1), "planks*4|dark_planks*2|dirt*2")

# the walls, cobble below and boards above, in a log frame
for y in range(1, EAVES):
    block = COBBLE if y <= 2 else PLANKS
    b.walls((X0, y, Z0), (X1, y, Z1), block)
    b.fill((X0 + 1, y, Z0 + 1), (X1 - 1, y, Z1 - 1), AIR)
b.corners((X0, 1, Z0), (X1, EAVES - 1, Z1), LOG)
b.fill((7, 1, Z0), (7, EAVES - 1, Z0), LOG)
b.fill((7, 1, Z1), (7, EAVES - 1, Z1), LOG)
for x in range(X0 + 1, X1):
    b.set(x, EAVES - 1, Z0, "log[axis=x]")
    b.set(x, EAVES - 1, Z1, "log[axis=x]")
for z in range(Z0 + 1, Z1):
    b.set(X0, EAVES - 1, z, "log[axis=z]")
    b.set(X1, EAVES - 1, z, "log[axis=z]")
# windows of broken panes, and the doorway with a step up to it
for x in (4, 10):
    for z in (Z0, Z1):
        b.set(x, 3, z, "glass_pane|air")
b.set(X1, 3, 5, "glass_pane|air")
b.fill((5, 1, Z1), (5, 2, Z1), AIR)
b.set(5, 0, Z1 + 1, stairs("cobblestone", "north"))
b.set(5, 1, Z1 + 1, AIR)

# the roof, a gable of dark plank stairs along x over a plank gable end
ridge = b.gable(X0, X1, Z0, Z1, EAVES, "dark_planks", along="x", gable=PLANKS)

# the chimney on the west end, and the hearth inside it
b.fill((0, 0, 4), (1, ridge + 1, 6), COBBLE)
b.fill((1, 1, 5), (X0, 2, 5), AIR)
b.set(1, 1, 5, "torch")
b.set(X0, 3, 5, slab("stone_bricks", top=True))
b.set(X0, 3, 4, "stone_bricks")
b.set(X0, 3, 6, "stone_bricks")
b.set(1, ridge + 2, 5, "cobblestone_wall")

# the collapse: the south-east corner down, with the roof over it
for (x, y, z), block in list(b.cells.items()):
    if y >= 1 and block not in (None, AIR) and x > 6 and fallen(x, y, z):
        b.set(x, y, z, AIR if X0 < x < X1 and Z0 < z < Z1 else None)
# what fell lies in the room and spills out of the corner
for x in range(7, X1 + 2):
    for z in range(3, Z1 + 2):
        inside = x < X1 and z < Z1
        if b.get(x, 1, z) in (None, AIR) and rand.random() < (0.45 if inside else 0.3):
            b.set(x, 1, z, rand.choice((
                COBBLE, COBBLE, slab("cobblestone"), stairs("dark_planks", "west"),
                slab("dark_planks"), "planks",
            )))
b.set(10, 2, 7, stairs("dark_planks", "north"))
b.set(9, 1, 6, "leaves")
b.set(10, 1, 6, "leaves|air")
b.set(10, 2, 6, "leaves|air*2")

# inside: a bookshelf, a barrel and the chest, a bed by the hearth, a table, ferns and moss
b.set(X0 + 1, 1, Z0 + 1, "bookshelf")
b.set(X0 + 1, 2, Z0 + 1, "bookshelf|air")
b.set(X0 + 2, 1, Z0 + 1, "bookshelf")
b.set(X0 + 3, 1, Z0 + 1, "barrel[facing=up]")
b.mark("loot", X0 + 4, 1, Z0 + 1)
b.set(X0 + 1, 1, Z1 - 1, "bed")
b.set(X0 + 1, 1, Z1 - 2, "bed")
b.set(5, 1, 4, "dark_planks_fence")
b.set(5, 2, 4, slab("dark_planks"))
b.set(5, 1, 3, stairs("planks", "south"))
b.set(4, 1, 6, "fern")
b.set(8, 1, 3, "fern|tall_grass")
b.set(5, 3, 4, "lantern|air")
# vines down the north wall and the chimney
for x in range(X0, X1 + 1):
    for y in range(2, EAVES):
        b.set(x, y, Z0 - 1, "vines[facing=north]|air*3")
for y in range(2, ridge):
    b.set(0, y, 3, "vines[facing=north]|air*2")
    b.set(0, y, 7, "vines[facing=south]|air*2")

# the yard's fence, fallen in places
for x in range(X0, X1 + 1):
    if x != 5:
        b.set(x, 1, D - 1, "dark_planks_fence*3|air*2")
for z in range(Z1 + 1, D - 1):
    b.set(X0, 1, z, "dark_planks_fence*3|air*2")
    b.set(X1, 1, z, "dark_planks_fence|air*3")
b.set(5, 1, D - 1, "dark_planks_fence")
b.set(5, 2, D - 1, "lantern|air")

b.write(
    "forest_ruin",
    {
        "cell": 448,
        "chance": 0.6,
        "biomes": ["forest", "base", "taiga"],
        "min_above_sea": 2,
        "max_rise": 4,
        "hilltop": False,
    },
    foundation="cobblestone",
    about="An abandoned cottage in the woods, its roof fallen in.",
    script="forest_ruin.py",
)
