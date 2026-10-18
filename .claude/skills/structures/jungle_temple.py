"""Writes assets/structures/jungle_temple.toml: a stepped stone temple overgrown by a jungle.

Four tiers of mossy cobblestone and weathered stone brick, each stepping in with a sloped edge
of stairs, climbed by a broad stair standing out from the south face between stepped
balustrades, to a shrine on top with jade masks over its doorway, a corbelled roof and a crest.
Under the tiers is a dark pillared chamber, reached by a ladder down from the shrine or a low
passage from the north, lit by a few lanterns, with a chest in each. Leaves have taken root on
the steps and vines hang down the faces.

    python3 .claude/skills/structures/jungle_temple.py
"""

from build import AIR, Build, slab, stairs

W, LAYERS, D = 23, 25, 30
FLOOR = 2
MID = 11

OLD = (
    "mossy_cobblestone*4|cobblestone*2|mossy_stone_bricks*2|cracked_stone_bricks"
    "|stone_bricks"
)
BRICK = "mossy_stone_bricks*2|stone_bricks*2|cracked_stone_bricks*2"
JADE = "polished_jade"
GROWTH = "leaves|air*3"

b = Build(W, LAYERS, D, floor=FLOOR)
b.fill((1, 0, 1), (21, FLOOR - 1, 21), "cobblestone")

# the tiers, each three layers, the top course a slope of stairs, leaves taking root on them
TIERS = [(1, 2), (3, 5), (5, 8), (7, 11)]
for inset, y0 in TIERS:
    lo, hi = inset, W - 1 - inset
    b.fill((lo, y0, lo), (hi, y0 + 1, hi), OLD)
    b.fill((lo + 1, y0 + 2, lo + 1), (hi - 1, y0 + 2, hi - 1), OLD)
    for x in range(lo, hi + 1):
        b.set(x, y0 + 2, lo, stairs("cobblestone", "south"))
        b.set(x, y0 + 2, hi, stairs("cobblestone", "north"))
    for z in range(lo + 1, hi):
        b.set(lo, y0 + 2, z, stairs("cobblestone", "east"))
        b.set(hi, y0 + 2, z, stairs("cobblestone", "west"))
    for x in range(lo, hi + 1):
        for z in range(lo, hi + 1):
            if x in (lo, hi) or z in (lo, hi):
                b.set(x, y0 + 1, z, BRICK)
    # vines down the faces from under the slope
    for x in range(lo, hi + 1):
        b.set(x, y0 + 1, lo - 1, "vines[facing=north]|air*2")
        b.set(x, y0, lo - 1, "vines[facing=north]|air*3")
    for z in range(lo, hi + 1):
        b.set(lo - 1, y0 + 1, z, "vines[facing=west]|air*2")
        b.set(hi + 1, y0 + 1, z, "vines[facing=east]|air*2")
        b.set(lo - 1, y0, z, "vines[facing=west]|air*3")
        b.set(hi + 1, y0, z, "vines[facing=east]|air*3")

# growth on the terraces each tier leaves round the next
for (inset, y0), (inner, _) in zip(TIERS, TIERS[1:] + [(9, 0)]):
    for x in range(inset + 1, W - 1 - inset):
        for z in range(inset + 1, W - 1 - inset):
            ring = not (inner <= x <= W - 1 - inner and inner <= z <= W - 1 - inner)
            if ring and (x + 2 * z) % 3:
                b.set(x, y0 + 3, z, GROWTH)

# the stair up the south face, standing out from it, between stepped balustrades
TOP = TIERS[-1][1] + 2
for i in range(TOP - FLOOR + 1):
    y, z = FLOOR + i, D - 2 - i
    for x in range(MID - 2, MID + 3):
        b.set(x, y, z, stairs("stone_bricks", "north"))
        b.fill((x, FLOOR, z), (x, y - 1, z), OLD)
    for x in (MID - 3, MID + 3):
        b.fill((x, FLOOR, z), (x, y, z), OLD)
        b.set(x, y + 1, z, stairs("cobblestone", "north") if i % 2 else BRICK)
b.fill((MID - 3, TOP, D - 2 - (TOP - FLOOR) - 2), (MID + 3, TOP, D - 3 - (TOP - FLOOR)), OLD)

# the shrine on top: thick walls, a doorway under jade masks, a corbelled roof and a crest
S0, S1 = 8, 14
y0 = TOP + 1
b.fill((S0, y0, S0), (S1, y0 + 3, S1), BRICK)
b.fill((S0 + 1, y0, S0 + 1), (S1 - 1, y0 + 2, S1 - 1), AIR)
b.corners((S0, y0, S0), (S1, y0 + 3, S1), "cobblestone")
b.fill((MID - 1, y0, S1), (MID + 1, y0 + 1, S1), AIR)
for x in (MID - 1, MID + 1):
    b.set(x, y0 + 2, S1, stairs("stone_bricks", "west" if x < MID else "east", top=True))
