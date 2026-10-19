"""Writes the adobe village's pieces into assets/structures/village/: the village of the desert
and the savanna, of mud brick round courtyards of trodden earth.

The style: walls of adobe on sandstone plinths, banded in terracotta (red, yellow or blue for
each building), flat roofs behind parapets stepped up at their corners, the vigas, the log
beams carrying each roof, standing out of the walls under it, roof terraces reached by stairs
up the outside, roof rooms and towers stepping the silhouette up, small deep-set windows on
sandstone sills, awnings of plank slabs on fence posts, and domes of stairs over the halls and
the bread ovens.

    python3 .claude/skills/structures/village_adobe.py
"""

from village import AIR, F, FLOWERS, Piece, slab, stairs

SCRIPT = "village_adobe.py"
STYLES = ["adobe"]

# Mud brick patched here and there; the sandstone trim; the band's colour for the whole
# building (of 3); the awnings' wood (of 2), apart from it.
ADOBE = "adobe*24|sandstone|terracotta"
TRIM = "sandstone"
TERRA = "red_terracotta/yellow_terracotta/blue_terracotta"
AWNING = "planks_slab/dark_planks_slab"
POST = "planks_fence/dark_planks_fence"
TILES = "terracotta*4|sandstone"
YARD = "path*5|sand*2|gravel"


def box(b, x0, x1, z0, z1, y0, y1, wall=ADOBE, floor=TILES):
    """A room of adobe walls from layer `y0` to `y1` round the box from `x0`, `z0` to `x1`, `z1`,
    on a sandstone plinth at layer `y0 - 1` floored with `floor` inside, cleared within."""
    b.fill((x0, y0 - 1, z0), (x1, y0 - 1, z1), TRIM)
    b.fill((x0 + 1, y0 - 1, z0 + 1), (x1 - 1, y0 - 1, z1 - 1), floor)
    for y in range(y0, y1 + 1):
        b.walls((x0, y, z0), (x1, y, z1), wall)
    b.fill((x0 + 1, y0, z0 + 1), (x1 - 1, y1, z1 - 1), AIR)


def band(b, x0, x1, z0, z1, y, block=TERRA):
    """A band of terracotta round the walls at layer `y`, where they're adobe."""
    for x in range(x0, x1 + 1):
        for z in range(z0, z1 + 1):
            if (x in (x0, x1) or z in (z0, z1)) and b.get(x, y, z) == ADOBE:
                b.set(x, y, z, block)


def flat_roof(b, x0, x1, z0, z1, y, vigas=(), along="z", gaps=(), piers="all", cap=None):
    """A flat roof of adobe at layer `y` over the walls round the box from `x0`, `z0` to `x1`,
    `z1`, carried on vigas, logs running `along` z (at the given x) or x (at the given z) whose
    ends stand a block out of the walls, behind a parapet a block high and capped with `cap`
    (sandstone slabs by default), stepped up into piers at the (x, z) `piers` (every corner by
    default), and open at the (x, z) `gaps`."""
    cap = cap or slab(TRIM)
    b.fill((x0, y, z0), (x1, y, z1), "adobe")
    for v in vigas:
        if along == "z":
            b.fill((v, y, z0 - 1), (v, y, z1 + 1), "log[axis=z]")
        else:
            b.fill((x0 - 1, y, v), (x1 + 1, y, v), "log[axis=x]")
    for x in range(x0, x1 + 1):
        for z in range(z0, z1 + 1):
            if (x in (x0, x1) or z in (z0, z1)) and (x, z) not in gaps:
                b.set(x, y + 1, z, ADOBE)
                b.set(x, y + 2, z, cap)
    if piers == "all":
        piers = [(x, z) for x in (x0, x1) for z in (z0, z1)]
    for x, z in piers:
        b.set(x, y + 2, z, ADOBE)
        b.set(x, y + 3, z, cap)


def window(b, x, y, z, out, high=1):
    """A small window deep in the wall, its pane on a sandstone sill outside and under a log
    lintel."""
    dx, dz = {"north": (0, -1), "south": (0, 1), "east": (1, 0), "west": (-1, 0)}[out]
    for h in range(high):
        b.set(x, y + h, z, "glass_pane")
    b.set(x + dx, y - 1, z + dz, slab(TRIM, top=True))
    b.set(x, y + high, z, "log[axis=x]" if dz else "log[axis=z]")


def awning(b, x0, x1, z0, z1, y, posts, base=F):
    """An awning of plank slabs at layer `y` over the box from `x0`, `z0` to `x1`, `z1`, on fence
    posts at the (x, z) given, standing on layer `base`."""
    for x, z in posts:
        b.fill((x, base, z), (x, y - 1, z), POST)
    b.fill((x0, y, z0), (x1, y, z1), AWNING)


