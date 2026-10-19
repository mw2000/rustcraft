"""Writes the lodge village's pieces into assets/structures/village/: the village of the taiga
and the cold, of timber on heavy stone under steep roofs hung low against the snow.

The style: stone bases standing up two blocks or more round each building, walls of dark
planks (or plain planks, for some) in frames of logs, steep roofs of dark planks climbing two
layers to each row with deep overhangs, patched with snow and capped with a snowy ridge (or
not, for each building), crossed horns over the gables, big stone chimneys, porches of log
posts and decks with rails, small windows, warm lanterns, and woodpiles stacked under the
eaves.

    python3 .claude/skills/structures/village_lodge.py
"""

from village import AIR, F, Piece, slab, stairs

SCRIPT = "village_lodge.py"
STYLES = ["lodge"]

# Fieldstone and stone brick, weathered; the walls' planks for the whole building (of 3); the
# ridge's snow, or none (of 2), apart from it.
STONE = "stone_bricks*3|cobblestone*3|mossy_cobblestone|mossy_stone_bricks|cracked_stone_bricks"
WALL = "dark_planks*2/planks"
ROOF_FULL = "dark_planks"
ROOF_CAP = "dark_planks_slab"
RIDGE = "snow/air"
FENCE = "dark_planks_fence"


def roof_stair(facing):
    """A roof's stair, of dark planks with snow lying here and there."""
    return f"dark_planks_stairs[facing={facing}]*14|snow"


def horns(b, x, z, cap, along):
    """Crossed horns over a gable at the end of a ridge running `along` x or z, its cap at `x`,
    `z` on layer `cap`: fence posts two high standing on the roof either side of the cap."""
    for d in (-1, 1):
        at = (x + d, z) if along == "z" else (x, z + d)
        b.fill((at[0], cap - 1, at[1]), (at[0], cap, at[1]), FENCE)


def lodge_walls(b, x0, x1, z0, z1, base, top, posts_x=(), posts_z=()):
    """Walls round the box from `x0`, `z0` to `x1`, `z1`: stone from the floor layer `F` up to
    layer `base`, timber over it up to the log plate at layer `top`, with log posts at the
    corners and at `posts_x` and `posts_z`, and a plank floor inside; cleared within."""
    b.fill((x0, 0, z0), (x1, F, z1), STONE)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F, z1 - 1), "planks")
    for y in range(F + 1, top + 1):
        b.walls((x0, y, z0), (x1, y, z1), STONE if y <= base else WALL)
    b.frame(x0, x1, z0, z1, base + 1, top, WALL, xs=posts_x, zs=posts_z)
    b.fill((x0 + 1, F + 1, z0 + 1), (x1 - 1, top, z1 - 1), AIR)


def window(b, x, y, z, out):
    """A small window, under a log lintel."""
    b.set(x, y, z, "glass_pane")
    if b.get(x, y + 1, z) not in ("log", None):
        b.set(x, y + 1, z, "log[axis=x]" if out in ("north", "south") else "log[axis=z]")


def woodpile(b, x0, x1, z0, z1, high, axis):
    """Logs stacked `high` from the floor over the box from `x0`, `z0` to `x1`, `z1`, the top
    course with gaps."""
    b.fill((x0, F, z0), (x1, F + high - 2, z1), f"log[axis={axis}]")
    b.fill((x0, F + high - 1, z0), (x1, F + high - 1, z1), f"log[axis={axis}]*3|air")


