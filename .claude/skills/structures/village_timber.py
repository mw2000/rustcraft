"""Writes the timber village's pieces into assets/structures/village/: the village of temperate
grassland, timber-framed cottages on stone plinths under steep thatch.

The style: a plinth of cobblestone a block high, walls of white lime plaster (or light planks,
for some houses) framed by log posts and beams, upper floors jettied out over the street on
the beams' ends, steep roofs of thatch or dark planks (a choice for each house) climbing two
layers to each row, brick or cobblestone chimneys, glass-pane windows with open shutters and
boxes of flowers, and gardens of grass and flowers round them. A stone cottage of cobblestone
and stone brick stands among them.

    python3 .claude/skills/structures/village_timber.py
"""

from village import AIR, F, FLOWERS, VERGE, Piece, slab, stairs, thatch_or_planks

SCRIPT = "village_timber.py"
STYLES = ["timber"]

# Choices for the whole house, with different totals so that they vary apart: the roof (of 2),
# the walls' infill (of 3) and the chimney (of 5).
ROOF = thatch_or_planks
ROOF_FULL = "thatch/dark_planks"
ROOF_CAP = "thatch_slab/dark_planks_slab"
INFILL = "plaster*2/light_planks"
CHIMNEY = "bricks*3/cobblestone*2"
PLINTH = "cobblestone*6|mossy_cobblestone*2|stone_bricks"
SHUTTER = "trapdoor[facing={facing},open=true]*3|air"
BOX = "trapdoor[facing={facing},half=top]"


def cottage():
    """A one-room cottage for one: a plinth, a framed front of two windows and a door between
    posts, a steep roof along its length over a cobblestone gable and chimney at its west end,
    a hearth, a bed, a table and a chest, and a garden of flowers."""
    b = Piece(7, 14, 8)
    b.ground("grass")
    x0, x1, z0, z1 = 0, 6, 1, 5
    # the plinth and the floor
    b.fill((x0, 0, z0), (x1, F, z1), PLINTH)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F, z1 - 1), "planks")
    # walls three high and the plate over them
    b.frame(x0, x1, z0, z1, F + 1, F + 4, INFILL, zs=(3,))
    b.fill((x0 + 1, F + 1, z0 + 1), (x1 - 1, F + 4, z1 - 1), AIR)
    # the west end is stone round the hearth and its chimney
    b.fill((x0, F + 1, z0 + 1), (x0, F + 3, z1 - 1), "cobblestone*4|mossy_cobblestone")
    top = b.roof(x0, x1, z0, z1, F + 4, ROOF, ROOF_FULL, ROOF_CAP, steep=True, gable=INFILL)
    b.king_post(x1, 3, F + 4, top, INFILL)
    b.fill((x0, F + 4, 2), (x0, top - 2, 4), "cobblestone*4|mossy_cobblestone")
    b.chimney(x0, 3, F + 4, top + 1, CHIMNEY, pot="cobblestone_wall")
    # a window in the east gable over the bed
    b.window(x1, F + 6, 3, "east")
    # the front: a door between posts, a window each side with shutters and a flower box
    b.door(3, F + 1, z1, "south")
    b.set(3, 0, 6, "path")
    b.set(3, 0, 7, "path")
    b.step_up(3, 6, "cobblestone")
    b.window(2, F + 2, z1, "south", box=BOX)
    b.window(4, F + 2, z1, "south", box=BOX)
    b.window(3, F + 2, z0, "north", shutters=SHUTTER)
    b.window(x1, F + 2, 2, "east")
    b.window(x1, F + 2, 4, "east")
    # inside: the hearth, the bed, a table, a chest and a lantern from a tie beam
    b.set(1, F + 1, 3, "furnace[facing=east]")
    b.set(1, F + 1, 2, "crafting_table")
    b.set(1, F + 1, 4, "barrel[facing=up]*2|air")
    b.set(5, F + 1, 2, "bed")
    b.mark("bed", 4, F + 1, 2)
    b.table(4, 4)
    b.set(5, F + 1, 4, stairs("dark_planks", "east"))
    b.mark("loot", 2, F + 1, 2)
    b.fill((1, F + 4, 3), (5, F + 4, 3), "log[axis=x]")
    b.hang(3, F + 3, 3)
    # the porch light, hung under the eaves by the door
    b.hang(4, F + 3, 6)
    # the garden: flowers along the front, grass elsewhere
    for x in (0, 1, 5, 6):
        b.set(x, F, 6, FLOWERS)
    b.verge()
    b.mark("entrance", 3, F, 7)
    b.write_piece(
        "timber_cottage", "house", STYLES, "cobblestone",
        "A timber cottage for one under a steep roof, its garden in flower.", SCRIPT,
    )