def oven(b, x, z, facing="south"):
    """A bread oven, a little dome of adobe over a furnace, its mouth facing `facing`."""
    b.fill((x - 1, F, z - 1), (x + 1, F, z + 1), "adobe")
    b.set(x, F, z, f"furnace[facing={facing}]")
    for dx, dz, f in ((0, -1, "south"), (0, 1, "north"), (-1, 0, "east"), (1, 0, "west")):
        b.set(x + dx, F + 1, z + dz, stairs("adobe", f))
    for dx in (-1, 1):
        for dz in (-1, 1):
            b.set(x + dx, F + 1, z + dz, slab("adobe"))
    b.set(x, F + 1, z, "adobe")
    b.set(x, F + 2, z, slab("adobe"))


def small_house():
    """A house for one: a block of adobe with vigas through its roof, a parapet stepped up at the
    corners, a stair up its east side to the roof, a chimney over the corner hearth, a bench by
    the door and a jar of water."""
    b = Piece(8, 10, 8)
    b.ground(YARD)
    x0, x1, z0, z1 = 0, 5, 1, 5
    box(b, x0, x1, z0, z1, F + 1, F + 3)
    band(b, x0, x1, z0, z1, F + 3)
    flat_roof(b, x0, x1, z0, z1, F + 4, vigas=(1, 3), gaps=((x1, 2),))
    # the door with a step up to it, a window either side
    b.door(2, F + 1, z1, "south")
    b.step_up(2, z1 + 1, TRIM)
    window(b, 4, F + 2, z1, "south")
    window(b, x0, F + 2, 3, "west")
    window(b, 3, F + 2, z0, "north")
    # the stair up the east side to the roof, built solid under it
    for i, z in enumerate(range(5, 1, -1)):
        b.set(x1 + 1, F + i, z, stairs("adobe", "north"))
        b.fill((x1 + 1, F, z), (x1 + 1, F + i - 1, z), "adobe")
    b.fill((x1 + 1, F, 1), (x1 + 1, F + 3, 1), "adobe")
    b.set(x1 + 1, F + 4, 1, slab("adobe"))
    # the corner hearth and its chimney through the roof
    b.set(1, F + 1, 2, "furnace[facing=south]")
    b.set(1, F + 2, 2, stairs("adobe", "north", top=True))
    b.chimney(1, 1, F + 3, F + 7, "adobe", pot=slab(TRIM))
    b.set(1, F + 3, 2, "adobe")
    # inside: the bed, a chest, a barrel of water, a lantern from the vigas
    b.set(4, F + 1, 2, "bed")
    b.mark("bed", 3, F + 1, 2)
    b.mark("loot", 4, F + 1, 3)
    b.set(1, F + 1, 4, "barrel[facing=up]")
    b.hang(3, F + 3, 3)
    # outside: a bench of sandstone stairs, a water jar, a lantern on the roof's edge
    b.set(0, F, 6, stairs(TRIM, "north"))
    b.set(1, F, 6, stairs(TRIM, "north"))
    b.set(4, F, 6, "barrel[facing=up]|air")
    b.set(3, F + 3, 6, "lantern[hanging=true]")
    b.set(3, F + 4, 6, "log[axis=z]")
    b.set(x1, F + 6, 4, "lantern")
    b.set(7, F, 2, "barrel[facing=up]")
    b.set(7, F + 1, 2, "lantern")
    b.mark("entrance", 2, F, 7)
    b.write_piece(
        "adobe_small_house", "house", STYLES, "sandstone",
        "An adobe house for one, a stair up its side to the roof.", SCRIPT,
    )


