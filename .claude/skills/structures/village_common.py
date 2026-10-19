"""Writes the village pieces the timber and lodge villages share into assets/structures/village/:
the farm and the animal pen, of fences, timber and fieldstone that suit either. The adobe
village has its own, walled in mud brick (village_adobe.py).

    python3 .claude/skills/structures/village_common.py
"""

from village import AIR, F, Piece, slab, stairs

SCRIPT = "village_common.py"
STYLES = ["timber", "lodge"]

# Wheat at every stage, more of it ripening than just sown, so a field looks worked.
WHEAT = "wheat_0|wheat_1|wheat_2*2|wheat_3*2|wheat_4*2|wheat_5*2|wheat_6*2|ripe_wheat*3"
FENCE = "planks_fence/dark_planks_fence"
KERB = "cobblestone*4|mossy_cobblestone*2|stone*1"


def scarecrow(b, x, z):
    """A scarecrow of a post, two arms, a head of straw and a plank hat."""
    b.fill((x, F, z), (x, F + 1, z), "dark_planks_fence")
    b.set(x - 1, F + 1, z, "dark_planks_fence")
    b.set(x + 1, F + 1, z, "dark_planks_fence")
    b.set(x, F + 2, z, "thatch")
    b.set(x, F + 3, z, slab("dark_planks"))


def farm():
    """A fenced field of wheat at every stage either side of a water channel, paths round it
    inside the fence, a scarecrow, a heap of straw and a barrel at the back, and gateposts with
    lanterns either side of the opening to the street."""
    b = Piece(13, 6, 11)
    # round the field, the world's own ground, snow or grass, as farms stand in either
    b.ground(None)
    x0, x1, z0, z1 = 0, 12, 0, 9
    # the field: farmland either side of the channel down its middle, paths round it
    b.ground("farmland", (2, 1), (10, 7))
    b.ground("water", (6, 1), (6, 7))
    b.ground("path*4|gravel", (1, 1), (1, 8))
    b.ground("path*4|gravel", (11, 1), (11, 8))
    b.ground("path*4|gravel", (1, 8), (11, 8))
    b.ground("path", (5, 9), (7, 10))
    for x in range(2, 11):
        for z in range(1, 8):
            if x != 6:
                b.set(x, F, z, WHEAT)
    # planks across the channel at the back, where the straw is carried over
    b.set(6, 0, 1, "planks")
    b.set(6, F, 1, AIR)
    # the fence round it, the opening in the front between taller gateposts
    for x in range(x0, x1 + 1):
        b.set(x, F, z0, FENCE)
        if not 5 <= x <= 7:
            b.set(x, F, z1, FENCE)
    for z in range(z0, z1 + 1):
        b.set(x0, F, z, FENCE)
        b.set(x1, F, z, FENCE)
    for x in (4, 8):
        b.fill((x, F, z1), (x, F + 1, z1), "log")
        b.set(x, F + 2, z1, "lantern")
    for x in (x0, x1):
        b.set(x, F + 1, z0, FENCE)
        b.set(x, F + 2, z0, "lantern")
    b.set(6, F + 1, 0, FENCE)
    b.set(6, F + 2, 0, "lantern")
    # the scarecrow among the wheat, and straw and a barrel at the back corners
    scarecrow(b, 8, 4)
    b.set(1, F, 1, "thatch")
    b.set(1, F, 2, "thatch")
    b.set(1, F + 1, 1, "thatch|air")
    b.set(11, F, 1, "barrel[facing=up]")
    b.set(11, F, 2, "barrel[facing=up]|air")
    b.set(3, F, 8, "barrel[facing=up]|air")
    # where the farmer works: among the wheat and on the path by it
    b.mark("work", 3, F, 4)
    b.mark("work", 9, F, 6)
    b.mark("work", 6, F, 8)
    b.mark("entrance", 6, F, 10)
    b.write_piece(
        "farm", "farm", STYLES, "dirt",
        "A fenced field of wheat either side of a water channel, with a scarecrow.", SCRIPT,
    )


def pen():
    """A pen for the village's animals: a kerb of fieldstone with a fence along it, too high to
    hop, closed all round, a trough of water and straw under a lean-to at the back, and the
    herder's place outside by a barrel of feed."""
    b = Piece(9, 6, 9)
    # round the pen, the world's own ground, snow or grass, as pens stand in either
    b.ground(None)
    x0, x1, z0, z1 = 0, 8, 0, 6
    b.ground("grass*5|path*2", (1, 1), (7, 5))
    for x in range(x0, x1 + 1):
        for z in (z0, z1):
            b.set(x, F, z, KERB)
            b.set(x, F + 1, z, FENCE)
    for z in range(z0, z1 + 1):
        for x in (x0, x1):
            b.set(x, F, z, KERB)
            b.set(x, F + 1, z, FENCE)
    # lanterns on the corner posts and the middle of the front
    for x, z in ((x0, z0), (x1, z0), (x0, z1), (x1, z1), (4, z1)):
        b.set(x, F + 1, z, FENCE)
        b.set(x, F + 2, z, "lantern")
    # the trough along the west kerb, and bales of straw kept off the kerb, as anything a
    # block high beside it would let an animal climb out over the fence
    b.fill((1, F, 2), (1, F, 4), "water")
    b.fill((2, F, 2), (2, F, 4), slab("dark_planks"))
    # a shelter in the north-east corner, its roof over the fence's height on posts too tall
    # to climb, straw under it
    for x in (5, 7):
        b.fill((x, F, 2), (x, F + 2, 2), FENCE)
        b.set(x, F + 2, z0, FENCE)
    b.set(x1, F + 2, 1, FENCE)
    b.fill((5, F + 3, z0), (x1, F + 3, 2), slab("dark_planks"))
    b.set(6, F, 2, "thatch")
    b.set(3, F, 2, "thatch|air")
    for x, z in ((3, 3), (5, 4), (4, 2), (6, 4)):
        b.mark("animal", x, F, z)
    # outside the front: a path along it, a barrel of feed and the herder's place
    b.ground("path*3|grass", (0, 7), (8, 8))
    # the barrels stand off the wall, which they'd make a step onto
    b.set(0, F, 8, "barrel[facing=up]")
    b.set(8, F, 8, "barrel[facing=up]|air")
    b.mark("work", 3, F, 7)
    b.mark("entrance", 4, F, 8)
    b.write_piece(
        "pen", "pen", STYLES, "dirt",
        "An animal pen fenced on a fieldstone kerb, with a trough and straw.", SCRIPT,
    )


farm()
pen()
