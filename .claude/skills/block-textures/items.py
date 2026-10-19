#!/usr/bin/env python3
"""Draw the item sprites into assets/textures/items.png, one 16px tile per row of the `items!` table in src/items.rs, in
its order, and write out/items_contact.png to check them. Usage: items.py

Items aren't blocks, so they're small pictures on transparency rather than seamless tiles: a stick, coal, ingots, and tools
lying on the diagonal, handle at the bottom left and head at the top right, each lit from the upper left and outlined
so it reads on a slot's grey. Wood and stone take their shades from the block atlas, as draw.py's tiles do."""
import numpy as np
from PIL import Image

from draw import shades
from generate import HERE, OUT

ITEMS = HERE / "../../../assets/textures/items.png"

# Darkest first. The handle's a stick of the log end's wood, a shade darker than planks.
HANDLE = shades("LogEnd", 1.3)[[0, 1, 3]] * 0.8
MATERIALS = dict(
    wooden=shades("Planks", 1.2)[[0, 1, 2]],
    stone=shades("Cobblestone", 2.2)[[1, 2, 3]] * 1.25,
    copper=np.array([(150, 76, 44), (204, 112, 62), (236, 160, 104)], float),
    iron=np.array([(140, 140, 148), (196, 196, 202), (236, 236, 240)], float),
    bright_steel=np.array([(120, 138, 162), (184, 204, 224), (240, 248, 255)], float),
)
GOLD = np.array([(170, 120, 30), (232, 188, 56), (255, 236, 146)], float)
FROST = np.array([(86, 146, 196), (150, 210, 240), (220, 246, 255)], float)
JADE = np.array([(34, 110, 70), (72, 172, 112), (156, 226, 170)], float)
EMBER = np.array([(176, 54, 22), (244, 124, 40), (255, 226, 120)], float)
# Coal's black, a little blue where it catches the light, as coal ore's flecks are.
COAL = np.array([(24, 24, 28), (44, 44, 50), (78, 80, 90)], float)

y, x = np.mgrid[:16, :16]
# Along the diagonal from the bottom left (u) and across it, positive to the bottom right (v), in texels, through
# texel centres.
U = (x + 15 - y) / np.sqrt(2)
V = (x - (15 - y)) / np.sqrt(2)


def handle(top=15.0):
    return (np.abs(V) < 0.6) & (U > 2.5) & (U < top)


def pickaxe():
    # a head across the handle's end, its tips swept back towards the hand
    return (np.abs(V) < 7.0) & (np.abs(U - 15.8 + V * V * 0.08) < 1.3)


def axe():
    # a blade out to one side of the handle's top, widening to its edge, and a stub on the other
    blade = (V > 0) & (V < 5.0) & (np.abs(U - 14.6) < 1.4 + V * 0.5)
    return blade | ((V > -1.6) & (V <= 0) & (np.abs(U - 14.6) < 1.0))


def shovel():
    # a rounded spade on the handle's end
    return ((U - 15.6) / 3.2) ** 2 + (V / 2.4) ** 2 < 1.0


def hoe():
    # a narrow blade out to one side of the handle's top, turned down at its edge
    blade = (V > -0.6) & (V < 5.2) & (np.abs(U - 15.0) < 1.3)
    return blade | ((V > 3.0) & (V < 5.2) & (U > 11.6) & (U < 16.3))


def stick():
    return (np.abs(V) < 0.6) & (U > 2.5) & (U < 19.0)


def lump():
    # a knobbly lump, a few rounded chunks run together
    chunks = [(8.5, 8.0, 4.6), (5.5, 9.5, 3.2), (10.5, 5.5, 3.0), (11.0, 10.5, 2.8)]
    return np.any([(x - cx) ** 2 + (y - cy) ** 2 < r * r for cx, cy, r in chunks], 0)