def house():
    """A house for two, stepping up: a block of rooms under a roof terrace, a roof room on its
    back corner with its own door onto the terrace and a shade over it, a stair up the east
    side, the vigas of both roofs standing out, and an awning on posts over the front door."""
    b = Piece(9, 13, 10)
    b.ground(YARD)
    x0, x1, z0, z1 = 0, 7, 1, 7
    box(b, x0, x1, z0, z1, F + 1, F + 3)
    band(b, x0, x1, z0, z1, F + 3)
    roof = F + 4
    flat_roof(b, x0, x1, z0, z1, roof, vigas=(1, 3, 6), gaps=((x1, 4),), piers=[(x1, z1), (x0, z1)])
    # the roof room on the back corner, its floor a string course of sandstone
    rx1, rz1 = 4, 4
    box(b, x0, rx1, z0, rz1, roof + 1, roof + 3)
    band(b, x0, rx1, z0, rz1, roof + 3)
    flat_roof(b, x0, rx1, z0, rz1, roof + 4, vigas=(1, 3))
    b.door(2, roof + 1, rz1, "south")
    window(b, x0, roof + 2, 2, "west")
    window(b, 3, roof + 2, z0, "north")
    # a shade over the terrace on tall posts
    awning(b, 5, 7, 5, 6, roof + 3, [(6, 5), (6, 6)], base=roof + 1)
    b.set(5, roof + 1, 6, stairs(TRIM, "west"))
    # the stair up the east side, built solid under it, to the gap in the parapet
    for i, z in enumerate(range(7, 3, -1)):
        b.set(x1 + 1, F + i, z, stairs("adobe", "north"))
        b.fill((x1 + 1, F, z), (x1 + 1, F + i - 1, z), "adobe")
    b.fill((x1 + 1, F, 3), (x1 + 1, F + 3, 3), "adobe")
    b.set(x1 + 1, F + 4, 3, slab(TRIM))
    # the front door under an awning, windows either side
    b.door(3, F + 1, z1, "south")
    b.step_up(3, z1 + 1, TRIM)
    awning(b, 2, 4, z1 + 1, z1 + 1, F + 3, [(2, z1 + 1), (4, z1 + 1)])
    window(b, 1, F + 2, z1, "south")
    window(b, 6, F + 2, z1, "south")
    window(b, x0, F + 2, 3, "west")
    window(b, x0, F + 2, 6, "west")
    window(b, x1, F + 2, 2, "east")
    # the hearth, its hood and chimney in the east wall
    b.set(6, F + 1, 3, "furnace[facing=west]")
    b.set(6, F + 2, 3, stairs("adobe", "east", top=True))
    b.chimney(x1, 3, F + 1, roof + 4, "adobe", pot=slab(TRIM))
    # downstairs: a bed, a table and chairs, a crafting table, barrels and the chest
    b.set(1, F + 1, 2, "bed")
    b.mark("bed", 2, F + 1, 2)
    b.table(4, 4)
    b.set(3, F + 1, 4, stairs("planks", "west"))
    b.set(5, F + 1, 4, stairs("planks", "east"))
    b.set(6, F + 1, 2, "crafting_table")
    b.set(6, F + 1, 5, "barrel[facing=up]")
    b.set(6, F + 1, 6, "barrel[facing=up]|air")
    b.mark("loot", 1, F + 1, 5)
    b.hang(3, roof - 1, 3)
    b.hang(4, roof - 1, 6)
    # the roof room: a bed, a barrel, a lantern; a lantern on the terrace
    b.set(1, roof + 1, 2, "bed")
    b.mark("bed", 2, roof + 1, 2)
    b.set(3, roof + 1, 2, "barrel[facing=up]")
    b.hang(2, roof + 3, 3)
    b.hang(6, roof + 2, 6)
    # the yard: a bench along the front, a jar, a lantern by the stair
    b.set(6, F, z1 + 1, stairs(TRIM, "north"))
    b.set(7, F, z1 + 1, stairs(TRIM, "north"))
    b.set(0, F, z1 + 1, "barrel[facing=up]|air")
    b.set(x1 + 1, F, 2, POST)
    b.set(x1 + 1, F + 1, 2, "lantern")
    b.mark("entrance", 3, F, 9)
    b.write_piece(
        "adobe_house", "house", STYLES, "sandstone",
        "An adobe house for two, a roof room over its terrace and an awning at the door.", SCRIPT,
    )