def cabin():
    """A cabin for one: a stone base two high under plank walls in a log frame, a steep roof
    along its length over a stone west gable and chimney, and a porch along the front, its deck
    railed and its lean-to roof carrying on the main roof's slope, with logs stacked by the
    door."""
    b = Piece(7, 15, 9)
    # the yard is the world's own ground, snow or grass as the taiga has it
    b.ground(None)
    x0, x1, z0, z1 = 0, 6, 1, 5
    eaves = F + 4
    lodge_walls(b, x0, x1, z0, z1, F + 1, eaves, posts_z=(3,))
    b.fill((x0, F + 1, z0 + 1), (x0, eaves, z1 - 1), STONE)
    top = b.roof(x0, x1, z0, z1, eaves, roof_stair, ROOF_FULL, ROOF_CAP, steep=True, gable=WALL,
                 ridge=RIDGE)
    b.fill((x0, eaves, z0 + 1), (x0, top - 2, z1 - 1), STONE)
    b.stack(x0, 3, F + 1, top + 1, STONE, breast=[(x0, 2), (x0, 4)], shoulder=eaves + 2,
            pot=slab("stone_bricks"))
    horns(b, x1, 3, top - 1, "x")
    b.king_post(x1, 3, eaves, top, WALL)
    # the door and small windows
    b.door(3, F + 1, z1, "south")
    window(b, 1, F + 2, z1, "south")
    window(b, 5, F + 2, z1, "south")
    window(b, x1, F + 2, 2, "east")
    window(b, x1, F + 2, 4, "east")
    window(b, 3, F + 2, z0, "north")
    window(b, x1, eaves + 2, 3, "east")
    # the porch: a deck, a step at the door, log posts and rails, a lean-to roof
    b.fill((x0, F, 6), (x1, F, 7), "planks")
    b.set(3, F, 7, stairs("planks", "north"))
    for x in (x0, x1):
        b.fill((x, F + 1, 7), (x, F + 2, 7), "log")
    for x in (1, 2, 4, 5):
        b.set(x, F + 1, 7, FENCE)
    b.fill((x0, F + 3, 7), (x1, F + 3, 7), roof_stair("north"))
    b.hang(1, F + 3, 6)
    woodpile(b, 4, 5, 6, 6, 2, "x")
    # inside: the hearth, the bed, a table, the chest, a lantern from the tie beam
    b.set(1, F + 1, 3, "furnace[facing=east]")
    b.set(1, F + 1, 2, "crafting_table")
    b.set(1, F + 1, 4, "barrel[facing=up]")
    b.set(5, F + 1, 2, "bed")
    b.mark("bed", 4, F + 1, 2)
    b.mark("loot", 5, F + 1, 4)
    b.table(4, 4)
    b.fill((x0 + 1, eaves, 3), (x1 - 1, eaves, 3), "log[axis=x]")
    b.hang(3, eaves - 1, 3)
    b.bracket_lamp(3, F + 2, z0 - 1, "north")
    b.ground("path", (3, 8), (3, 8))
    b.mark("entrance", 3, F, 8)
    b.write_piece(
        "lodge_cabin", "house", STYLES, "stone_bricks",
        "A log-framed cabin for one on a stone base, a railed porch along its front.", SCRIPT,
    )