def house():
    """A house for two, its gable to the street: a plastered ground floor on a plinth, the upper
    floor jettied out a block over the front on the ends of its joists and corbels, a steep roof
    running back from a front gable with a king post, a brick chimney up its east side, and a
    stair inside climbing to the bedroom."""
    b = Piece(9, 20, 10)
    b.ground("grass")
    x0, x1, z0, z1 = 1, 7, 1, 6
    up = z1 + 1
    b.fill((x0, 0, z0), (x1, F, z1), PLINTH)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F, z1 - 1), "planks")
    # the ground floor's walls, the door between windows
    b.frame(x0, x1, z0, z1, F + 1, F + 3, INFILL, zs=(4,), plate=False)
    b.fill((x0 + 1, F + 1, z0 + 1), (x1 - 1, F + 3, z1 - 1), AIR)
    b.door(4, F + 1, z1, "south")
    b.step_up(4, z1 + 1, "cobblestone")
    b.window(2, F + 2, z1, "south", shutters=SHUTTER)
    b.window(6, F + 2, z1, "south", shutters=SHUTTER)
    b.window(x0, F + 2, 3, "west")
    b.window(x0, F + 2, 5, "west")
    # the upper floor's beams, their ends standing out over the front on corbels
    floor = F + 4
    b.beam_ring(x0, x1, z0, up, floor)
    for x in range(x0 + 1, x1):
        b.set(x, floor, up, "log[axis=z]")
    b.fill((x0 + 1, floor, z0 + 1), (x1 - 1, floor, up - 1), "dark_planks")
    for x in (x0, 3, 5, x1):
        b.set(x, floor - 1, up, stairs("dark_planks", "north", top=True))
    # the upper floor, jettied: close studs either side of a pair of windows
    b.frame(x0, x1, z0, up, floor + 1, floor + 3, INFILL, xs=(4,), zs=(4,))
    b.fill((x0 + 1, floor + 1, z0 + 1), (x1 - 1, floor + 3, up - 1), AIR)
    b.window(3, floor + 2, up, "south", shutters=SHUTTER, box=BOX)
    b.window(5, floor + 2, up, "south", shutters=SHUTTER, box=BOX)
    b.window(x0, floor + 2, 3, "west", shutters=SHUTTER)
    b.window(x1, floor + 2, 6, "east", shutters=SHUTTER)
    b.window(4, floor + 2, z0, "north")
    # the roof, its front gable hanging a block over the jetty
    eaves = floor + 4
    top = b.roof(x0, x1, z0, up, eaves, ROOF, ROOF_FULL, ROOF_CAP, along="z", ends=1,
                 steep=True, gable=INFILL)
    b.king_post(4, up, eaves, top, INFILL)
    b.king_post(4, z0, eaves, top, INFILL)
    b.window(3, eaves + 2, up, "south")
    b.window(5, eaves + 2, up, "south")
    b.fill((x0, eaves, 4), (x1, eaves, 4), "log[axis=x]")
    # the chimney up the east side, the hearth inside it
    b.fill((x1, F + 1, 3), (x1, eaves - 1, 3), CHIMNEY)
    b.stack(x1 + 1, 3, 0, top, CHIMNEY, breast=[(x1 + 1, 2), (x1 + 1, 4)], shoulder=F + 3,
            pot="cobblestone_wall")
    b.set(x1 + 1, F + 4, 2, slab("bricks") + "/" + slab("cobblestone"))
    b.set(x1 + 1, F + 4, 4, slab("bricks") + "/" + slab("cobblestone"))
    b.set(x1 - 1, F + 1, 3, "furnace[facing=west]")
    # the stair up the back wall, climbing east to the landing, the floor open over it
    for i, x in enumerate(range(3, 6)):
        b.set(x, F + 1 + i, z0 + 1, stairs("dark_planks", "east"))
        b.fill((x, F + 1, z0 + 1), (x, F + i, z0 + 1), "dark_planks")
    b.fill((3, floor, z0 + 1), (5, floor, z0 + 1), AIR)
    b.fill((4, floor + 1, z0 + 2), (5, floor + 1, z0 + 2), "dark_planks_fence")
    b.set(x1 - 1, F + 1, z0 + 1, "barrel[facing=up]")
    # downstairs: a table and benches, a crafting table, the chest and a lantern
    b.table(3, 4)
    b.set(2, F + 1, 4, stairs("dark_planks", "west"))
    b.set(4, F + 1, 4, stairs("dark_planks", "east"))
    b.set(x1 - 1, F + 1, 5, "crafting_table")
    b.mark("loot", 2, F + 1, 2)
    b.hang(4, floor - 1, 3)
    # upstairs: two beds, a bookshelf and a lantern from the tie beam
    b.set(2, floor + 1, 3, "bed")
    b.mark("bed", 3, floor + 1, 3)
    b.set(2, floor + 1, 6, "bed")
    b.mark("bed", 3, floor + 1, 6)
    b.set(6, floor + 1, 4, "bookshelf")
    b.set(6, floor + 1, 5, "barrel[facing=up]|air")
    b.hang(4, eaves - 1, 4)
    b.bracket_lamp(4, F + 2, z0 - 1, "north")
    # the garden: a path to the door, flowers either side, a lantern at the gate
    b.set(4, 0, up, "path")
    b.set(4, 0, 8, "path")
    b.set(4, 0, 9, "path")
    for x in (1, 2, 6, 7):
        b.set(x, F, 8, FLOWERS)
    b.set(5, F, 9, "dark_planks_fence")
    b.set(5, F + 1, 9, "lantern")
    b.verge()
    b.mark("entrance", 4, F, 9)
    b.write_piece(
        "timber_house", "house", STYLES, "cobblestone",
        "A timber house for two, jettied over its garden, its gable to the street.", SCRIPT,
    )