def courtyard_house():
    """A house for three round a courtyard: a long range of rooms across the back, a range down
    the west side rising at the front into a tower, and a wall round the rest with an arched
    gateway to the street. In the courtyard a bread oven and a fig tree."""
    b = Piece(11, 14, 12)
    b.ground(YARD)
    # the north range across the back, the west range down the side
    box(b, 0, 10, 1, 4, F + 1, F + 3)
    box(b, 0, 3, 4, 9, F + 1, F + 3)
    band(b, 0, 10, 1, 4, F + 3)
    band(b, 0, 3, 4, 9, F + 3)
    roof = F + 4
    flat_roof(b, 0, 10, 1, 4, roof, vigas=(2, 5, 8), piers=[(10, 1), (10, 4), (0, 1)])
    flat_roof(b, 0, 3, 4, 9, roof, vigas=(6,), along="x", piers=[])
    b.fill((1, F + 1, 4), (2, F + 3, 4), AIR)
    b.set(1, F + 1, 4, "adobe")
    b.fill((1, F + 3, 4), (2, F + 3, 4), "adobe")
    # the tower at the front of the west range, a storey higher
    box(b, 0, 3, 6, 9, roof + 1, roof + 3)
    band(b, 0, 3, 6, 9, roof + 3)
    flat_roof(b, 0, 3, 6, 9, roof + 4, vigas=(7,), along="x")
    window(b, 1, roof + 2, 9, "south")
    window(b, x=0, y=roof + 2, z=7, out="west")
    window(b, 3, roof + 2, 8, "east")
    b.hang(2, roof + 3, 7)
    # the courtyard wall, and the arched gateway in it
    for x in range(4, 11):
        b.fill((x, F, 9), (x, F + 1, 9), ADOBE)
        b.set(x, F + 2, 9, slab(TRIM))
    for z in range(5, 10):
        b.fill((10, F, z), (10, F + 1, z), ADOBE)
        b.set(10, F + 2, z, slab(TRIM))
    b.fill((6, F, 9), (7, F + 2, 9), AIR)
    for x in (5, 8):
        b.fill((x, F, 9), (x, F + 3, 9), ADOBE)
        b.set(x, F + 4, 9, "lantern")
    b.fill((6, F + 3, 9), (7, F + 3, 9), "log[axis=x]")
    b.set(6, F + 2, 9, stairs("adobe", "east", top=True))
    b.set(7, F + 2, 9, stairs("adobe", "west", top=True))
    b.ground("path", (6, 9), (7, 11))
    # doors onto the courtyard, steps up to them
    b.door(6, F + 1, 4, "south")
    b.step_up(6, 5, TRIM)
    b.door(3, F + 1, 7, "east")
    b.set(4, F, 7, stairs(TRIM, "west"))
    # windows to the street and the courtyard
    window(b, 1, F + 2, 9, "south")
    window(b, 0, F + 2, 6, "west")
    window(b, 3, F + 2, 1, "north")
    window(b, 8, F + 2, 1, "north")
    window(b, 9, F + 2, 4, "south")
    window(b, 3, F + 2, 5, "east")
    window(b, 10, F + 2, 2, "east")
    # the north range: two beds at the east end, a table, the hearth and the chest at the west
    b.set(9, F + 1, 2, "bed")
    b.mark("bed", 8, F + 1, 2)
    b.set(9, F + 1, 3, "bed")
    b.mark("bed", 8, F + 1, 3)
    b.set(1, F + 1, 2, "furnace[facing=east]")
    b.set(1, F + 2, 2, stairs("adobe", "west", top=True))
    b.chimney(0, 2, F + 1, roof + 4, "adobe", pot=slab(TRIM))
    b.set(1, F + 1, 3, "crafting_table")
    b.mark("loot", 4, F + 1, 2)
    b.table(6, 2)
    b.set(5, F + 1, 2, stairs("planks", "west"))
    b.set(7, F + 1, 2, stairs("planks", "east"))
    b.hang(3, roof - 1, 3)
    b.hang(8, roof - 1, 3)
    # the west range: a bed, barrels and a lantern
    b.set(1, F + 1, 5, "bed")
    b.mark("bed", 2, F + 1, 5)
    b.set(1, F + 1, 8, "barrel[facing=up]")
    b.set(2, F + 1, 8, "barrel[facing=up]|air")
    b.hang(2, roof - 1, 7)
    # the courtyard: the oven, the fig tree, a bench and a jar
    oven(b, 8, 7, facing="west")
    b.set(8, F + 3, 7, "lantern")
    b.fill((5, F, 6), (5, F + 2, 6), "log")
    b.fill((4, F + 3, 5), (6, F + 3, 7), "leaves")
    b.fill((5, F + 4, 5), (5, F + 4, 7), "leaves")
    b.fill((4, F + 4, 6), (6, F + 4, 6), "leaves")
    b.set(5, F + 3, 6, "log")
    b.set(4, F, 8, stairs(TRIM, "south"))
    b.set(5, F, 8, stairs(TRIM, "south"))
    b.set(9, F, 5, "barrel[facing=up]")
    b.mark("entrance", 6, F, 11)
    b.write_piece(
        "adobe_courtyard_house", "house", STYLES, "sandstone",
        "An adobe house for three round a courtyard, a tower over its gateway side.", SCRIPT,
    )