def house():
    """A house for two, its gable to the street: a stone base under timber walls, a steep roof
    running back from the street and hanging two blocks out over a porch on log posts, the
    gable over the porch boarded and crowned with horns, a stone chimney up its east side and a
    woodpile along its west."""
    b = Piece(9, 17, 11)
    # the yard is the world's own ground, snow or grass as the taiga has it
    b.ground(None)
    x0, x1, z0, z1 = 1, 7, 1, 7
    eaves = F + 5
    lodge_walls(b, x0, x1, z0, z1, F + 1, eaves, posts_z=(4,), posts_x=(4,))
    top = b.roof(x0, x1, z0, z1, eaves, roof_stair, ROOF_FULL, ROOF_CAP, along="z", ends=2,
                 steep=True, gable=WALL, ridge=RIDGE)
    # the porch under the roof's overhang: a deck, posts, a beam and the boarded gable over it
    porch = z1 + 2
    b.fill((x0, F, z1 + 1), (x1, F, porch), "planks")
    b.set(4, F, porch, stairs("planks", "north"))
    for x in (x0, x1):
        b.fill((x, F + 1, porch), (x, eaves - 1, porch), "log")
    b.fill((x0, eaves, porch), (x1, eaves, porch), "log[axis=x]")
    for y in range(eaves + 1, top):
        for x in range(x0, x1 + 1):
            if b.get(x, y, z1) == WALL:
                b.set(x, y, porch, WALL)
    for x in (2, 3, 5, 6):
        b.set(x, F + 1, porch, FENCE)
    b.king_post(4, porch, eaves, top, WALL)
    b.king_post(4, z1, eaves + 3, top, WALL)
    b.king_post(4, z0, eaves, top, WALL)
    horns(b, 4, porch, top - 1, "z")
    b.hang(2, eaves - 1, z1 + 1)
    b.hang(6, eaves - 1, z1 + 1)
    # the door, windows, one in the gable
    b.door(4, F + 1, z1, "south")
    window(b, 2, F + 2, z1, "south")
    window(b, 6, F + 2, z1, "south")
    window(b, 4, eaves + 2, z1, "south")
    window(b, x0, F + 2, 3, "west")
    window(b, x0, F + 2, 6, "west")
    window(b, x1, F + 2, 2, "east")
    window(b, x1, F + 2, 6, "east")
    # the chimney up the east side, the hearth inside it
    b.fill((x1, F + 1, 4), (x1, eaves, 4), STONE)
    b.stack(x1 + 1, 4, 0, top, STONE, breast=[(x1 + 1, 3), (x1 + 1, 5)], shoulder=F + 4,
            pot=slab("stone_bricks"))
    b.set(x1 + 1, F + 5, 3, slab("stone_bricks"))
    b.set(x1 + 1, F + 5, 5, slab("stone_bricks"))
    b.set(x1 - 1, F + 1, 4, "furnace[facing=west]")
    # the woodpile along the west wall, under the eaves
    woodpile(b, 0, 0, 2, 6, 3, "z")
    # inside: two beds, a table and chairs, the chest, barrels and lanterns on a tie beam
    b.set(2, F + 1, 2, "bed")
    b.mark("bed", 3, F + 1, 2)
    b.set(2, F + 1, 4, "bed")
    b.mark("bed", 3, F + 1, 4)
    b.table(4, 5)
    b.set(3, F + 1, 5, stairs("dark_planks", "west"))
    b.set(5, F + 1, 5, stairs("dark_planks", "east"))
    b.mark("loot", 6, F + 1, 2)
    b.set(6, F + 1, 6, "barrel[facing=up]")
    b.set(2, F + 1, 6, "crafting_table")
    b.fill((x0 + 1, eaves, 4), (x1 - 1, eaves, 4), "log[axis=x]")
    b.hang(4, eaves - 1, 4)
    b.bracket_lamp(4, F + 2, z0 - 1, "north")
    b.ground("path", (4, 10), (4, 10))
    b.mark("entrance", 4, F, 10)
    b.write_piece(
        "lodge_house", "house", STYLES, "stone_bricks",
        "A lodge for two, its steep roof hung out over a porch on log posts.", SCRIPT,
    )