def longhouse():
    """A house for three, L-shaped: a hall range along the back with a stone gable and chimney
    at its west end, and a cross-wing coming forward to the street at its east end, its gable
    jettied over a pair of windows, the two roofs meeting in a valley. A small yard of flowers
    lies in the angle between them, the door opening onto it."""
    b = Piece(11, 16, 12)
    b.ground("grass")
    # the hall range, and the wing sharing its east wall
    hx0, hx1, hz0, hz1 = 0, 5, 3, 8
    wx0, wx1, wz0, wz1 = 5, 9, 1, 10
    for (x0, x1, z0, z1) in ((hx0, hx1, hz0, hz1), (wx0, wx1, wz0, wz1)):
        b.fill((x0, 0, z0), (x1, F, z1), PLINTH)
    b.fill((hx0 + 1, F, hz0 + 1), (hx1 - 1, F, hz1 - 1), "planks")
    b.fill((wx0 + 1, F, wz0 + 1), (wx1 - 1, F, wz1 - 1), "planks")
    b.frame(hx0, hx1, hz0, hz1, F + 1, F + 4, INFILL, xs=(3,))
    b.frame(wx0, wx1, wz0, wz1, F + 1, F + 4, INFILL, xs=(7,), zs=(4, 7))
    b.fill((hx0 + 1, F + 1, hz0 + 1), (hx1 - 1, F + 4, hz1 - 1), AIR)
    b.fill((wx0 + 1, F + 1, wz0 + 1), (wx1 - 1, F + 4, wz1 - 1), AIR)
    # the stone west end of the hall, round its hearth
    b.fill((hx0, F + 1, hz0 + 1), (hx0, F + 4, hz1 - 1), "cobblestone*4|mossy_cobblestone")
    # the way through between them, and the front door onto the yard
    b.fill((hx1, F + 1, 6), (hx1, F + 2, 6), AIR)
    b.door(2, F + 1, hz1, "south")
    b.step_up(2, hz1 + 1, "cobblestone")
    b.window(4, F + 2, hz1, "south", shutters=SHUTTER, box=BOX)
    b.window(1, F + 2, hz1, "south", box=BOX)
    b.window(2, F + 2, hz0, "north", shutters=SHUTTER)
    b.window(4, F + 2, hz0, "north")
    # the wing's front, a pair of windows under its jettied gable
    b.window(6, F + 2, wz1, "south", shutters=SHUTTER, box=BOX)
    b.window(8, F + 2, wz1, "south", shutters=SHUTTER, box=BOX)
    b.window(wx1, F + 2, 3, "east", shutters=SHUTTER)
    b.window(wx1, F + 2, 6, "east", shutters=SHUTTER)
    b.window(wx1, F + 2, 8, "east")
    b.window(7, F + 2, wz0, "north")
    eaves = F + 4
    top = b.roofs(
        dict(x0=hx0, x1=hx1, z0=hz0, z1=hz1, y=eaves, stair=ROOF, full=ROOF_FULL, cap=ROOF_CAP,
             steep=True, gable="cobblestone*4|mossy_cobblestone"),
        dict(x0=wx0, x1=wx1, z0=wz0, z1=wz1, y=eaves, stair=ROOF, full=ROOF_FULL, cap=ROOF_CAP,
             along="z", ends=1, steep=True, gable=INFILL),
    )
    b.king_post(7, wz1, eaves, top, INFILL)
    b.king_post(7, wz0, eaves, top, INFILL)
    b.window(6, eaves + 2, wz1, "south")
    b.window(8, eaves + 2, wz1, "south")
    b.stack(hx0, 5, F + 4, top + 1, CHIMNEY, pot="cobblestone_wall")
    # the hall: the hearth, a table and benches, a bed by the window, the chest
    b.set(1, F + 1, 5, "furnace[facing=east]")
    b.set(1, F + 1, 6, "furnace[facing=east]|barrel[facing=up]")
    b.set(1, F + 1, 4, "crafting_table")
    b.table(3, 5)
    b.set(3, F + 1, 4, stairs("dark_planks", "north"))
    b.set(3, F + 1, 6, stairs("dark_planks", "south"))
    b.set(4, F + 1, 4, "bed")
    b.mark("bed", 4, F + 1, 5)
    b.mark("loot", 1, F + 1, 7)
    b.fill((hx0 + 1, eaves, 5), (hx1 - 1, eaves, 5), "log[axis=x]")
    b.hang(2, eaves - 1, 5)
    # the wing: two beds at the back, a chest, a bookshelf and barrels by the front windows
    b.set(6, F + 1, 2, "bed")
    b.mark("bed", 6, F + 1, 3)
    b.set(8, F + 1, 2, "bed")
    b.mark("bed", 8, F + 1, 3)
    b.set(7, F + 1, 2, "barrel[facing=up]")
    b.set(7, F + 2, 2, "lantern")
    b.set(8, F + 1, 5, "bookshelf")
    b.set(8, F + 1, 6, "bookshelf|barrel[facing=up]")
    b.set(6, F + 1, 9, "barrel[facing=up]|air")
    b.table(8, 9)
    b.fill((wx0 + 1, eaves, 6), (wx1 - 1, eaves, 6), "log[axis=x]")
    b.hang(7, eaves - 1, 6)
    # the yard: a path to the door, a lantern on a post, flowers
    b.set(2, 0, 9, "path")
    b.set(2, 0, 10, "path")
    b.set(2, 0, 11, "path")
    b.set(3, F, 10, "dark_planks_fence")
    b.set(3, F + 1, 10, "lantern")
    for x in (0, 1, 4):
        b.set(x, F, 9, FLOWERS)
    b.verge()
    b.mark("entrance", 2, F, 11)
    b.write_piece(
        "timber_longhouse", "house", STYLES, "cobblestone",
        "A timber house for three, a hall range and a cross-wing round a yard.", SCRIPT,
    )