def hall():
    """The meeting hall: a tall hall of adobe under a sandstone dome on a drum, a portico of
    sandstone columns and arches along its front with steps up to it, solid towers at its
    front corners, and inside long tables, a counter with the keeper behind it, shelves set
    into the back wall and the chest."""
    b = Piece(13, 17, 13)
    b.ground(YARD)
    x0, x1, z0, z1 = 1, 11, 1, 8
    box(b, x0, x1, z0, z1, F + 1, F + 7)
    band(b, x0, x1, z0, z1, F + 4)
    band(b, x0, x1, z0, z1, F + 7)
    roof = F + 8
    flat_roof(b, x0, x1, z0, z1, roof, vigas=(2, 4, 8, 10), piers=[])
    # the dome on its drum, open into the hall under it
    b.fill((4, roof + 1, 2), (8, roof + 2, 6), ADOBE)
    band(b, 4, 8, 2, 6, roof + 2, TERRA)
    b.fill((5, roof, 3), (7, roof + 3, 5), AIR)
    for x, z in ((6, 2), (6, 6), (4, 4), (8, 4)):
        b.set(x, roof + 1, z, "glass_pane")
        b.set(x, roof + 2, z, "glass_pane")
    top = b.spire(4, 8, 2, 6, roof + 3, lambda f: stairs(TRIM, f), TRIM, slab(TRIM), overhang=0,
                  steep=False)
    for x, z in ((4, 2), (8, 2), (4, 6), (8, 6)):
        b.set(x, roof + 3, z, TRIM)
        b.set(x, roof + 4, z, slab(TRIM))
    b.set(6, top, 4, TRIM)
    b.set(6, top + 1, 4, "sandstone_wall")
    b.set(6, top + 2, 4, "gold_block")
    b.hang(6, top - 1, 4)
    # the towers at the front corners, solid, with blind windows and stepped tops
    for tx in (0, 10):
        b.fill((tx, F, 8), (tx + 2, roof + 1, 10), ADOBE)
        b.fill((tx, F - 1, 8), (tx + 2, F - 1, 10), TRIM)
        band(b, tx, tx + 2, 8, 10, F + 4)
        band(b, tx, tx + 2, 8, 10, F + 7)
        for y in (F + 2, F + 5):
            b.set(tx + 1, y, 10, "glass_pane")
        for x in range(tx, tx + 3):
            for z in range(8, 11):
                if x == tx + 1 and z == 9:
                    b.set(x, roof + 2, z, ADOBE)
                    b.set(x, roof + 3, z, slab(TRIM))
                elif (x == tx + 1) != (z == 9):
                    b.set(x, roof + 2, z, slab(TRIM))
                else:
                    b.set(x, roof + 2, z, ADOBE)
                    b.set(x, roof + 3, z, slab(TRIM))
    # the portico: a raised floor, columns, arches of upturned stairs and a roof over them
    b.fill((3, F, 9), (9, F, 10), TRIM)
    for x in range(3, 10):
        b.set(x, F, 11, stairs(TRIM, "north"))
    for x in (3, 6, 9):
        if x != 6:
            b.fill((x, F + 1, 10), (x, F + 4, 10), TRIM)
    b.fill((3, F + 5, 9), (9, F + 5, 10), "adobe")
    for x in range(3, 10):
        b.set(x, F + 6, 10, ADOBE)
        b.set(x, F + 7, 10, slab(TRIM))
        b.set(x, F + 5, 11, "log[axis=z]" if x % 2 == 1 else None)
    for x, f in ((4, "west"), (5, "east"), (7, "west"), (8, "east")):
        b.set(x, F + 4, 10, stairs(TRIM, f, top=True))
    b.hang(5, F + 4, 9)
    b.hang(7, F + 4, 9)
    # the door in a sandstone surround, windows high up either side
    b.door(6, F + 1, z1, "south")
    b.fill((5, F + 1, z1), (5, F + 3, z1), TRIM)
    b.fill((7, F + 1, z1), (7, F + 3, z1), TRIM)
    b.set(6, F + 3, z1, TRIM)
    for x in (4, 8):
        window(b, x, F + 5, z1, "south", high=2)
    for z in (3, 6):
        window(b, x0, F + 5, z, "west", high=2)
        window(b, x1, F + 5, z, "east", high=2)
    for x in (3, 9):
        window(b, x, F + 5, z0, "north", high=2)
    # inside: the counter across the back, the keeper behind it, shelves set into the wall
    b.fill((4, F + 1, 3), (8, F + 1, 3), TRIM)
    b.set(4, F + 2, 3, "lantern")
    b.mark("work", 6, F + 1, 2)
    for x in range(3, 10):
        b.set(x, F + 2, z0, "bookshelf" if x % 3 == 0 else "barrel[facing=south]")
        b.set(x, F + 3, z0, "barrel[facing=south]*2|bookshelf")
    b.set(2, F + 1, 2, "barrel[facing=up]")
    b.set(10, F + 1, 2, "barrel[facing=up]")
    b.mark("loot", 10, F + 1, 7)
    # two long tables down the hall, benches either side
    for tx in (3, 8):
        for x in (tx, tx + 1):
            b.table(x, 5)
            b.set(x, F + 1, 4, stairs("planks", "north"))
            b.set(x, F + 1, 6, stairs("planks", "south"))
    b.mark("work", 6, F + 1, 6)
    for x, z in ((3, 3), (9, 3), (3, 7), (9, 7), (6, 5)):
        b.hang(x, roof - 1, z)
    # lanterns on brackets round the outside
    for x, z, out in ((3, z0 - 1, "north"), (9, z0 - 1, "north"), (x0 - 1, 4, "west"),
                      (x1 + 1, 4, "east")):
        b.bracket_lamp(x, F + 3, z, out, material=TRIM)
    b.mark("entrance", 6, F, 12)
    b.write_piece(
        "adobe_hall", "hall", STYLES, "sandstone",
        "The adobe village's meeting hall under a dome, a portico of columns along its front.",
        SCRIPT,
    )