def homestead():
    """A house for three, L-shaped: a long range along the back with stone gables and a
    chimney at each end, and a wing coming forward to the street at its east end, the roofs
    meeting in a valley, a porch with a lean-to roof in the angle, and logs stacked against the
    wing."""
    b = Piece(11, 17, 12)
    # the yard is the world's own ground, snow or grass as the taiga has it
    b.ground(None)
    mx0, mx1, mz0, mz1 = 0, 10, 1, 5
    wx0, wx1, wz0, wz1 = 5, 9, 5, 9
    eaves = F + 4
    lodge_walls(b, mx0, mx1, mz0, mz1, F + 1, eaves, posts_x=(3, 7))
    lodge_walls(b, wx0, wx1, wz0, wz1, F + 1, eaves, posts_z=(7,))
    b.fill((wx0 + 1, F + 1, wz0), (wx1 - 1, eaves - 1, wz0), AIR)
    b.fill((mx0 + 1, F + 1, mz0 + 1), (mx1 - 1, eaves, mz1 - 1), AIR)
    for x in (mx0, mx1):
        b.fill((x, F + 1, mz0 + 1), (x, eaves, mz1 - 1), STONE)
    top = b.roofs(
        dict(x0=mx0, x1=mx1, z0=mz0, z1=mz1, y=eaves, stair=roof_stair, full=ROOF_FULL,
             cap=ROOF_CAP, steep=True, gable=STONE, ridge=RIDGE),
        dict(x0=wx0, x1=wx1, z0=wz0, z1=wz1, y=eaves, stair=roof_stair, full=ROOF_FULL,
             cap=ROOF_CAP, along="z", ends=1, steep=True, gable=WALL, ridge=RIDGE),
    )
    for x in (mx0, mx1):
        b.stack(x, 3, F + 1, top + 1, STONE, pot=slab("stone_bricks"))
    b.king_post(7, wz1, eaves, top, WALL)
    horns(b, 7, wz1 + 1, top - 1, "z")
    # the doors and windows
    b.door(2, F + 1, mz1, "south")
    window(b, 4, F + 2, mz1, "south")
    window(b, 6, F + 2, wz1, "south")
    window(b, 8, F + 2, wz1, "south")
    window(b, 7, eaves + 2, wz1, "south")
    window(b, wx1, F + 2, 7, "east")
    window(b, 2, F + 2, mz0, "north")
    window(b, 5, F + 2, mz0, "north")
    window(b, 8, F + 2, mz0, "north")
    # the porch in the angle: a deck, a step, posts and rails, a lean-to roof
    b.fill((mx0, F, 6), (wx0 - 1, F, 7), "planks")
    b.set(2, F, 7, stairs("planks", "north"))
    b.fill((mx0, F + 1, 7), (mx0, F + 2, 7), "log")
    b.fill((4, F + 1, 7), (4, F + 2, 7), "log")
    b.set(1, F + 1, 7, FENCE)
    b.set(3, F + 1, 7, FENCE)
    b.fill((mx0, F + 3, 7), (wx0 - 1, F + 3, 7), roof_stair("north"))
    b.hang(1, F + 3, 6)
    b.hang(3, F + 3, 6)
    # the logs against the wing's east side, under its eaves
    woodpile(b, 10, 10, 6, 9, 3, "z")
    # inside the range: the hearths at both ends, two beds, a table and the chest
    b.set(1, F + 1, 3, "furnace[facing=east]")
    b.set(9, F + 1, 3, "furnace[facing=west]")
    b.set(1, F + 1, 2, "bed")
    b.mark("bed", 2, F + 1, 2)
    b.set(9, F + 1, 2, "bed")
    b.mark("bed", 8, F + 1, 2)
    b.table(5, 3)
    b.set(4, F + 1, 3, stairs("dark_planks", "west"))
    b.set(6, F + 1, 3, stairs("dark_planks", "east"))
    b.mark("loot", 1, F + 1, 4)
    b.set(9, F + 1, 4, "barrel[facing=up]")
    b.fill((mx0 + 1, eaves, 3), (mx1 - 1, eaves, 3), "log[axis=x]")
    for x in (3, 7):
        b.hang(x, eaves - 1, 3)
    # inside the wing: a bed, a bookshelf, a crafting table and barrels
    b.set(8, F + 1, 8, "bed")
    b.mark("bed", 7, F + 1, 8)
    b.set(6, F + 1, 8, "crafting_table")
    b.set(8, F + 1, 6, "bookshelf")
    b.set(6, F + 1, 6, "barrel[facing=up]|air")
    b.fill((wx0 + 1, eaves, 7), (wx1 - 1, eaves, 7), "log[axis=x]")
    b.hang(7, eaves - 1, 7)
    b.bracket_lamp(5, F + 2, mz0 - 1, "north")
    b.ground("path", (2, 8), (2, 11))
    b.set(0, F, 9, FENCE)
    b.set(0, F + 1, 9, "lantern")
    b.mark("entrance", 2, F, 11)
    b.write_piece(
        "lodge_homestead", "house", STYLES, "stone_bricks",
        "A lodge for three, a long range and a wing round a porch, a chimney at each end.",
        SCRIPT,
    )


