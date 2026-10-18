"""Writes assets/structures/watchtower.toml: an old stone watchtower on a hilltop.

A shaft of weathered stone brick seven blocks square on a sloping cobblestone plinth, a door in
its south side and arrow slits up it, a ladder climbing the inside past a middle floor, and at
the top a timber lookout overhanging the shaft on stone corbels: log posts, plank walls with a
band of windows, a hipped roof of dark plank stairs and a flagpole, lit by lanterns, with the
chest inside.

    python3 .claude/skills/structures/watchtower.py
"""

from build import AIR, Build, stairs

SIZE = 11
LAYERS = 27
FLOOR = 1
# the shaft's walls, and the lookout's on the corbels round it
SHAFT = (2, 8)
ROOM = (1, 9)
MIDDLE = 8
LOOKOUT = 16
ROOF = 20

COBBLE = "cobblestone*3|mossy_cobblestone"
BRICK = "stone_bricks*6|mossy_stone_bricks*2|cracked_stone_bricks*2"
# lower courses where the cobble gives way to brick
MIXED = "cobblestone*2|stone_bricks*2|mossy_cobblestone|mossy_stone_bricks"

b = Build(SIZE, LAYERS, SIZE, floor=FLOOR)
lo, hi = SHAFT
mid = SIZE // 2

# the footing, and a plinth a block out from the shaft with a sloping top
b.fill((1, 0, 1), (9, 0, 9), COBBLE)
b.walls((1, 1, 1), (9, 2, 9), COBBLE)
for x in range(1, 10):
    b.set(x, 3, 1, stairs("cobblestone", "south"))
    b.set(x, 3, 9, stairs("cobblestone", "north"))
for z in range(2, 9):
    b.set(1, 3, z, stairs("cobblestone", "east"))
    b.set(9, 3, z, stairs("cobblestone", "west"))

# the shaft, cobble low down giving way to brick
for y in range(1, LOOKOUT):
    block = COBBLE if y < 4 else MIXED if y < 7 else BRICK
    b.walls((lo, y, lo), (hi, y, hi), block)
    b.fill((lo + 1, y, lo + 1), (hi - 1, y, hi - 1), AIR)
# arrow slits up each side, the door below on the south
for y in (5, 6, 11, 12):
    for x, z in ((mid, lo), (mid, hi), (lo, mid), (hi, mid)):
        b.set(x, y, z, AIR)
b.fill((mid, 1, 9), (mid, 2, 9), AIR)
b.set(mid, 3, 9, stairs("stone_bricks", "north", top=True))
b.door(mid, 1, hi, "south")
b.set(mid, 3, hi, "stone_bricks")
# vines creeping up the north face
for y in range(4, 14):
    for x in range(lo, hi + 1):
        b.set(x, y, lo - 1, "vines[facing=north]|air*3")

# the middle floor, with the ladder's way through it, a barrel and a lantern under it
b.fill((lo + 1, MIDDLE, lo + 1), (hi - 1, MIDDLE, hi - 1), "dark_planks")
b.ladder(mid, lo + 1, 1, LOOKOUT, "south")
b.set(lo + 1, 1, hi - 1, "barrel[facing=up]")
b.set(lo + 2, 1, hi - 1, "barrel[facing=up]|air")
b.set(lo + 1, 1, lo + 1, "gravel")
b.set(hi - 1, MIDDLE - 1, hi - 1, "lantern[hanging=true]")
b.set(hi - 1, MIDDLE + 1, lo + 1, "barrel[facing=up]")
b.set(lo + 1, LOOKOUT - 1, hi - 1, "lantern[hanging=true]")

# a string course round the shaft at the middle floor, and corbels carrying the lookout out
# over it at the top, each a ring of stairs turned over against the shaft
r0, r1 = ROOM


def ring(y):
    for x in range(r0, r1 + 1):
        b.set(x, y, r0, stairs("stone_bricks", "south", top=True))
        b.set(x, y, r1, stairs("stone_bricks", "north", top=True))
    for z in range(r0 + 1, r1):
        b.set(r0, y, z, stairs("stone_bricks", "east", top=True))
        b.set(r1, y, z, stairs("stone_bricks", "west", top=True))


ring(MIDDLE)
ring(LOOKOUT - 1)

# the lookout: a plank floor, log posts and beams, a band of windows
b.fill((r0, LOOKOUT, r0), (r1, LOOKOUT, r1), "dark_planks")
b.set(mid, LOOKOUT, lo + 1, "ladder[facing=south]")
for y in range(LOOKOUT + 1, ROOF):
    b.walls((r0, y, r0), (r1, y, r1), "planks")
    b.fill((r0 + 1, y, r0 + 1), (r1 - 1, y, r1 - 1), AIR)
b.corners((r0, LOOKOUT + 1, r0), (r1, ROOF - 1, r1), "log")
for x, z in ((mid, r0), (mid, r1), (r0, mid), (r1, mid)):
    b.fill((x, LOOKOUT + 1, z), (x, ROOF - 1, z), "log")
for x in range(r0 + 1, r1):
    for z in (r0, r1):
        b.set(x, ROOF - 1, z, "log[axis=x]")
        if x != mid:
            b.set(x, LOOKOUT + 2, z, "glass_pane")
for z in range(r0 + 1, r1):
    for x in (r0, r1):
        b.set(x, ROOF - 1, z, "log[axis=z]")
        if z != mid:
            b.set(x, LOOKOUT + 2, z, "glass_pane")

# inside: the chest, a bed, a barrel and a table by the windows, a lantern hung from the roof
b.mark("loot", r1 - 1, LOOKOUT + 1, r1 - 1)
b.set(r0 + 1, LOOKOUT + 1, r1 - 1, "bed")
b.set(r1 - 1, LOOKOUT + 1, r0 + 1, "barrel[facing=up]")
b.set(r0 + 1, LOOKOUT + 1, r0 + 1, "dark_planks_fence")
b.set(r0 + 1, LOOKOUT + 2, r0 + 1, "lantern")

# a hipped roof hanging out over the walls, a flagpole on its point
top = b.hip(r0, r1, r0, r1, ROOF, "dark_planks", cap="dark_planks")
b.set(mid, top - 1, mid, "lantern[hanging=true]")
b.set(mid, top + 1, mid, "dark_planks_fence")

b.write(
    "watchtower",
    {
        "cell": 640,
        "chance": 0.6,
        "biomes": ["forest", "foothills", "taiga", "savanna"],
        "min_above_sea": 12,
        "max_rise": 4,
        "hilltop": True,
    },
    foundation="cobblestone",
    about="An old stone watchtower on a hilltop, its timber lookout lit by lanterns.",
    script="watchtower.py",
)