def smithy():
    """The smithy, a walled yard: the forge under a flat roof against the back wall, its
    furnace in an adobe hearth under a tall chimney, an awning on posts over the anvil and the
    quenching trough, ore heaped by the wall, and the chest."""
    b = Piece(10, 13, 10)
    b.ground(YARD)
    b.ground("path*3|gravel*2|sand", (1, 1), (8, 7))
    # the yard walls: the back and west high, the east low, the front low with the way in
    for x in range(0, 10):
        b.fill((x, F, 0), (x, F + 3, 0), ADOBE)
    for z in range(0, 9):
        b.fill((0, F, z), (0, F + 2, z), ADOBE)
        b.fill((9, F, z), (9, F + 1, z), ADOBE)
        b.set(9, F + 2, z, slab(TRIM))
    for x in range(0, 10):
        if not 4 <= x <= 5:
            b.fill((x, F, 8), (x, F + 1, 8), ADOBE)
            b.set(x, F + 2, 8, slab(TRIM))
    for z in range(1, 8):
        b.set(0, F + 3, z, slab(TRIM))
    band(b, 0, 9, 0, 0, F + 2)
    # the forge: a roof over the west half of the back, on a sandstone pillar
    b.fill((5, F, 3), (5, F + 3, 3), TRIM)
    b.fill((0, F + 4, 0), (5, F + 4, 3), "adobe")
    for x in range(0, 6):
        b.set(x, F + 5, 3, ADOBE)
        b.set(x, F + 4, 4, "log[axis=z]" if x % 2 == 1 else None)
    b.fill((0, F + 5, 0), (0, F + 5, 3), ADOBE)
    b.fill((1, F + 5, 0), (5, F + 5, 0), ADOBE)
    # the hearth and its chimney, tall over the back wall
    b.fill((1, F, 1), (3, F + 1, 1), "adobe")
    b.set(2, F, 1, "furnace[facing=south]")
    b.set(3, F, 1, "furnace[facing=south]")
    b.set(2, F + 1, 2, stairs("adobe", "north", top=True))
    b.set(3, F + 1, 2, stairs("adobe", "north", top=True))
    b.fill((2, F + 2, 1), (3, F + 3, 1), "adobe")
    b.fill((2, F + 5, 0), (3, F + 9, 1), ADOBE)
    b.set(2, F + 10, 0, slab(TRIM))
    b.set(3, F + 10, 1, slab(TRIM))
    b.fill((2, F + 10, 1), (3, F + 10, 1), slab(TRIM))
    b.fill((2, F + 10, 0), (3, F + 10, 0), slab(TRIM))
    band(b, 2, 3, 0, 1, F + 8)
    b.set(1, F, 2, "barrel[facing=up]")
    b.mark("work", 2, F, 3)
    b.hang(3, F + 3, 2)
    # the anvil, the trough and the awning over them
    b.set(4, F, 5, "cobblestone_wall")
    b.set(4, F + 1, 5, slab("stone_bricks"))
    b.mark("work", 4, F, 6)
    b.fill((6, F, 1), (8, F, 2), TRIM)
    b.set(7, F, 1, "water")
    b.set(8, F, 1, "water")
    awning(b, 6, 8, 1, 5, F + 3, [(6, 5), (8, 5)])
    b.hang(7, F + 2, 4)
    # ore and coal by the west wall, the chest, a crafting table and barrels
    b.set(1, F, 5, "coal_ore*2|iron_ore|gravel")
    b.set(1, F, 6, "coal_ore|iron_ore*2|cobblestone")
    b.set(1, F + 1, 6, "coal_ore|air*2")
    b.set(1, F, 7, "crafting_table")
    b.mark("loot", 8, F, 6)
    b.set(8, F, 7, "barrel[facing=up]")
    b.set(8, F + 1, 7, "lantern")
    for x in (3, 6):
        b.set(x, F + 2, 8, "lantern")
    b.ground("path", (4, 8), (5, 9))
    b.mark("entrance", 4, F, 9)
    b.write_piece(
        "adobe_smithy", "smithy", STYLES, "sandstone",
        "The adobe village's smithy, a walled yard round a forge under a tall chimney.", SCRIPT,
    )