def hall():
    """The mead hall: a long hall, its gable to the street, on a stone base three high, its
    steep roof hanging two blocks out over its sides and over a porch on log posts at its
    front, crowned with horns; inside a long hearth of embers between tables and benches, a
    fireplace and the high seat at the back, lit by lanterns from the tie beams."""
    b = Piece(13, 22, 13)
    # the yard is the world's own ground, snow or grass as the taiga has it
    b.ground(None)
    x0, x1, z0, z1 = 2, 10, 1, 9
    eaves = F + 6
    lodge_walls(b, x0, x1, z0, z1, F + 2, eaves, posts_x=(6,), posts_z=(3, 5, 7))
    top = b.roof(x0, x1, z0, z1, eaves, roof_stair, ROOF_FULL, ROOF_CAP, along="z", overhang=2,
                 ends=2, steep=True, gable=WALL, ridge=RIDGE)
    # the porch under the front overhang, its gable boarded over a beam
    porch = z1 + 2
    b.fill((x0, F, z1 + 1), (x1, F, porch), "planks")
    b.set(6, F, porch, stairs("planks", "north"))
    for x in (x0, 6 - 2, 6 + 2, x1):
        b.fill((x, F + 1, porch), (x, eaves - 1, porch), "log")
    b.fill((x0, eaves, porch), (x1, eaves, porch), "log[axis=x]")
    for y in range(eaves + 1, top):
        for x in range(x0, x1 + 1):
            if b.get(x, y, z1) == WALL:
                b.set(x, y, porch, WALL)
    for x in (3, 5, 7, 9):
        b.set(x, F + 1, porch, FENCE)
    for z in (z1, porch, z0):
        b.king_post(6, z, eaves, top, WALL)
    horns(b, 6, porch, top - 1, "z")
    for x in (5, 7):
        b.hang(x, eaves - 1, z1 + 1)
    # the door between posts, windows high in the long walls and the gable
    b.door(6, F + 1, z1, "south")
    b.fill((5, F + 1, z1), (5, F + 3, z1), "log")
    b.fill((7, F + 1, z1), (7, F + 3, z1), "log")
    for x in (4, 8):
        window(b, x, F + 4, z1, "south")
    window(b, 6, eaves + 3, z1, "south")
    for z in (2, 4, 6, 8):
        window(b, x0, F + 4, z, "west")
        window(b, x1, F + 4, z, "east")
    # the long hearth of embers down the middle, kerbed in cobblestone
    b.fill((6, F + 1, 4), (6, F + 1, 6), "magma")
    for z in range(4, 7):
        b.set(5, F + 1, z, slab("cobblestone"))
        b.set(7, F + 1, z, slab("cobblestone"))
    # tables and benches along either side
    for z in range(3, 8):
        b.table(4, z)
        b.set(3, F + 1, z, stairs("dark_planks", "west"))
        b.table(8, z)
        b.set(9, F + 1, z, stairs("dark_planks", "east"))
    # the fireplace and chimney in the back gable, the high seat and barrels by it
    b.fill((5, F + 1, z0), (7, F + 3, z0), STONE)
    b.set(6, F + 1, z0 + 1, "furnace[facing=south]")
    b.set(6, F + 2, z0 + 1, stairs("stone_bricks", "south", top=True))
    b.set(5, F + 1, z0 + 1, "stone_bricks")
    b.set(7, F + 1, z0 + 1, "stone_bricks")
    b.stack(6, z0 - 1, 0, top + 1, STONE, breast=[(5, z0 - 1), (7, z0 - 1)], shoulder=F + 6,
            pot=slab("stone_bricks"))
    b.fill((6, F + 3, z0), (6, top, z0), STONE)
    b.set(4, F + 1, 2, stairs("dark_planks", "north"))
    b.mark("work", 5, F + 1, 3)
    b.mark("work", 7, F + 1, 8)
    b.set(3, F + 1, 2, "barrel[facing=up]")
    b.set(9, F + 1, 2, "barrel[facing=up]")
    b.mark("loot", 8, F + 1, 2)
    b.set(3, F + 1, 8, "barrel[facing=up]|air")
    b.set(9, F + 1, 8, "barrel[facing=up]")
    for z in (3, 7):
        b.fill((x0 + 1, eaves, z), (x1 - 1, eaves, z), "log[axis=x]")
        b.hang(4, eaves - 1, z)
        b.hang(8, eaves - 1, z)
    for z in (2, 8):
        b.bracket_lamp(0 + 1, F + 2, z, "west")
        b.bracket_lamp(x1 + 1, F + 2, z, "east")
    b.ground("path", (6, 12), (6, 12))
    b.mark("entrance", 6, F, 12)
    b.write_piece(
        "lodge_hall", "hall", STYLES, "stone_bricks",
        "The lodge village's mead hall, a hearth of embers down its middle, a porch at its gable.",
        SCRIPT,
    )


