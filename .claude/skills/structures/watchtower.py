"""Writes assets/structures/watchtower.toml: a ruined stone watchtower on a hilltop.

Seven blocks square and twenty layers tall: a cobblestone footing sunk a layer into the ground,
cobblestone courses rising into stone brick, a doorway, arrow slits, a stair of single blocks
climbing once round the inside to a floor under the parapet, and a corner fallen away. The ruin
is drawn from a fixed seed, so running this again writes the same file.

    python3 .claude/skills/structures/watchtower.py
"""

import random
from pathlib import Path

SIZE = 7
LAYERS = 20
# the layer standing at ground level, over the footing
FLOOR = 1
# the stair's last step, under the floor of the top
TOP = 17

rand = random.Random(7)


def ring(low, high):
    """The cells round the square low..high, clockwise from just past the middle of its -z
    side, as the stair climbs them."""
    cells = [(x, low) for x in range(low, high + 1)]
    cells += [(high, z) for z in range(low + 1, high + 1)]
    cells += [(x, high) for x in range(high - 1, low - 1, -1)]
    cells += [(low, z) for z in range(high - 1, low, -1)]
    middle = (low + high) // 2
    start = cells.index((middle + 1, low))
    return cells[start:] + cells[:start]


def on_wall(x, z):
    return x in (0, SIZE - 1) or z in (0, SIZE - 1)


def fallen(x, y, z):
    """Whether the block at x, y, z has fallen from the broken corner over +x, +z: the higher
    and nearer the corner, the more of it is gone."""
    from_corner = (SIZE - 1 - x) + (SIZE - 1 - z)
    return y >= 9 + 2 * from_corner + rand.choice((0, 0, 1))


stair = ring(1, SIZE - 2)
middle = SIZE // 2
slits = {(middle, 0), (SIZE - 1, middle), (middle, SIZE - 1), (0, middle)}
layers = []
for y in range(LAYERS):
    rows = []
    for z in range(SIZE):
        row = ""
        for x in range(SIZE):
            if y < FLOOR:
                cell = "c"
            elif on_wall(x, z):
                if (x, z) == (middle, 0) and y < FLOOR + 2:
                    cell = "-"  # the doorway
                elif (x, z) in slits and y in (6, 7, 12, 13):
                    cell = "-"
                elif y == LAYERS - 1 and (x + z) % 2:
                    cell = "."  # between the merlons
                elif y < FLOOR + 3 or rand.random() < 0.15:
                    cell = "c"
                else:
                    cell = "s"
                if cell != "-" and (fallen(x, y, z) or y >= LAYERS - 6 and rand.random() < 0.15):
                    cell = "."
            elif y == TOP:
                # the floor of the top, open over the last two steps so there's headroom on them
                cell = "." if (x, z) in stair[-2:] or fallen(x, y, z) else "s"
            elif y > TOP:
                cell = "."
            elif (x, z) in stair and stair.index((x, z)) == y - FLOOR:
                cell = "c"
            elif y == FLOOR and (x, z) in {(2, 3), (3, 3), (3, 4)}:
                cell = "g" if x == 3 else "c"  # rubble fallen down the middle
            else:
                cell = "-"
            row += cell
        rows.append(row)
    layers.append(rows)

out = [
    "# A ruined stone watchtower on a hilltop, written by .claude/skills/structures/watchtower.py.",
    'name = "watchtower"',
    "",
    "[placement]",
    "cell = 640",
    "chance = 0.6",
    'biomes = ["forest", "foothills", "taiga", "savanna"]',
    "min_above_sea = 12",
    "max_rise = 4",
    "hilltop = true",
    "",
    "[template]",
    f"size = [{SIZE}, {LAYERS}, {SIZE}]",
    f"floor = {FLOOR}",
    'foundation = "cobblestone"',
    'palette = { c = "cobblestone", s = "stone_bricks", g = "gravel" }',
    # a chest's place on the floor of the top
    f"markers = {{ loot = [[{middle}, {TOP + 1}, {middle}]] }}",
    "# bottom to top, each layer's rows running +z down the page and +x across",
    "layers = [",
]
for y, rows in enumerate(layers):
    out.append(f"  # {y}")
    out.append("  '''")
    out.extend(rows)
    out.append("''',")
out.append("]")
path = Path(__file__).resolve().parents[3] / "assets" / "structures" / "watchtower.toml"
path.write_text("\n".join(out) + "\n")
print(f"wrote {path}")
