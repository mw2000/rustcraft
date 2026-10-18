"""Writes assets/structures/desert_temple.toml: a stepped sandstone temple half buried in a desert.

Three tiers of sandstone, each with a sloping skirt at its foot and a cornice of turned-over
stairs at its top, banded in terracotta (orange or red, chosen for each temple) with blue
inlays, and a pyramid-roofed shrine on top. Two crenellated towers flank a columned portico
on the south, where steps lead up to a recessed doorway. Inside is a pillared hall laid in
terracotta, lit by lanterns, with an altar at its far end; behind the altar a hatch in the
floor opens on a ladder down to a crypt sunk under the temple, where the chests are. A stair
climbs the east side to the terraces. Sand has drifted against the walls.

    python3 .claude/skills/structures/desert_temple.py
"""

from build import AIR, Build, slab, stairs

W, LAYERS, D = 27, 27, 29
FLOOR = 6
# the ground's level, the hall's floor a step over it, and the crypt's floor under it
GROUND = FLOOR
HALL = FLOOR
CRYPT = 1
MID = 13

STONE = "sandstone*14|sand"
SMOOTH = "sandstone"
BAND = "terracotta/red_terracotta"
INLAY = "blue_terracotta"
DRIFT = "sand*2|air*3"

b = Build(W, LAYERS, D, floor=FLOOR)


def tier(x0, z0, x1, z1, y0, y1, skirt=True):
    """A tier's walls from layer `y0` to its cornice at `y1`, solid through, with a skirt of
    stairs round its foot and its roof a terrace at `y1 + 1`."""
    b.fill((x0, y0, z0), (x1, y1, z1), STONE)
    if skirt:
        for x in range(x0 - 1, x1 + 2):
            b.set(x, y0, z0 - 1, stairs("sandstone", "south"))
            b.set(x, y0, z1 + 1, stairs("sandstone", "north"))
        for z in range(z0, z1 + 1):
            b.set(x0 - 1, y0, z, stairs("sandstone", "east"))
            b.set(x1 + 1, y0, z, stairs("sandstone", "west"))
    # the cornice, standing out a block, and a low parapet of slabs on the terrace's edge
    for x in range(x0 - 1, x1 + 2):
        b.set(x, y1, z0 - 1, stairs("sandstone", "south", top=True))
        b.set(x, y1, z1 + 1, stairs("sandstone", "north", top=True))
        b.set(x, y1 + 1, z0 - 1, slab("sandstone"))
        b.set(x, y1 + 1, z1 + 1, slab("sandstone"))
    for z in range(z0, z1 + 1):
        b.set(x0 - 1, y1, z, stairs("sandstone", "east", top=True))
        b.set(x1 + 1, y1, z, stairs("sandstone", "west", top=True))
        b.set(x0 - 1, y1 + 1, z, slab("sandstone"))
        b.set(x1 + 1, y1 + 1, z, slab("sandstone"))
    b.fill((x0, y1 + 1, z0), (x1, y1 + 1, z1), SMOOTH)


def band(x0, z0, x1, z1, y):
    """A ring of terracotta round the walls at layer `y`, inlaid with blue every few blocks."""
    for x in range(x0, x1 + 1):
        for z in (z0, z1):
            b.set(x, y, z, INLAY if (x - MID) % 4 == 0 else BAND)
    for z in range(z0, z1 + 1):
        for x in (x0, x1):
            b.set(x, y, z, INLAY if (z - z0) % 4 == 2 else BAND)


# the ground under it all, sunk, with the crypt carved out of it
b.fill((2, 0, 3), (24, FLOOR - 1, 24), SMOOTH)

# the first tier, holding the hall
T1 = (3, 4, 23, 23)
tier(*T1, HALL, 11)
band(*T1, 9)
# the second, holding the sanctum, and the shrine on top
T2 = (7, 7, 19, 18)
tier(*T2, 13, 17)
band(*T2, 15)
S = (10, 10, 16, 16)
b.fill((S[0], 19, S[1]), (S[2], 21, S[3]), STONE)
for y in (19, 20, 21):
    for x, z in ((S[0], S[1]), (S[2], S[1]), (S[0], S[3]), (S[2], S[3])):
        b.set(x, y, z, "sandstone")
roof = b.hip(S[0], S[2], S[1], S[3], 22, "sandstone", cap=INLAY)