def smithy():
    """The smithy: a stone forge with a steep roof and a great stone chimney, open on its east
    side to a lean-to on log posts where the anvil and the quenching trough stand, coal and ore
    heaped by the wall and a woodpile behind."""
    b = Piece(10, 15, 10)
    # the yard is the world's own ground, snow or grass as the taiga has it
    b.ground(None)
    b.ground("path*3|gravel|cobblestone*2", (0, 1), (9, 7))
    b.ground("path*4|gravel", (3, 8), (6, 9))
    # the forge: stone walls three high on three sides, open to the lean-to on the east
    x0, x1, z0, z1 = 0, 5, 1, 6
    for y in range(F, F + 3):
        b.walls((x0, y, z0), (x1, y, z1), STONE)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F + 2, z1 - 1), AIR)
    b.fill((x1, F, z0 + 1), (x1, F + 2, z1 - 1), AIR)
    b.fill((x1, F, 4), (x1, F + 2, 4), "log")
    b.fill((2, F, z1), (3, F + 1, z1), AIR)
    b.beam_ring(x0, x1, z0, z1, F + 3)
    top = b.roof(x0, x1, z0, z1, F + 3, roof_stair, ROOF_FULL, ROOF_CAP, steep=True, gable=STONE,
                 ridge=RIDGE)
    # the hearth along the west wall, two furnaces under a hood, the chimney over them
    b.fill((1, F, 2), (1, F, 5), STONE)
    b.set(1, F, 3, "furnace[facing=east]")
    b.set(1, F, 4, "furnace[facing=east]")
    b.fill((1, F + 1, 2), (1, F + 2, 5), STONE)
    b.set(2, F + 2, 3, stairs("stone_bricks", "west", top=True))
    b.set(2, F + 2, 4, stairs("stone_bricks", "west", top=True))
    b.fill((x0, F, 3), (x0, top + 2, 4), STONE)
    b.set(x0, top + 3, 3, slab("stone_bricks"))
    b.set(x0, top + 3, 4, slab("stone_bricks"))
    b.mark("work", 2, F, 4)
    b.mark("loot", 4, F, 2)
    b.set(3, F, 2, "crafting_table")
    b.set(4, F, 5, "barrel[facing=up]")
    b.hang(3, F + 2, 3)
    # the lean-to: log posts, a roof of stairs off the forge's wall plate
    for z in (z0, 4, z1):
        b.fill((9, F, z), (9, F + 1, z), "log")
    for x in range(6, 10):
        layer = F + 3 if x <= 7 else F + 2
        for z in range(z0, z1 + 1):
            b.set(x, layer, z, roof_stair("west") if x in (7, 9) else ROOF_FULL)
    b.fill((6, F + 2, z0), (8, F + 2, z1), AIR)
    b.fill((8, F + 2, z0), (8, F + 2, z1), "log[axis=z]")
    b.fill((9, F + 2, z0), (9, F + 2, z1), roof_stair("west"))
    b.fill((6, F + 3, z0), (6, F + 3, z1), ROOF_FULL)
    b.fill((7, F + 3, z0), (7, F + 3, z1), roof_stair("west"))
    b.fill((8, F + 3, z0), (9, F + 3, z1), AIR)
    # the anvil, the trough, coal and ore
    b.set(7, F, 4, "cobblestone_wall")
    b.set(7, F + 1, 4, slab("stone_bricks"))
    b.mark("work", 7, F, 5)
    b.fill((6, F, 1), (8, F, 2), "stone_bricks")
    b.set(7, F, 1, "water")
    b.set(8, F, 1, "water")
    b.set(8, F, 6, "coal_ore*2|iron_ore|gravel")
    b.set(8, F + 1, 6, "coal_ore|air")
    b.set(6, F, 6, "barrel[facing=up]")
    b.hang(8, F + 1, 3)
    b.set(8, F + 2, 3, "log[axis=z]")
    # the woodpile behind, a lantern at the yard's corner
    woodpile(b, 0, 8, 0, 0, 2, "x")
    b.set(9, F, 8, FENCE)
    b.set(9, F + 1, 8, "lantern")
    b.set(0, F, 8, FENCE)
    b.set(0, F + 1, 8, "lantern")
    b.mark("entrance", 4, F, 9)
    b.write_piece(
        "lodge_smithy", "smithy", STYLES, "stone_bricks",
        "The lodge village's smithy, a stone forge under a great chimney and a lean-to.", SCRIPT,
    )


