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
)
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