def stone_cottage():
    """A stone cottage for two, of cobblestone with stone brick quoins and lintels, under a
    roof hipped at its east end and gabled over a stone chimney at its west, with a lean-to
    porch on posts over the door and a bench under the window."""
    b = Piece(8, 13, 9)
    b.ground("grass")
    x0, x1, z0, z1 = 0, 6, 1, 5
    wall = "cobblestone*5|mossy_cobblestone*2|stone*1"
    quoin = "stone_bricks*4|mossy_stone_bricks|cracked_stone_bricks"
    b.fill((x0, 0, z0), (x1, F, z1), quoin)
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F, z1 - 1), "dark_planks")
    for y in range(F + 1, F + 4):
        b.walls((x0, y, z0), (x1, y, z1), wall)
    b.corners((x0, F + 1, z0), (x1, F + 3, z1), quoin)
    b.fill((x0 + 1, F + 1, z0 + 1), (x1 - 1, F + 3, z1 - 1), AIR)
    # a string course of slabs at the eaves
    b.beam_ring(x0, x1, z0, z1, F + 4)
    # windows under stone lintels, a door with a porch over it
    b.door(4, F + 1, z1, "south")
    b.step_up(4, z1 + 1, "stone_bricks")
    for x, z, out in ((2, z1, "south"), (2, z0, "north"), (5, z0, "north"), (x1, 3, "east")):
        b.window(x, F + 2, z, out, shutters=SHUTTER if out != "east" else None)
        b.set(x, F + 3, z, "stone_bricks")
    # the roof: a gable over the west chimney, hipped at the east
    eaves = F + 4
    top = b.roof(x0, x1, z0, z1, eaves, ROOF, ROOF_FULL, ROOF_CAP, steep=True,
                 gable="cobblestone*4|mossy_cobblestone")
    # hip the east end: a slope of stairs climbing west in from beyond the east wall
    for k in range(0, top - eaves + 1):
        inset = (k + 1) // 2
        x = x1 + 1 - inset
        lip = k % 2 == 0
        for z in range(z0 - 1 + inset, z1 + 2 - inset):
            if lip:
                b.set(x, eaves + k, z, ROOF("west"))
            else:
                b.set(x, eaves + k, z, ROOF_FULL)
        for xx in range(x + 1, x1 + 2):
            for z in range(z0 - 1, z1 + 2):
                if b.get(xx, eaves + k, z) not in (None, AIR) and k > 0:
                    b.set(xx, eaves + k, z, None)
    b.fill((x0, F + 4, 2), (x0, top - 1, 4), "cobblestone*4|mossy_cobblestone")
    b.stack(x0, 3, F + 1, top + 1, "cobblestone*4|mossy_cobblestone", pot="cobblestone_wall")
    # the porch: a lean-to of slabs on two posts over the step
    for x in (3, 5):
        b.fill((x, F, 7), (x, F + 2, 7), "dark_planks_fence")
    b.fill((3, F + 3, 6), (5, F + 3, 7), slab("dark_planks"))
    b.hang(5, F + 2, 6)
    b.set(1, F, 6, stairs("dark_planks", "north"))
    b.set(2, F, 6, stairs("dark_planks", "north"))
    # inside: the hearth, two beds, a table, a barrel and the chest, a lantern on a beam
    b.set(1, F + 1, 3, "furnace[facing=east]")
    b.set(1, F + 1, 2, "barrel[facing=up]")
    b.set(1, F + 1, 4, "crafting_table")
    b.set(5, F + 1, 2, "bed")
    b.mark("bed", 4, F + 1, 2)
    b.set(5, F + 1, 3, "bed")
    b.mark("bed", 4, F + 1, 3)
    b.mark("loot", 5, F + 1, 4)
    b.table(3, 3)
    b.fill((x0 + 1, F + 4, 3), (x1 - 1, F + 4, 3), "log[axis=x]")
    b.hang(3, F + 3, 2)
    b.set(4, 0, 7, "path")
    b.set(4, 0, 8, "path")
    b.verge()
    b.mark("entrance", 4, F, 8)
    b.write_piece(
        "timber_stone_cottage", "house", STYLES, "cobblestone",
        "A stone cottage for two, hipped at one end, with a porch on posts.", SCRIPT,
    )