def tower():
    """The watchtower: a shaft of adobe on a sloping sandstone foot, banded in terracotta, vigas
    standing out of it at two heights, a lookout behind a parapet with piers, and over it a bell
    under a little pyramid of sandstone on four posts. A ladder climbs inside; the watchman
    keeps its foot."""
    b = Piece(7, 23, 7)
    b.ground(YARD)
    x0, x1, z0, z1 = 1, 5, 0, 4
    b.fill((x0, 0, z0), (x1, 0, z1), TRIM)
    for x in range(x0 - 1, x1 + 2):
        if x != 3:
            b.set(x, F, z1 + 1, stairs(TRIM, "north"))
    for z in range(z0, z1 + 1):
        b.set(x0 - 1, F, z, stairs(TRIM, "east"))
        b.set(x1 + 1, F, z, stairs(TRIM, "west"))
    shaft = 14
    for y in range(F, F + shaft):
        b.walls((x0, y, z0), (x1, y, z1), ADOBE if y > F else TRIM)
    for y in (F + 5, F + 10):
        band(b, x0, x1, z0, z1, y)
    b.fill((x0 + 1, 0, z0 + 1), (x1 - 1, 0, z1 - 1), TILES)
    # vigas standing out of the sides at two heights
    for y in (F + 6, F + 11):
        for z in (1, 3):
            b.set(x0 - 1, y, z, "log[axis=x]")
            b.set(x1 + 1, y, z, "log[axis=x]")
    # the door, slits and a lantern on a bracket
    b.door(3, F, z1, "south")
    b.bracket_lamp(4, F + 2, z1 + 1, "south", material=TRIM)
    for y in (F + 4, F + 8):
        b.set(3, y, z0, "glass_pane")
        b.set(x0, y, 2, "glass_pane")
        b.set(x1, y, 2, "glass_pane")
    b.set(3, F + 7, z1, "glass_pane")
    # inside: the ladder, the watchman's barrel, beams with lanterns up the shaft
    b.ladder(3, z0 + 1, F, F + shaft, "south")
    b.set(2, F, 3, "barrel[facing=up]")
    b.mark("work", 4, F, 2)
    for y in (F + 4, F + 10):
        b.fill((x0 + 1, y, 3), (x1 - 1, y, 3), "log[axis=x]")
        b.hang(2, y - 1, 3)
    # the lookout floor and its parapet
    look = F + shaft
    b.fill((x0, look, z0), (x1, look, z1), "adobe")
    b.set(3, look, 1, "ladder[facing=south]")
    flat_roof(b, x0, x1, z0, z1, look)
    b.fill((x0 + 1, look + 1, z0 + 1), (x1 - 1, look + 2, z1 - 1), AIR)
    # the bell under its pyramid on four posts
    for x in (2, 4):
        for z in (1, 3):
            b.fill((x, look + 1, z), (x, look + 3, z), "sandstone_wall")
    top = b.spire(2, 4, 1, 3, look + 4, lambda f: stairs(TRIM, f), TRIM, slab(TRIM),
                  steep=False)
    b.fill((2, look + 4, 2), (4, look + 4, 2), "log[axis=x]")
    b.set(3, look + 3, 2, "gold_block")
    b.set(2, look + 2, 3, "lantern")
    b.mark("entrance", 3, F, 6)
    b.write_piece(
        "adobe_tower", "tower", STYLES, "sandstone",
        "The adobe village's watchtower, a bell under a little pyramid over its lookout.",
        SCRIPT,
    )


def fountain():
    """The fountain at the village's centre: a basin of sandstone with a bowl on a column in
    its middle, lanterns on its corners, on a square paved in terracotta and sandstone with a
    palm at each corner, where the villagers meet."""
    b = Piece(9, 9, 9)
    b.ground("path*4|sand")
    b.ground("terracotta*2|sandstone*2|" + "red_terracotta", (1, 1), (7, 7))
    # the basin and the water in it
    for x in range(2, 7):
        for z in range(2, 7):
            if x in (2, 6) or z in (2, 6):
                b.set(x, F, z, TRIM)
            else:
                b.set(x, F - 1, z, TRIM)
                b.set(x, F, z, "water")
    for x, z in ((2, 2), (6, 2), (2, 6), (6, 6)):
        b.set(x, F + 1, z, "lantern")
    # the column and its bowl, water standing in it
    b.fill((4, F, 4), (4, F + 1, 4), "sandstone_wall")
    b.set(4, F + 2, 4, TRIM)
    b.set(4, F + 3, 4, "water")
    for dx, dz, f in ((1, 0, "east"), (-1, 0, "west"), (0, 1, "south"), (0, -1, "north")):
        b.set(4 + dx, F + 3, 4 + dz, stairs(TRIM, f))
    # palms at the corners of the paving, their crowns drooping over it
    for x, z in ((1, 1), (7, 1), (1, 7), (7, 7)):
        b.fill((x, F, z), (x, F + 3, z), "log")
        b.set(x, F + 4, z, "leaves")
        for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            b.set(x + dx, F + 4, z + dz, "leaves")
    for x, z in ((3, 1), (5, 1), (1, 3), (1, 5), (7, 3), (7, 5), (3, 7), (5, 7)):
        b.mark("gather", x, F, z)
    b.mark("entrance", 4, F, 8)
    b.write_piece(
        "adobe_fountain", "well", STYLES, "sandstone",
        "The adobe village's fountain, a basin and a bowl on a paved square under palms.", SCRIPT,
    )