# towers flanking the front, crenellated with stairs between posts of sandstone wall
for tx in (1, 21):
    x0, x1, z0, z1 = tx, tx + 4, 19, 24
    b.fill((x0, HALL, z0), (x1, 18, z1), STONE)
    band(x0, z0, x1, z1, 9)
    band(x0, z0, x1, z1, 15)
    for x in range(x0 - 1, x1 + 2):
        b.set(x, 18, z0 - 1, stairs("sandstone", "south", top=True))
        b.set(x, 18, z1 + 1, stairs("sandstone", "north", top=True))
    for z in range(z0, z1 + 1):
        b.set(x0 - 1, 18, z, stairs("sandstone", "east", top=True))
        b.set(x1 + 1, 18, z, stairs("sandstone", "west", top=True))
    b.fill((x0 - 1, 19, z0 - 1), (x1 + 1, 19, z1 + 1), SMOOTH)
    for x in range(x0 - 1, x1 + 2):
        for z in range(z0 - 1, z1 + 2):
            edge = x in (x0 - 1, x1 + 1) or z in (z0 - 1, z1 + 1)
            if not edge:
                continue
            corner = x in (x0 - 1, x1 + 1) and z in (z0 - 1, z1 + 1)
            if corner or (x + z) % 2 == 0:
                b.set(x, 20, z, "sandstone_wall")
                b.set(x, 21, z, "lantern" if corner else "sandstone_slab|air")
            else:
                facing = (
                    "south" if z == z0 - 1 else "north" if z == z1 + 1 else
                    "east" if x == x0 - 1 else "west"
                )
                b.set(x, 20, z, stairs("sandstone", facing) + "*5|air")
    # slits looking out
    for y in (12, 13, 16):
        b.set(tx + 2, y, 24, AIR)
        b.set(tx if tx == 1 else tx + 4, y, 21, AIR)

# the portico: four columns on a raised floor, carrying a slab roof with a cornice
b.fill((8, HALL, 24), (18, HALL, 26), SMOOTH)
for x in range(8, 19):
    b.set(x, HALL, 27, stairs("sandstone", "north"))
for x in (9, 11, 15, 17):
    b.set(x, HALL + 1, 25, stairs("sandstone", "south"))
    b.fill((x, HALL + 2, 25), (x, HALL + 4, 25), "sandstone")
    b.set(x, HALL + 1, 25, "sandstone")
    b.set(x, HALL + 5, 25, stairs("sandstone", "south", top=True))
b.fill((8, HALL + 6, 24), (18, HALL + 6, 26), SMOOTH)
for x in range(8, 19):
    b.set(x, HALL + 6, 27, stairs("sandstone", "north", top=True))
    b.set(x, HALL + 7, 26, slab("sandstone"))
    b.set(x, HALL + 6, 26, INLAY if x in (9, 13, 17) else BAND)
for z in range(24, 27):
    b.set(7, HALL + 6, z, stairs("sandstone", "east", top=True))
    b.set(19, HALL + 6, z, stairs("sandstone", "west", top=True))

# the doorway, recessed into the front wall under a lintel of turned-over stairs
b.fill((MID - 1, HALL + 1, 22), (MID + 1, HALL + 4, 23), AIR)
b.fill((MID - 2, HALL + 5, 23), (MID + 2, HALL + 5, 23), SMOOTH)
for x, facing in ((MID - 1, "east"), (MID + 1, "west")):
    b.set(x, HALL + 4, 23, stairs("sandstone", facing, top=True))
b.set(MID, HALL + 5, 23, INLAY)

# the hall: a floor of terracotta with a blue aisle, two rows of pillars, lanterns hung
# between them, windows high in the side walls and an altar at the far end
hx0, hz0, hx1, hz1 = T1[0] + 1, T1[1] + 1, T1[2] - 1, T1[3] - 2
b.fill((hx0, HALL + 1, hz0), (hx1, 11, hz1), AIR)
for x in range(hx0, hx1 + 1):
    for z in range(hz0, hz1 + 1):
        tile = INLAY if abs(x - MID) <= 1 else "terracotta" if (x + z) % 2 else "yellow_terracotta"
        b.set(x, HALL, z, tile)
for x in (MID - 4, MID + 4):
    for z in (8, 12, 16, 20):
        b.fill((x, HALL + 1, z), (x, 10, z), "sandstone")
        b.set(x, HALL + 3, z, BAND)
        b.set(x, 11, z, "sandstone")
for x in (MID - 2, MID + 2):
    for z in (10, 14, 18):
        b.set(x, 11, z, "lantern[hanging=true]")