def inn():
    """The inn, the village's hall: a tall gable-fronted block, its stone ground floor a taproom
    with a counter, barrels and tables, its timber upper floor jettied over the door and reached
    by a stair, and a lower kitchen range along its west side under its own roof and chimney,
    with tables outside on the terrace in front of it."""
    b = Piece(13, 21, 13)
    b.ground("grass")
    mx0, mx1, mz0, mz1 = 4, 11, 1, 9
    up = mz1 + 1
    kx0, kx1, kz0, kz1 = 0, 4, 3, 9
    stone = "cobblestone*5|mossy_cobblestone|stone_bricks*2"
    for (x0, x1, z0, z1) in ((mx0, mx1, mz0, mz1), (kx0, kx1, kz0, kz1)):
        b.fill((x0, 0, z0), (x1, F, z1), PLINTH)
        b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F, z1 - 1), "planks")
    # the taproom: stone walls four high, quoined, big windows either side of the door
    for y in range(F + 1, F + 5):
        b.walls((mx0, y, mz0), (mx1, y, mz1), stone)
    b.corners((mx0, F + 1, mz0), (mx1, F + 4, mz1), "stone_bricks")
    b.fill((mx0 + 1, F + 1, mz0 + 1), (mx1 - 1, F + 4, mz1 - 1), AIR)
    b.door(7, F + 1, mz1, "south")
    b.step_up(7, up, "stone_bricks")
    for x in (5, 6, 9, 10):
        b.window(x, F + 2, mz1, "south", high=2)
    b.window(mx1, F + 2, 4, "east", high=2)
    b.window(mx1, F + 2, 7, "east", high=2)
    b.window(8, F + 2, mz0, "north", high=2)
    # the upper floor on its beams, jettied over the door on corbels
    floor = F + 5
    b.beam_ring(mx0, mx1, mz0, up, floor)
    for x in range(mx0 + 1, mx1):
        b.set(x, floor, up, "log[axis=z]")
    b.fill((mx0 + 1, floor, mz0 + 1), (mx1 - 1, floor, up - 1), "dark_planks")
    for x in (mx0, 6, 9, mx1):
        b.set(x, floor - 1, up, stairs("dark_planks", "north", top=True))
    b.frame(mx0, mx1, mz0, up, floor + 1, floor + 3, INFILL, xs=(7, 8), zs=(5,))
    b.fill((mx0 + 1, floor + 1, mz0 + 1), (mx1 - 1, floor + 3, up - 1), AIR)
    for x in (5, 10):
        b.window(x, floor + 2, up, "south", shutters=SHUTTER, box=BOX)
    b.window(mx1, floor + 2, 3, "east", shutters=SHUTTER)
    b.window(mx1, floor + 2, 7, "east", shutters=SHUTTER)
    b.window(mx0, floor + 2, 3, "west", shutters=SHUTTER)
    # the kitchen range: framed walls under a lower roof, a chimney at its west end
    b.frame(kx0, kx1, kz0, kz1, F + 1, F + 4, INFILL, xs=(2,))
    b.fill((kx0 + 1, F + 1, kz0 + 1), (kx1 - 1, F + 4, kz1 - 1), AIR)
    b.fill((kx0, F + 1, kz0 + 1), (kx0, F + 4, kz1 - 1), stone)
    b.fill((mx0, F + 1, 8), (mx0, F + 2, 8), AIR)
    b.window(1, F + 2, kz1, "south", shutters=SHUTTER, box=BOX)
    b.window(3, F + 2, kz1, "south", shutters=SHUTTER, box=BOX)
    # the roofs: the main gable hanging over the jetty, the kitchen's meeting its side
    eaves = floor + 4
    top = b.roofs(
        dict(x0=mx0, x1=mx1, z0=mz0, z1=up, y=eaves, stair=ROOF, full=ROOF_FULL, cap=ROOF_CAP,
             along="z", ends=1, steep=True, gable=INFILL),
        dict(x0=kx0, x1=kx1, z0=kz0, z1=kz1, y=F + 4, stair=ROOF, full=ROOF_FULL, cap=ROOF_CAP,
             steep=True, gable=stone),
    )
    for x in (7, 8):
        b.king_post(x, up, eaves, top, INFILL)
    b.window(6, eaves + 2, up, "south")
    b.window(9, eaves + 2, up, "south")
    b.stack(kx0, 6, F + 1, F + 12, CHIMNEY, pot="cobblestone_wall")
    b.stack(mx1, 2, eaves, top + 1, CHIMNEY, pot="cobblestone_wall")
    # the taproom: a counter with the keeper behind it, barrels on the wall, tables and benches
    for z in range(3, 7):
        b.set(8, F + 1, z, "dark_planks")
        b.set(8, F + 2, z, slab("dark_planks"))
    b.mark("work", 9, F + 1, 4)
    for z in range(3, 9):
        b.set(mx1 - 1, F + 1, z, "barrel[facing=west]")
        b.set(mx1 - 1, F + 2, z, "barrel[facing=west]*2|air")
    b.mark("loot", mx1 - 1, F + 1, 2)
    # chairs are stairs with their backs to the table
    for z in (4, 7):
        b.table(6, z)
        b.set(5, F + 1, z, stairs("dark_planks", "west"))
    b.set(7, F + 1, 4, stairs("dark_planks", "east"))
    b.set(6, F + 1, 8, stairs("dark_planks", "south"))
    for x, z in ((6, 5), (8, 8), (9, 6)):
        b.hang(x, floor - 1, z)
    # the stair along the north wall, climbing east to the landing, the floor open over it
    for i, x in enumerate(range(6, 10)):
        b.set(x, F + 1 + i, mz0 + 1, stairs("dark_planks", "east"))
        b.fill((x, F + 1, mz0 + 1), (x, F + i, mz0 + 1), "dark_planks")
    b.fill((7, floor, mz0 + 1), (9, floor, mz0 + 1), AIR)
    b.fill((7, floor + 1, mz0 + 2), (9, floor + 1, mz0 + 2), "dark_planks_fence")
    # upstairs: rooms for travellers, beds, a table and lanterns from a tie beam
    for z in (4, 6, 8):
        b.set(5, floor + 1, z, "bed")
    b.set(10, floor + 1, 9, "bed")
    b.set(10, floor + 1, 5, "bookshelf")
    b.table(8, 6, y=floor + 1)
    b.fill((mx0 + 1, eaves, 5), (mx1 - 1, eaves, 5), "log[axis=x]")
    b.hang(7, eaves - 1, 5)
    b.hang(8, eaves - 1, 8)
    # the kitchen: furnaces in the hearth, a crafting table, barrels, the cook's place
    b.set(1, F + 1, 6, "furnace[facing=east]")
    b.set(1, F + 1, 5, "furnace[facing=east]")
    b.set(1, F + 1, 4, "crafting_table")
    b.set(3, F + 1, 4, "barrel[facing=up]")
    b.set(1, F + 1, 8, "barrel[facing=up]|air")
    b.mark("work", 2, F + 1, 5)
    b.fill((kx0 + 1, F + 4, 6), (kx1 - 1, F + 4, 6), "log[axis=x]")
    b.hang(2, F + 3, 6)
    b.bracket_lamp(2, F + 2, kz0 - 1, "north")
    b.bracket_lamp(9, F + 2, mz0 - 1, "north")
    # the terrace: a path to the door, lanterns under the jetty, a table and benches outside
    for z in range(up, 13):
        b.set(7, 0, z, "path")
    b.fill((0, 0, 10), (4, 0, 11), "path")
    b.hang(6, floor - 1, up)
    b.hang(9, floor - 1, up)
    b.table(2, 11, y=F)
    b.set(1, F, 11, stairs("dark_planks", "west"))
    b.set(3, F, 11, stairs("dark_planks", "east"))
    b.set(0, F, 10, "barrel[facing=up]|air")
    b.set(4, F, 11, "dark_planks_fence")
    b.set(4, F + 1, 11, "lantern")
    for x in (5, 6, 8, 9, 10, 11):
        b.set(x, F, 11, FLOWERS)
    b.verge()
    b.mark("entrance", 7, F, 12)
    b.write_piece(
        "timber_inn", "hall", STYLES, "cobblestone",
        "The timber village's inn: a taproom under a jettied gable, a kitchen range beside it.",
        SCRIPT,
    )