def ingot():
    # a bar seen from above at a slant: its top face, and its front face below it
    return (y >= 6) & (y <= 11) & (x >= 2 + (11 - y) // 2) & (x <= 13 - (11 - y) // 2)


def crystal():
    # a long shard on the diagonal, pointed at both ends, with a shorter one leaning off its foot
    long = np.abs(V) / 2.4 + np.abs(U - 11.0) / 7.5 < 1.0
    short = np.abs(V - 3.2) / 1.5 + np.abs(U - 7.0) / 3.6 < 1.0
    return long | short


def gem():
    # a rounded pebble, smoothed as jade is
    return ((x - 7.5) / 5.6) ** 2 + ((y - 8.5) / 4.6) ** 2 < 1.0


def heap():
    # a little heap of grains, wider at its foot
    return (y >= 6) & (y <= 13) & (np.abs(x - 7.5) < (y - 4) * 0.85)


def jade():
    out = single(gem(), JADE)
    out[6, 5:7, :3] = out[7, 5, :3] = (214, 250, 222)  # a soft gleam, upper left
    return out


def blend():
    """A heap of iron filings and bright grains mixed, ready for the furnace."""
    out = np.zeros((16, 16, 4))
    lit(heap(), MATERIALS["iron"] * 0.85, out)
    for gy, gx in [(8, 7), (10, 5), (10, 10), (12, 8), (12, 3), (11, 12), (9, 9)]:
        out[gy, gx, :3] = (240, 222, 150)
    outline(out)
    return out


def ember():
    out = single(lump(), EMBER)
    for gy, gx in [(7, 8), (8, 7), (8, 8), (9, 9), (6, 10)]:  # its white-hot heart
        out[gy, gx, :3] = (255, 246, 190)
    return out


def apple():
    """A round fruit, a little wider than tall, dimpled where its stalk goes in, with a leaf beside the stalk."""
    out = np.zeros((16, 16, 4))
    body = ((x - 7.5) / 5.8) ** 2 + ((y - 9.5) / 5.3) ** 2 < 1.0
    body &= ~((np.abs(x - 7.5) < 1.0) & (y < 5))
    lit(body, np.array([(150, 22, 26), (204, 38, 36), (236, 92, 80)], float), out)
    out[6:8, 4] = out[6, 5] = (255, 200, 190, 255)  # a glint, upper left
    lit((x == 8) & (y >= 1) & (y <= 4), HANDLE, out)
    lit(((x - 11) / 2.2) ** 2 + ((y - 2.5) / 1.2) ** 2 < 1.0, np.array([(46, 102, 34), (72, 140, 48), (110, 176, 70)], float), out)
    outline(out)
    return out


def seeds():
    """A few seeds scattered, each a grain two texels long, lit at its upper end."""
    out = np.zeros((16, 16, 4))
    husk = np.array([(120, 100, 44), (176, 152, 76), (222, 204, 130)], float)
    for gy, gx in [(3, 7), (6, 11), (7, 3), (10, 8), (12, 3), (12, 12)]:
        lit((y >= gy) & (y <= gy + 2) & (x >= gx) & (x <= gx + 1) & ~((y == gy) & (x == gx + 1)), husk, out)
    outline(out)
    return out


def sheaf():
    """A sheaf of wheat lying on the diagonal: stalks bundled at the bottom left, tied, and fanning out into
    golden ears at the top right."""
    out = np.zeros((16, 16, 4))
    stalks = np.array([(150, 112, 40), (196, 158, 66), (226, 196, 108)], float)
    ears = np.array([(176, 128, 36), (226, 184, 76), (250, 226, 140)], float)
    fan = np.abs(V) < 0.7 + np.clip(U - 7, 0, None) * 0.28
    lit(fan & (U > 2.5) & (U < 12.5) & (np.round(V * 1.6) % 2 == 0), stalks, out)
    lit(fan & (U >= 11.5) & (U < 19.0) & ((x + y) % 2 == 0), ears, out)
    out[(U > 6.3) & (U < 7.7) & (np.abs(V) < 1.5)] = (110, 74, 34, 255)  # the band tying it
    outline(out)
    return out


def bread():
    """A loaf, longer than it's wide, its crust browner along its top, with slashes across it."""
    out = np.zeros((16, 16, 4))
    loaf = ((x - 7.5) / 6.8) ** 2 + ((y - 9.0) / 4.2) ** 2 < 1.0
    lit(loaf, np.array([(132, 74, 30), (186, 116, 52), (222, 160, 86)], float), out)
    for cut in [4, 7, 10]:  # slashes, pale where the crust split open
        out[(x - cut == (9 - y) // 2) & (y >= 6) & (y <= 9) & loaf] = (238, 204, 140, 255)
    outline(out)
    return out


def mutton(colours):
    """A leg of meat on the diagonal, fat along its top, with the bone sticking out of its narrow end."""
    def make():
        out = np.zeros((16, 16, 4))
        joint = ((U - 13.0) / 6.2) ** 2 + (V / (3.0 + (U - 7.0) * 0.25)) ** 2 < 1.0
        bone = (np.abs(V) < 0.8) & (U > 2.5) & (U < 8.0)
        knob = ((U - 3.0) ** 2 + V ** 2 < 2.2)
        lit((bone | knob) & ~joint, np.array([(178, 170, 150), (224, 218, 200), (246, 242, 230)], float), out)
        lit(joint, colours, out)
        out[joint & (V < -1.6) & (V > -3.4) & ~shift(joint, 1, -1)] = (*colours[2] * 0.5 + 120, 255)  # the fat on its back
        outline(out)
        return out
    return make


def wool():
    """A soft ball of wool, a few round tufts run together, curled all over."""
    out = np.zeros((16, 16, 4))
    tufts = [(8.0, 8.5, 4.8), (4.8, 9.5, 3.0), (11.2, 9.5, 3.0), (6.2, 5.2, 2.9), (10.0, 5.2, 2.9), (8.0, 12.5, 2.8)]
    ball = np.any([(x - cx) ** 2 + (y - cy) ** 2 < r * r for cx, cy, r in tufts], 0)
    lit(ball, np.array([(178, 172, 158), (222, 218, 206), (248, 246, 238)], float), out)
    for cx, cy in [(5, 8), (8, 6), (11, 8), (6, 12), (9, 10), (12, 11), (8, 13)]:  # the curls of its fleece
        out[cy, cx, :3] = (196, 190, 176)
        out[cy - 1, cx - 1, :3] = (250, 249, 244)
    outline(out)
    return out


def shears():
    """Shears lying on the diagonal, closed: two iron blades side by side out to the top right from a rivet, and
    two loops of handle at the bottom left."""
    out = np.zeros((16, 16, 4))
    taper = np.clip((19.5 - U) / 10.0, 0, 1)
    blades = (U > 8.5) & (U < 19.5) & (np.abs(V) < 1.5 * taper + 0.4)
    loops = np.zeros_like(blades)
    for v in [-2.6, 2.6]:
        d = (U - 4.4) ** 2 + (V - v) ** 2
        loops |= (d < 6.0) & (d > 1.8)
    arms = (U > 5.5) & (U < 9.0) & (np.abs(np.abs(V) - (9.0 - U) * 0.6) < 0.6)
    lit(loops | arms, np.array([(112, 40, 34), (164, 62, 48), (204, 98, 76)], float), out)
    lit(blades, MATERIALS["iron"], out)
    out[blades & (np.abs(V) < 0.4) & (U > 10.5) & (U < 18.0)] = (*MATERIALS["iron"][0] * 0.8, 255)  # where they meet
    out[(np.abs(U - 8.9) < 0.8) & (np.abs(V) < 0.8)] = (90, 90, 98, 255)  # the rivet
    outline(out)
    return out


def leather():
    """A hide, a rounded square with its legs' corners drawn out and its edges a little ragged."""
    out = np.zeros((16, 16, 4))
    body = ((np.abs(x - 7.5) / 5.6) ** 2.5 + (np.abs(y - 7.5) / 5.0) ** 2.5 < 1.0)
    corners = np.any([(x - cx) ** 2 + (y - cy) ** 2 < 3.2 for cx, cy in [(2, 2), (13, 2), (2, 13), (13, 13)]], 0)
    hide = (body | corners) & ~(((x + y * 3) % 7 == 0) & ~shift(body, 1, 1))
    lit(hide, np.array([(102, 60, 34), (146, 90, 52), (180, 122, 76)], float), out)
    for cy, cx in [(6, 5), (9, 10), (5, 10), (10, 6)]:  # its grain
        out[cy, cx, :3] = (124, 76, 44)
    outline(out)
    return out


def feather():
    """A feather on the diagonal: a pale quill bottom left, and the vane widening round it and tapering to a tip."""
    out = np.zeros((16, 16, 4))
    width = np.clip(np.minimum(U - 5.0, 19.5 - U) * 0.42, 0, 2.6)
    vane = (np.abs(V + 0.15 * (U - 12) ** 2 / 12) < width) & (U > 5.0)
    lit(vane & ~((np.round(U * 1.3) % 3 == 0) & (np.abs(V) > 1.2)), np.array([(176, 180, 186), (222, 224, 228), (248, 248, 250)], float), out)
    out[(np.abs(V + 0.15 * (U - 12) ** 2 / 12) < 0.5) & (U > 2.5) & (U < 18.0)] = (196, 184, 160, 255)  # the quill
    outline(out)
    return out


def steak(colours):
    """A steak, a rounded slab a little wider than tall with a rim of fat down one side and a streak through it."""
    def make():
        out = np.zeros((16, 16, 4))
        slab = ((x - 7.5) / 6.4) ** 2 + ((y - 8.0) / 5.2) ** 2 + 0.02 * (x - 7.5) * (y - 8.0) < 1.0
        lit(slab, colours, out)
        fat = slab & ~shift(slab, 0, -2) & (x > 9)
        out[fat] = (*colours[2] * 0.4 + 150, 255)
        out[slab & (np.abs(y - 8 - (x - 7.5) * 0.4) < 0.5) & (x > 3) & (x < 11)] = (*colours[2] * 0.5 + 110, 255)
        outline(out)
        return out
    return make


def chop(colours):
    """A chop: a cut of meat, rounder at one end, with its rind along the top and a stub of bone out of the other."""
    def make():
        out = np.zeros((16, 16, 4))
        cut = ((x - 8.5) / 5.8) ** 2 + ((y - 8.5) / (4.4 - (x - 8.5) * 0.12)) ** 2 < 1.0
        bone = (x >= 1) & (x <= 4) & (np.abs(y - 9.5) < 1.2)
        lit(bone & ~cut, np.array([(178, 170, 150), (224, 218, 200), (246, 242, 230)], float), out)
        lit(cut, colours, out)
        rind = cut & ~shift(cut, 2, 0)
        out[rind] = (*colours[2] * 0.45 + 130, 255)
        outline(out)
        return out
    return make


def drumstick(colours):
    """A drumstick: a plump round of meat on the diagonal and its thin bone, knobbed at the end."""
    def make():
        out = np.zeros((16, 16, 4))
        meat = ((U - 13.0) / 5.0) ** 2 + (V / 3.6) ** 2 < 1.0
        bone = (np.abs(V) < 0.7) & (U > 3.5) & (U < 9.0)
        knob = ((U - 3.5) ** 2 + (V - 0.9) ** 2 < 1.6) | ((U - 3.5) ** 2 + (V + 0.9) ** 2 < 1.6)
        lit((bone | knob) & ~meat, np.array([(178, 170, 150), (224, 218, 200), (246, 242, 230)], float), out)
        lit(meat, colours, out)
        outline(out)
        return out
    return make


def sword(colours):
    """A sword on the diagonal: a straight blade to a point at the top right, a crossguard, and a short grip with a
    pommel at the bottom left."""
    def make():
        out = np.zeros((16, 16, 4))
        blade = (U > 6.4) & (U < 20.2) & (np.abs(V) < np.clip((20.2 - U) * 0.5, 0, 1.3))
        guard = (np.abs(U - 6.0) < 0.8) & (np.abs(V) < 3.2)
        grip = (np.abs(V) < 0.6) & (U > 2.6) & (U < 5.6)
        pommel = (U - 2.4) ** 2 + V ** 2 < 1.4
        lit(grip | pommel, HANDLE, out)
        lit(guard, colours * 0.85, out)
        lit(blade, colours, out)
        out[blade & (np.abs(V + 0.35) < 0.3) & (U > 7.5) & (U < 18.0)] = (*colours[2], 255)  # its fuller catching the light
        outline(out)
        return out
    return make


def helmet():
    # a dome over a brow, open at the face
    dome = ((x - 7.5) / 6.2) ** 2 + ((y - 9.0) / 6.4) ** 2 < 1.0
    return dome & (y <= 11) & ~((y >= 8) & (np.abs(x - 7.5) < 3.6))


def chestplate():
    # a breastplate with shoulders, a notch for the neck and the arms cut away below the shoulders
    body = (np.abs(x - 7.5) < 6.6) & (y >= 2) & (y <= 14)
    neck = (np.abs(x - 7.5) < 2.2) & (y <= 4)
    arms = (np.abs(x - 7.5) > 4.0) & (y >= 7)
    return body & ~neck & ~arms


def leggings():
    # a waistband and two legs apart
    waist = (np.abs(x - 7.5) < 5.6) & (y >= 2) & (y <= 5)
    legs = (np.abs(np.abs(x - 7.5) - 3.0) < 2.4) & (y >= 5) & (y <= 14)
    return waist | legs


def boots():
    # two boots side by side, toes forward to the left
    return np.any([(x >= x0 + 2) & (x <= x0 + 4) & (y >= 5) & (y <= 13) | (x >= x0) & (x <= x0 + 4) & (y >= 11) & (y <= 13)
                   for x0 in [1, 9]], 0)


PIECES = dict(helmet=helmet, chestplate=chestplate, leggings=leggings, boots=boots)
LEATHER = np.array([(102, 60, 34), (146, 90, 52), (180, 122, 76)], float)


def piece(shape, colours):
    """A piece of armour: its shape in its material, a darker band along its edges where it's trimmed."""
    def make():
        out = single(shape(), colours)
        out[(out[..., 3] > 0) & (y == 2 + (shape is boots) * 9) & (out[..., 0] > 0)] *= (0.8, 0.8, 0.8, 1)
        return out
    return make


def shift(a, dy, dx):
    """`a` moved by (dy, dx), filling in with zeros: sprites don't wrap round, as tiles do."""
    out = np.zeros_like(a)
    out[max(dy, 0):16 + min(dy, 0), max(dx, 0):16 + min(dx, 0)] = a[max(-dy, 0):16 + min(-dy, 0), max(-dx, 0):16 + min(-dx, 0)]
    return out


def lit(shape, colours, out):
    """Paint `shape` in a material's three shades: lit where its upper left is open, shaded where its lower right is."""
    inside = lambda dy, dx: shift(shape, dy, dx)
    upper_left_open = ~inside(1, 0) | ~inside(0, 1)
    lower_right_open = ~inside(-1, 0) | ~inside(0, -1)
    out[shape] = (*colours[1], 255)
    out[shape & lower_right_open] = (*colours[0], 255)
    out[shape & upper_left_open & ~lower_right_open] = (*colours[2], 255)


def outline(out):
    """A dark edge round everything drawn, each texel a darker take on what it borders."""
    drawn = out[..., 3] > 0
    edge = np.zeros_like(drawn)
    darkest = np.zeros((16, 16, 3))
    for dy, dx in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
        near = shift(drawn, dy, dx) & ~drawn
        rgb = shift(out[..., :3], dy, dx)
        darkest = np.where((near & ~edge)[..., None], rgb, darkest)
        edge |= near
    out[edge] = np.concatenate([darkest[edge] * 0.35, np.full((edge.sum(), 1), 255)], 1)


def tool(head, colours):
    out = np.zeros((16, 16, 4))
    shape = head()
    lit(handle() & ~shape, HANDLE, out)
    lit(shape, colours, out)
    outline(out)
    return out


def bar(colours):
    out = np.zeros((16, 16, 4))
    lit(ingot(), colours, out)
    out[6, :, :3] = np.where(out[6, :, 3:] > 0, colours[2], 0)  # a bright top edge, as ingots catch the light
    outline(out)
    return out


def single(shape, colours):
    out = np.zeros((16, 16, 4))
    lit(shape, colours, out)
    outline(out)
    return out


HEADS = dict(pickaxe=pickaxe, axe=axe, shovel=shovel)  # and hoes, after the apple
# In the order of the `items!` table.
def coal():
    out = single(lump(), COAL)
    # glints where its facets catch the light
    for gy, gx in [(5, 6), (6, 7), (8, 4), (4, 10)]:
        out[gy, gx, :3] = (120, 124, 138)
    return out


SPRITES = dict(stick=lambda: single(stick(), HANDLE), coal=coal, copper_ingot=lambda: bar(MATERIALS["copper"]), iron_ingot=lambda: bar(MATERIALS["iron"]))
for material in ["wooden", "stone", "copper", "iron"]:
    for kind, head in HEADS.items():
        SPRITES[f"{material}_{kind}"] = lambda head=head, material=material: tool(head, MATERIALS[material])
SPRITES["apple"] = apple
for material in ["wooden", "stone", "copper", "iron"]:
    SPRITES[f"{material}_hoe"] = lambda material=material: tool(hoe, MATERIALS[material])
SPRITES.update(seeds=seeds, wheat=sheaf, bread=bread)
SPRITES.update(mutton=mutton(np.array([(150, 40, 46), (200, 66, 70), (232, 118, 112)], float)),
               cooked_mutton=mutton(np.array([(92, 48, 26), (138, 78, 42), (180, 116, 66)], float)))
SPRITES.update(gold_ingot=lambda: bar(GOLD), frost_crystal=lambda: single(crystal(), FROST), jade=jade,
               bright_steel_blend=blend, bright_steel_ingot=lambda: bar(MATERIALS["bright_steel"]), ember=ember)
for kind, head in [*HEADS.items(), ("hoe", hoe)]:
    SPRITES[f"bright_steel_{kind}"] = lambda head=head: tool(head, MATERIALS["bright_steel"])
SPRITES.update(wool=wool, shears=shears, leather=leather, feather=feather,
               beef=steak(np.array([(140, 30, 36), (190, 52, 56), (226, 100, 96)], float)),
               cooked_beef=steak(np.array([(80, 42, 24), (122, 70, 38), (164, 106, 60)], float)),
               pork=chop(np.array([(186, 92, 98), (226, 136, 138), (246, 180, 176)], float)),
               cooked_pork=chop(np.array([(120, 70, 36), (168, 108, 58), (206, 150, 92)], float)),
               chicken=drumstick(np.array([(196, 138, 120), (232, 184, 164), (248, 220, 204)], float)),
               cooked_chicken=drumstick(np.array([(138, 84, 30), (186, 124, 52), (222, 168, 86)], float)))
for material in ["wooden", "stone", "copper", "iron", "bright_steel"]:
    SPRITES[f"{material}_sword"] = sword(MATERIALS[material])
for material, colours in [("leather", LEATHER), ("iron", MATERIALS["iron"]), ("bright_steel", MATERIALS["bright_steel"])]:
    for kind, shape in PIECES.items():
        SPRITES[f"{material}_{kind}"] = piece(shape, colours)


def coin():
    """A round coin of bronze with a square hole through its middle, its rim raised and a mark stamped either
    side of the hole, the villages' money."""
    y, x = np.mgrid[:16, :16]
    r = np.hypot(y - 7.5, x - 7.5)
    shape = (r < 6.2) & ~((abs(y - 7.5) < 1.5) & (abs(x - 7.5) < 1.5))
    out = np.zeros((16, 16, 4))
    lit(shape, COIN, out)
    face = shape & (r < 4.6) & ~((abs(y - 7.5) < 2.5) & (abs(x - 7.5) < 2.5))
    out[face, :3] = COIN[1] * 0.92  # the face, sunk inside the rim
    for my, mx in [(4, 7), (4, 8), (11, 7), (11, 8), (7, 4), (8, 4), (7, 11), (8, 11)]:
        out[my, mx, :3] = COIN[2]  # the stamped marks
    outline(out)
    return out


COIN = np.array([(120, 74, 34), (176, 118, 52), (226, 178, 96)], float)
SPRITES["coin"] = coin


def bucket(contents=None):
    """An iron pail seen a little from above, wider at its rim, with its handle arched over it: empty, dark inside,
    or full to the rim of what it carries."""
    def make():
        y, x = np.mgrid[:16, :16]
        rim = ((x - 7.5) / 5.6) ** 2 + ((y - 6.5) / 1.8) ** 2 < 1.0
        half = 5.2 - (y - 6.5) * 0.28
        body = (y >= 6.5) & (y < 14.5) & (np.abs(x - 7.5) < half)
        handle = (np.abs(np.hypot((x - 7.5) / 5.4, (y - 6.5) / 5.4) - 1.0) < 0.1) & (y < 6)
        out = np.zeros((16, 16, 4))
        lit(body | rim | handle, MATERIALS["iron"], out)
        inside = ((x - 7.5) / 4.4) ** 2 + ((y - 6.5) / 1.1) ** 2 < 1.0
        out[inside, :3] = (52, 52, 58) if contents is None else contents[(x[inside] + y[inside]) % 2]
        outline(out)
        return out
    return make


SPRITES.update(bucket=bucket(), water_bucket=bucket(np.array([(48, 88, 200), (72, 120, 228)], float)),
               lava_bucket=bucket(np.array([(240, 110, 28), (255, 176, 60)], float)))


def table_order():
    """The names in the `items!` table, which the tiles must follow."""
    import re
    source = (HERE / "../../../src/items.rs").read_text()
    table = source[source.index("items! {"):]
    return re.findall(r'name: "([a-z_]+)"', table[:table.index("\n}\n")])


if __name__ == "__main__":
    names = table_order()
    assert names == list(SPRITES), f"items.py's SPRITES differ from the items! table: {names}"
    strip = np.concatenate([SPRITES[name]() for name in names], 1)
    Image.fromarray(np.clip(strip.round(), 0, 255).astype("uint8"), "RGBA").save(ITEMS)
    # on a slot's grey, 6x
    OUT.mkdir(exist_ok=True)
    sheet = Image.new("RGBA", (16 * len(names) + 4 * (len(names) + 1), 24), (107, 107, 107, 255))
    for i in range(len(names)):
        sheet.alpha_composite(Image.open(ITEMS).crop((i * 16, 0, i * 16 + 16, 16)), (4 + i * 20, 4))
    sheet.resize((sheet.width * 6, sheet.height * 6), Image.NEAREST).save(OUT / "items_contact.png")
    print("drew", len(names), "items; check", OUT / "items_contact.png")