def lamp():
    """A lamp post by the street: a column of sandstone wall on a sandstone foot, a lantern on
    top."""
    b = Piece(1, 6, 1)
    b.ground("path")
    b.set(0, F, 0, TRIM)
    b.fill((0, F + 1, 0), (0, F + 2, 0), "sandstone_wall")
    b.set(0, F + 3, 0, "lantern")
    b.write_piece(
        "adobe_lamp", "lamp", STYLES, "sandstone",
        "A lamp post of the adobe village, a lantern on a sandstone column.", SCRIPT,
    )


def farm():
    """A walled field of wheat at every stage, watered by two channels, a shade of plank slabs
    on posts over straw and jars at the front, and lanterns on sandstone posts at the corners
    and either side of the opening."""
    b = Piece(13, 6, 11)
    b.ground(YARD)
    b.ground("farmland", (1, 1), (11, 6))
    for x in (4, 8):
        b.ground("water", (x, 1), (x, 6))
    b.ground("path*4|sand", (1, 7), (11, 8))
    for x in range(1, 12):
        for z in range(1, 7):
            if x not in (4, 8):
                b.set(x, F, z, "wheat_0|wheat_1|wheat_2*2|wheat_3*2|wheat_4*2|wheat_5*2|wheat_6*2|ripe_wheat*3")
    # the low wall, capped with sandstone, open in the middle of the front
    for x in range(0, 13):
        for z in range(0, 10):
            if x in (0, 12) or z in (0, 9):
                if z == 9 and 5 <= x <= 7:
                    continue
                b.set(x, F, z, ADOBE)
                b.set(x, F + 1, z, slab(TRIM))
    for x, z in ((0, 0), (12, 0), (6, 0), (4, 9), (8, 9), (0, 9), (12, 9)):
        b.set(x, F + 1, z, "sandstone_wall")
        b.set(x, F + 2, z, "lantern")
    b.ground("path", (5, 9), (7, 10))
    # the shade over straw and jars
    awning(b, 1, 3, 7, 8, F + 2, [(3, 7)])
    b.set(1, F, 8, "thatch")
    b.set(2, F, 8, "thatch|barrel[facing=up]")
    b.set(1, F + 1, 8, "thatch|air")
    b.set(1, F, 7, "barrel[facing=up]")
    b.set(11, F, 8, "barrel[facing=up]|air")
    # planks across the channels at the back, for the harvest
    for x in (4, 8):
        b.set(x, 0, 1, "planks")
    b.mark("work", 2, F, 3)
    b.mark("work", 10, F, 5)
    b.mark("work", 6, F, 7)
    b.mark("entrance", 6, F, 10)
    b.write_piece(
        "adobe_farm", "farm", STYLES, "sandstone",
        "A walled field of wheat watered by two channels, a shade at its front.", SCRIPT,
    )


def pen():
    """A round corral of adobe topped with a fence, too high to hop and closed all round, a
    trough and straw inside, and the herder's place outside by a jar of feed."""
    b = Piece(9, 6, 9)
    b.ground(YARD)
    cx, cz = 4, 3

    def inside(x, z):
        dx, dz = abs(x - cx), abs(z - cz)
        return dx <= 3 and dz <= 2 and dx + dz <= 4

    for x in range(9):
        for z in range(7):
            if inside(x, z):
                b.set(x, F - 1, z, "path*3|sand|grass")
            elif any(inside(x + dx, z + dz) for dx in (-1, 0, 1) for dz in (-1, 0, 1)):
                b.set(x, F, z, ADOBE)
                b.set(x, F + 1, z, POST)
    for x, z in ((4, 0), (0, 3), (8, 3), (4, 6)):
        b.set(x, F + 2, z, "lantern")
    # the trough and the straw, kept off the wall
    b.set(2, F, 3, "water")
    b.set(3, F, 3, "water")
    b.set(2, F, 4, slab(TRIM))
    b.set(3, F, 4, slab(TRIM))
    b.set(6, F, 2, "thatch")
    b.set(5, F, 2, "thatch|air")
    for x, z in ((3, 2), (5, 4), (6, 3), (4, 3)):
        b.mark("animal", x, F, z)
    b.ground("path*3|sand", (0, 7), (8, 8))
    # the barrels stand off the wall, which they'd make a step onto
    b.set(0, F, 8, "barrel[facing=up]")
    b.set(8, F, 8, "barrel[facing=up]|air")
    b.mark("work", 3, F, 7)
    b.mark("entrance", 4, F, 8)
    b.write_piece(
        "adobe_pen", "pen", STYLES, "sandstone",
        "A round corral of adobe topped with a fence, a trough and straw inside.", SCRIPT,
    )


small_house()
house()
courtyard_house()
hall()
smithy()
tower()
fountain()
lamp()
farm()
pen()