def smithy():
    """The smithy: a workshop open to the street between log posts, its floor the trodden yard,
    stone at the back and west round the forge, two furnaces under a brick hood and chimney,
    an anvil, a quenching trough, a heap of ore and the chest, under a plank roof, with a
    woodpile against its east end."""
    b = Piece(10, 14, 10)
    b.ground("grass")
    x0, x1, z0, z1 = 0, 8, 1, 7
    stone = "cobblestone*5|mossy_cobblestone|stone_bricks*2"
    # the floor, trodden earth and cobbles, and the yard in front
    b.ground("path*3|gravel|cobblestone*2", (x0, z0), (x1, z1))
    b.ground("path*4|gravel", (2, z1 + 1), (6, 9))
    # stone walls at the back and west, a framed east wall, the front open between posts
    for y in range(F, F + 4):
        b.fill((x0, y, z0), (x1, y, z0), stone)
        b.fill((x0, y, z0), (x0, y, z1), stone)
    b.frame(x1, x1, z0, z1, F, F + 3, INFILL, zs=(4,), plate=False)
    for x in (x0, 3, 6, x1):
        b.fill((x, F, z1), (x, F + 3, z1), "log")
    b.fill((x0, F + 3, z1), (x1, F + 3, z1), "log[axis=x]")
    b.fill((x1, F + 3, z0), (x1, F + 3, z1), "log[axis=z]")
    b.window(x1, F + 1, 3, "east", shutters=SHUTTER)
    b.window(x1, F + 1, 5, "east", shutters=SHUTTER)
    # a plank roof, a little shallower than the houses', gabled in stone and plaster
    top = b.roof(x0, x1, z0, z1, F + 4, lambda f: stairs("dark_planks", f),
                 "dark_planks", slab("dark_planks"), gable=INFILL)
    b.fill((x0, F + 4, z0 + 1), (x0, top - 1, z1 - 1), stone)
    b.king_post(x1, 4, F + 4, top, INFILL)
    # the forge: two furnaces in a cobblestone hearth, a brick hood and the chimney over them
    b.fill((1, F, 2), (3, F, 2), "cobblestone")
    b.set(2, F, 2, "furnace[facing=south]")
    b.set(3, F, 2, "furnace[facing=south]")
    b.fill((1, F + 1, 2), (3, F + 2, 2), "bricks")
    for x in (1, 2, 3):
        b.set(x, F + 2, 3, stairs("bricks", "north", top=True))
    b.stack(2, 1, F + 3, top + 2, "bricks", breast=[(1, 1), (3, 1), (2, 2)], shoulder=F + 5,
            pot="cobblestone_wall")
    b.set(1, F, 3, "barrel[facing=up]")
    b.mark("work", 2, F, 3)
    # the anvil, a slab of stone brick on a cobblestone post, and the smith's place by it
    b.set(3, F, 4, "cobblestone_wall")
    b.set(3, F + 1, 4, slab("stone_bricks"))
    b.mark("work", 4, F, 4)
    # the trough the work is quenched in, and the heap of ore and coal waiting
    b.fill((5, F, 2), (7, F, 3), "stone_bricks")
    b.set(6, F, 2, "water")
    b.set(7, F, 2, "water")
    b.set(1, F, 6, "coal_ore*2|cobblestone|iron_ore")
    b.set(1, F, 5, "coal_ore|iron_ore|gravel")
    b.set(1, F + 1, 6, "coal_ore|air*2")
    b.mark("loot", 7, F, 5)
    b.set(7, F, 4, "crafting_table")
    b.set(7, F, 6, "barrel[facing=up]")
    b.set(7, F + 1, 6, "lantern")
    b.fill((x0 + 1, F + 3, 4), (x1 - 1, F + 3, 4), "log[axis=x]")
    b.hang(5, F + 2, 4)
    b.hang(1, F + 2, 4)
    b.bracket_lamp(5, F + 2, z0 - 1, "north", material="stone_bricks")
    # the woodpile against the east end, under a slab roof
    b.fill((9, F, 2), (9, F + 1, 6), "log[axis=z]")
    b.fill((9, F + 1, 2), (9, F + 1, 6), "log[axis=z]*3|air")
    b.fill((9, F + 2, 1), (9, F + 2, 7), slab("dark_planks"))
    b.ground("path", (9, 1), (9, 7))
    # a lantern on a post at the yard's corner
    b.set(8, F, 8, "dark_planks_fence")
    b.set(8, F + 1, 8, "lantern")
    b.verge()
    b.mark("entrance", 4, F, 9)
    b.write_piece(
        "timber_smithy", "smithy", STYLES, "cobblestone",
        "The timber village's smithy, open to the street, its forge glowing under a brick hood.",
        SCRIPT,
    )