def tower():
    """The lookout: a stone base five high, a timber shaft in a log frame over it, a gallery
    railed with fences jutting out round the lookout room at the top, where a gold bell hangs,
    and a steep spire with snow on its point. A ladder climbs inside; the lookout keeps its
    foot."""
    b = Piece(7, 26, 7)
    b.ground("path*3|gravel")
    x0, x1, z0, z1 = 1, 5, 1, 5
    stone_top, shaft_top = F + 4, F + 13
    b.fill((x0, 0, z0), (x1, 0, z1), STONE)
    for y in range(F, shaft_top + 1):
        b.walls((x0, y, z0), (x1, y, z1), STONE if y <= stone_top else WALL)
    b.corners((x0, stone_top + 1, z0), (x1, shaft_top, z1), "log")
    b.beam_ring(x0, x1, z0, z1, shaft_top)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, shaft_top, z1 - 1), AIR)
    b.fill((x0 + 1, 0, z0 + 1), (x1 - 1, 0, z1 - 1), "planks")
    # a batter of stairs round the foot but at the door
    for x in range(0, 7):
        for z, f in ((0, "south"), (6, "north")):
            if not (z == 6 and x == 3):
                b.set(x, F, z, stairs("stone_bricks", f))
    for z in range(1, 6):
        b.set(0, F, z, stairs("stone_bricks", "east"))
        b.set(6, F, z, stairs("stone_bricks", "west"))
    b.door(3, F, z1, "south")
    b.bracket_lamp(4, F + 2, z1 + 1, "south", material="stone_bricks")
    for y in (F + 3, F + 7, F + 11):
        window(b, 3, y, z0, "north")
        window(b, x0, y, 3, "west")
        window(b, x1, y, 3, "east")
    window(b, 3, F + 8, z1, "south")
    # inside: the ladder, a barrel, lanterns on beams up the shaft
    b.ladder(3, z0 + 1, F, shaft_top + 1, "south")
    b.set(2, F, 4, "barrel[facing=up]")
    b.mark("work", 4, F, 3)
    for y in (F + 4, F + 10):
        b.fill((x0 + 1, y, 4), (x1 - 1, y, 4), "log[axis=x]")
        b.hang(4, y - 1, 4)
    # the gallery: a floor jutting out all round on upturned stairs, railed, and the lookout
    # room's posts
    look = shaft_top + 1
    b.fill((0, look, 0), (6, look, 6), "dark_planks")
    b.set(3, look, z0 + 1, "ladder[facing=south]")
    for x in range(0, 7):
        for z in range(0, 7):
            if x in (0, 6) or z in (0, 6):
                b.set(x, look + 1, z, FENCE)
    for x, f in ((0, "east"), (6, "west")):
        for z in range(0, 7):
            b.set(x, look - 1, z, stairs("dark_planks", f, top=True))
    for z, f in ((0, "south"), (6, "north")):
        for x in range(1, 6):
            b.set(x, look - 1, z, stairs("dark_planks", f, top=True))
    b.corners((x0, look + 1, z0), (x1, look + 3, z1), "log")
    b.beam_ring(x0, x1, z0, z1, look + 4)
    b.fill((x0 + 1, look + 4, 3), (x1 - 1, look + 4, 3), "log[axis=x]")
    b.set(3, look + 3, 3, FENCE)
    b.set(3, look + 2, 3, "gold_block")
    b.hang(2, look + 3, 4)
    b.set(2, look + 4, 4, "log[axis=x]")
    top = b.spire(x0, x1, z0, z1, look + 5, roof_stair, ROOF_FULL, "snow/dark_planks_slab")
    b.set(3, top + 1, 3, FENCE)
    b.mark("entrance", 3, F, 6)
    b.write_piece(
        "lodge_tower", "tower", STYLES, "stone_bricks",
        "The lodge village's lookout, a railed gallery and a bell under a steep spire.", SCRIPT,
    )