b.set(MID, y0 + 2, S1, AIR)
for x in (MID - 2, MID + 2, MID):
    b.set(x, y0 + 3, S1, JADE)
for z in (MID,):
    b.set(S0, y0 + 1, z, AIR)
    b.set(S1, y0 + 1, z, AIR)
top = b.hip(S0, S1, S0, S1, y0 + 4, "cobblestone", cap="mossy_cobblestone")
for x in range(MID - 2, MID + 3):
    b.set(x, top + 1, MID, "cobblestone_wall")
b.set(MID - 2, top + 2, MID, "mossy_cobblestone")
b.set(MID + 2, top + 2, MID, "mossy_cobblestone")
# inside the shrine: an altar of jade, the way down and a chest
b.set(MID, y0, S0 + 1, JADE)
b.set(MID, y0 + 1, S0 + 1, "lantern")
b.set(MID - 1, y0, S0 + 1, slab("stone_bricks"))
b.set(MID + 1, y0, S0 + 1, slab("stone_bricks"))
b.mark("loot", S0 + 1, y0, S1 - 1)
b.set(S1 - 1, y0 + 2, S1 - 1, "lantern[hanging=true]")

# the chamber under the tiers, dark but for a few lanterns, pillared and overgrown
X0, X1, Z0, Z1 = 6, 16, 9, 17
b.fill((X0, FLOOR, Z0), (X1, FLOOR + 3, Z1), AIR)
b.fill((X0, FLOOR - 1, Z0), (X1, FLOOR - 1, Z1), "mossy_cobblestone|cobblestone|gravel")
for x in (X0 + 2, X1 - 2):
    for z in (Z0 + 2, Z1 - 2):
        b.fill((x, FLOOR + 1, z), (x, FLOOR + 3, z), "mossy_stone_bricks|stone_bricks")
        b.set(x, FLOOR, z, "cobblestone")
for x, z in ((X0 + 2, Z0 + 3), (X1 - 2, Z1 - 3)):
    b.set(x, FLOOR + 3, z, "lantern[hanging=true]")
for x in range(X0, X1 + 1):
    b.set(x, FLOOR + 3, Z0, "vines[facing=south]|air*2")
    b.set(x, FLOOR + 3, Z1, "vines[facing=north]|air*2")
b.mark("loot", MID - 2, FLOOR, Z0)
b.set(MID - 3, FLOOR, Z0, "barrel[facing=up]|air")
b.set(MID, FLOOR, Z1 - 3, JADE)
b.set(MID, FLOOR + 1, Z1 - 3, slab("stone_bricks"))
b.set(MID - 1, FLOOR, Z1 - 3, stairs("stone_bricks", "east"))
b.set(MID + 1, FLOOR, Z1 - 3, stairs("stone_bricks", "west"))
# the ladder down from the shrine, against the chamber's north wall and up a shaft
shaft = (MID + 2, Z0)
b.fill((shaft[0], FLOOR + 4, shaft[1]), (shaft[0], y0 - 1, shaft[1]), AIR)
b.ladder(shaft[0], shaft[1], FLOOR, y0, "south")
# and the low passage in from the north, under a lintel, a lantern at its mouth
b.fill((MID, FLOOR, 0), (MID, FLOOR + 1, Z0), AIR)
b.fill((MID - 1, FLOOR, 0), (MID + 1, FLOOR + 2, 0), AIR)
b.set(MID - 1, FLOOR + 2, 0, stairs("stone_bricks", "east", top=True))
b.set(MID + 1, FLOOR + 2, 0, stairs("stone_bricks", "west", top=True))
b.set(MID, FLOOR + 2, 0, BRICK)
b.fill((MID - 1, FLOOR, 1), (MID + 1, FLOOR + 1, 1), AIR)
b.set(MID - 1, FLOOR, 1, "cobblestone_wall")
b.set(MID - 1, FLOOR + 1, 1, "lantern")

b.write(
    "jungle_temple",
    {
        "cell": 512,
        "chance": 0.9,
        "biomes": ["jungle"],
        "min_above_sea": 3,
        "max_rise": 10,
        "hilltop": False,
    },
    foundation="mossy_cobblestone",
    about="A stepped stone temple overgrown by the jungle, with a dark chamber under it.",
    script="jungle_temple.py",
)