def bell_tower():
    """The bell tower: a square stone shaft, cobblestone weathering into stone brick, with a
    string course and a door at its foot, an open timber belfry where a gold bell hangs from a
    beam, and a steep plank spire with a finial over it. A ladder climbs inside for those who
    can climb; the warden keeps the door."""
    b = Piece(7, 25, 7)
    b.ground("path*3|cobblestone")
    x0, x1, z0, z1 = 1, 5, 0, 4
    lower = "cobblestone*5|mossy_cobblestone*2|stone_bricks"
    upper = "stone_bricks*6|mossy_stone_bricks|cracked_stone_bricks*2"
    # a plinth a block wider on three sides, a batter of stairs round it
    b.fill((x0, 0, z0), (x1, 0, z1), "cobblestone")
    for x in range(x0 - 1, x1 + 2):
        b.set(x, F, z1 + 1, stairs("cobblestone", "north") if x != 3 else None)
    for z in range(z0, z1 + 1):
        b.set(x0 - 1, F, z, stairs("cobblestone", "east"))
        b.set(x1 + 1, F, z, stairs("cobblestone", "west"))
    shaft = 12
    for y in range(F, F + shaft):
        b.walls((x0, y, z0), (x1, y, z1), lower if y < F + 6 else upper)
    b.corners((x0, F, z0), (x1, F + shaft - 1, z1), "stone_bricks")
    b.fill((x0 + 1, 0, z0 + 1), (x1 - 1, 0, z1 - 1), "cobblestone")
    b.fill((x0 + 1, F, z0 + 1), (x1 - 1, F + shaft - 1, z1 - 1), AIR)
    # the door, with a lantern on a bracket over it, and slits up the shaft
    b.door(3, F, z1, "south")
    b.bracket_lamp(4, F + 2, z1 + 1, "south", material="stone_bricks")
    for y in (F + 4, F + 8):
        b.set(3, y, z0, "glass_pane")
        b.set(x0, y, 2, "glass_pane")
        b.set(x1, y, 2, "glass_pane")
    b.set(3, F + 6, z1, "glass_pane")
    # a string course of upturned stairs round the shaft
    for x in range(x0 - 1, x1 + 2):
        b.set(x, F + 6, z0 - 1, stairs("stone_bricks", "south", top=True))
        b.set(x, F + 6, z1 + 1, stairs("stone_bricks", "north", top=True))
    for z in range(z0, z1 + 1):
        b.set(x0 - 1, F + 6, z, stairs("stone_bricks", "east", top=True))
        b.set(x1 + 1, F + 6, z, stairs("stone_bricks", "west", top=True))
    # inside: a ladder up the back wall, a barrel and a lantern by the warden
    b.ladder(3, z0 + 1, F, F + shaft - 1, "south")
    b.set(2, F, 3, "barrel[facing=up]")
    b.mark("work", 4, F, 3)
    b.hang(2, F + 3, 2)
    b.set(2, F + 4, 2, "dark_planks")
    b.hang(4, F + 8, 3)
    b.set(4, F + 9, 3, "dark_planks")
    # the belfry floor, a cornice of upturned stairs under it
    belfry = F + shaft
    b.fill((x0, belfry, z0), (x1, belfry, z1), "dark_planks")
    b.set(3, belfry, z0 + 1, AIR)
    b.ladder(3, z0 + 1, belfry, belfry, "south")
    # the belfry: log posts at the corners and a rail of fences between them, open arches
    for y in range(belfry + 1, belfry + 5):
        b.corners((x0, y, z0), (x1, y, z1), "log")
    for x in range(x0 + 1, x1):
        b.set(x, belfry + 1, z0, "dark_planks_fence")
        b.set(x, belfry + 1, z1, "dark_planks_fence")
    for z in range(z0 + 1, z1):
        b.set(x0, belfry + 1, z, "dark_planks_fence")
        b.set(x1, belfry + 1, z, "dark_planks_fence")
    b.set(3, belfry + 1, z0, AIR)
    b.beam_ring(x0, x1, z0, z1, belfry + 4)
    b.fill((x0 + 1, belfry + 4, 2), (x1 - 1, belfry + 4, 2), "log[axis=x]")
    # the bell, hung from the beam, and a lantern by it
    b.set(3, belfry + 3, 2, "gold_block")
    b.set(3, belfry + 2, 2, "cobblestone_wall")
    b.hang(2, belfry + 3, z1 - 1)
    b.set(2, belfry + 4, z1 - 1, "log[axis=x]")
    # the spire: steep, of dark planks, with a finial
    top = b.spire(x0, x1, z0, z1, belfry + 5, lambda f: stairs("dark_planks", f), "dark_planks",
                  slab("dark_planks"))
    b.set(3, top, 2, "dark_planks")
    b.set(3, top + 1, 2, "dark_planks_fence")
    b.set(3, top + 2, 2, "dark_planks_fence")
    b.mark("entrance", 3, F, 6)
    b.write_piece(
        "timber_bell_tower", "tower", STYLES, "cobblestone",
        "The timber village's bell tower, a gold bell in its open belfry under a steep spire.",
        SCRIPT,
    )