def well():
    """The village's well, its centre: a stone-lined shaft of water three deep behind a rim of
    stone, a frame of logs over it with the windlass and a lantern under a steep little roof
    with snow on its ridge, on a square of fieldstone with lanterns on log posts at its
    corners, where the villagers meet."""
    b = Piece(7, 11, 7)
    b.floor = 3
    W = b.floor
    b.fill((0, W - 1, 0), (6, W - 1, 6), "path*3|gravel|cobblestone")
    b.fill((1, W - 1, 1), (5, W - 1, 5), "cobblestone*4|mossy_cobblestone|stone_bricks*2")
    b.fill((2, 0, 2), (4, W, 4), STONE)
    b.fill((3, 0, 3), (3, W - 1, 3), "water")
    b.set(3, W, 3, AIR)
    for x, z in ((2, 2), (4, 2), (2, 4), (4, 4)):
        b.fill((x, W + 1, z), (x, W + 2, z), "log")
    b.fill((2, W + 3, 3), (4, W + 3, 3), "log[axis=x]")
    b.hang(3, W + 2, 3)
    b.roof(2, 4, 2, 4, W + 3, roof_stair, ROOF_FULL, ROOF_CAP, ends=1, steep=True, ridge=RIDGE)
    for x, z in ((0, 0), (6, 0), (0, 6), (6, 6)):
        b.set(x, W, z, "cobblestone_wall")
        b.set(x, W + 1, z, "log")
        b.set(x, W + 2, z, "lantern")
    for x, z in ((1, 1), (3, 1), (5, 1), (1, 3), (5, 3), (1, 5), (5, 5), (3, 5)):
        b.mark("gather", x, W, z)
    b.mark("entrance", 3, W, 6)
    b.write_piece(
        "lodge_well", "well", STYLES, "stone_bricks",
        "The lodge village's well in a frame of logs under a steep little roof.", SCRIPT,
    )


def lamp():
    """A lamp post by the street: a log on a stone foot, an arm with a lantern hung from it
    under a little roof."""
    b = Piece(2, 7, 1)
    b.ground("path")
    b.set(0, F, 0, "cobblestone_wall")
    b.fill((0, F + 1, 0), (0, F + 3, 0), "log")
    b.set(1, F + 3, 0, FENCE)
    b.hang(1, F + 2, 0)
    b.set(0, F + 4, 0, stairs("dark_planks", "east"))
    b.set(1, F + 4, 0, stairs("dark_planks", "west"))
    b.write_piece(
        "lodge_lamp", "lamp", STYLES, "stone_bricks",
        "A lamp post of the lodge village, a lantern hung from a log post's arm.", SCRIPT,
    )


cabin()
house()
homestead()
hall()
smithy()
tower()
well()
lamp()