for z in (10, 14, 18):
    for x in (T1[0], T1[2]):
        b.set(x, 9, z, AIR)
        b.set(x, 10, z, AIR)
# the altar on a dais, braziers either side
b.fill((MID - 3, HALL + 1, hz0), (MID + 3, HALL + 1, hz0 + 3), slab("sandstone"))
b.fill((MID - 1, HALL + 2, hz0 + 2), (MID + 1, HALL + 2, hz0 + 2), "sandstone")
b.set(MID, HALL + 2, hz0 + 2, INLAY)
for x in (MID - 3, MID + 3):
    b.set(x, HALL + 2, hz0 + 3, "sandstone_wall")
    b.set(x, HALL + 3, hz0 + 3, "lantern")
# the hatch behind the altar, and the ladder down to the crypt
hatch = (MID, hz0)
b.set(hatch[0], HALL + 1, hatch[1], "trapdoor[facing=south]")
b.set(hatch[0], HALL, hatch[1], AIR)

# the crypt under the hall
cx0, cz0, cx1, cz1 = MID - 5, hz0, MID + 5, hz0 + 8
b.fill((cx0, CRYPT, cz0), (cx1, CRYPT + 2, cz1), AIR)
b.fill((cx0, CRYPT - 1, cz0), (cx1, CRYPT - 1, cz1), "yellow_terracotta")
b.ladder(hatch[0], hatch[1], CRYPT, HALL, "south")
for x in (cx0 + 2, cx1 - 2):
    for z in (cz0 + 3, cz1 - 1):
        b.fill((x, CRYPT, z), (x, CRYPT + 2, z), "sandstone")
        b.set(x, CRYPT + 1, z, BAND)
# a sarcophagus in the middle, chests at either end, lanterns on posts
b.fill((MID - 1, CRYPT, cz0 + 4), (MID + 1, CRYPT, cz0 + 6), "sandstone")
b.fill((MID - 1, CRYPT + 1, cz0 + 4), (MID + 1, CRYPT + 1, cz0 + 6), slab("sandstone"))
b.set(MID, CRYPT + 1, cz0 + 5, INLAY)
b.mark("loot", cx0, CRYPT, cz1)
b.mark("loot", cx1, CRYPT, cz1)
b.mark("loot", cx1, CRYPT, cz0 + 1)
for x in (cx0, cx1):
    b.set(x, CRYPT, cz0 + 4, "sandstone_wall")
    b.set(x, CRYPT + 1, cz0 + 4, "lantern")
b.set(MID, CRYPT + 2, cz1, "lantern[hanging=true]")
b.fill((cx0, CRYPT + 2, cz0 + 1), (cx0, CRYPT + 2, cz0 + 1), "yellow_terracotta")

# the stair up the east side to the terrace, on a landing over the cornice
for i in range(7):
    z, y = 20 - i, HALL + i
    for x in (25, 26):
        b.set(x, y, z, stairs("sandstone", "north"))
        b.fill((x, HALL, z), (x, y - 1, z), SMOOTH)
b.fill((24, HALL, 12), (26, 12, 13), SMOOTH)
# and up the second tier's face to the shrine
for i in range(6):
    for x in (MID - 1, MID, MID + 1):
        b.set(x, 13 + i, 23 - i, stairs("sandstone", "north"))
b.fill((MID, 19, S[3]), (MID, 20, S[3]), AIR)
b.set(MID, 21, S[3], stairs("sandstone", "south", top=True))
b.fill((S[0] + 1, 19, S[1] + 1), (S[2] - 1, 21, S[3] - 1), AIR)
b.set(MID, 19, S[1] + 1, "sandstone")
b.set(MID, 20, S[1] + 1, "lantern")

# sand drifted against the walls and over the skirts
for x in range(0, W):
    for z in range(0, D):
        if b.get(x, GROUND, z) is None:
            near = any(
                b.get(x + dx, GROUND, z + dz) not in (None, AIR)
                for dx in (-1, 0, 1)
                for dz in (-1, 0, 1)
            )
            if near:
                b.set(x, GROUND, z, DRIFT)

b.write(
    "desert_temple",
    {
        "cell": 512,
        "chance": 0.9,
        "biomes": ["desert"],
        "min_above_sea": 3,
        "max_rise": 10,
        "hilltop": False,
    },
    foundation="sandstone",
    about="A stepped sandstone temple in the desert, with a crypt under its hall.",
    script="desert_temple.py",
)