def well():
    """The village's well, its centre: a stone-lined shaft of water three deep behind a low
    rim, under a little roof on two posts with the windlass beam and a lantern, on a paved
    square with lanterns on posts at its corners, where the villagers meet."""
    b = Piece(7, 10, 7)
    b.floor = 3
    W = b.floor
    b.fill((0, W - 1, 0), (6, W - 1, 6), "path*3|cobblestone")
    b.fill((1, W - 1, 1), (5, W - 1, 5), "stone_bricks*5|mossy_stone_bricks|cobblestone*2")
    # the shaft, lined with stone, the water in it
    b.fill((2, 0, 2), (4, W, 4), "stone_bricks*4|mossy_stone_bricks*2|cobblestone*2")
    b.fill((3, 0, 3), (3, W - 1, 3), "water")
    b.set(3, W, 3, AIR)
    for x, z in ((2, 2), (4, 2), (2, 4), (4, 4)):
        b.set(x, W, z, "stone_bricks")
    # the posts, the windlass beam and the lantern hung over the water
    for x in (2, 4):
        b.fill((x, W + 1, 3), (x, W + 2, 3), "log")
    b.fill((2, W + 3, 3), (4, W + 3, 3), "log[axis=x]")
    b.hang(3, W + 2, 3)
    b.roof(2, 4, 2, 4, W + 3, ROOF, ROOF_FULL, ROOF_CAP, ends=1)
    # lanterns on posts at the square's corners
    for x, z in ((0, 0), (6, 0), (0, 6), (6, 6)):
        b.set(x, W, z, "cobblestone_wall")
        b.set(x, W + 1, z, "dark_planks_fence")
        b.set(x, W + 2, z, "lantern")
    for x, z in ((1, 1), (3, 1), (5, 1), (1, 3), (5, 3), (1, 5), (5, 5), (3, 5)):
        b.mark("gather", x, W, z)
    b.mark("entrance", 3, W, 6)
    b.write_piece(
        "timber_well", "well", STYLES, "stone_bricks",
        "The timber village's well under its little roof, on a square lit at the corners.",
        SCRIPT,
    )


def lamp():
    """A lamp post by the street: a timber post on a stone foot, two arms with a lantern hung
    from each, along the street."""
    b = Piece(3, 7, 1)
    b.ground("path")
    b.set(1, F, 0, "cobblestone_wall")
    b.fill((1, F + 1, 0), (1, F + 3, 0), "dark_planks_fence")
    b.fill((0, F + 3, 0), (2, F + 3, 0), "dark_planks_fence")
    b.set(1, F + 4, 0, slab("dark_planks"))
    b.hang(0, F + 2, 0)
    b.hang(2, F + 2, 0)
    b.write_piece(
        "timber_lamp", "lamp", STYLES, "cobblestone",
        "A lamp post of the timber village, two lanterns hung from its arms.", SCRIPT,
    )


cottage()
house()
longhouse()
stone_cottage()
inn()
smithy()
bell_tower()
well()
lamp()
