#!/usr/bin/env python3
"""Draw the cracks a block shows as it's broken into assets/textures/cracks.png: one 16px tile per stage, in order, as
`CRACK_STAGES` in src/breaking.rs, and write out/cracks_contact.png with each over stone. Usage: cracks.py [--seed N]

The cracks are one network, grown from the middle a texel at a time along wandering lines that fork, and each stage
shows more of it, in the order it grew, so they spread out from where the block's struck. They're dark texels on
transparency, with a pale edge below and to the right of each, as if the crack's lip catches the light."""
import argparse
import re

import numpy as np
from PIL import Image

from draw import source
from generate import HERE, OUT

CRACKS = HERE / "../../../assets/textures/cracks.png"
STAGES = int(re.search(r"CRACK_STAGES: usize = (\d+);", (HERE / "../../../src/breaking.rs").read_text()).group(1))
# The share of the network the first stage shows, so even it's a visible crack, and how many texels the whole of it
# covers, about a quarter of the block's face, so the last stage is a block about to go rather than a black one.
FIRST = 0.12
SIZE = 64
DIRECTIONS = [(-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1), (0, -1), (-1, -1)]


def grow(rng):
    """The network's texels, in the order they grew."""
    order, seen = [], set()
    # lines leave the middle every way, and fork as they go
    lines = [((7 + rng.integers(0, 2), 7 + rng.integers(0, 2)), d) for d in rng.permutation(8)[:5]]
    while lines and len(order) < SIZE:
        (y, x), d = lines.pop(0)
        for _ in range(rng.integers(3, 7)):
            if (y, x) not in seen:
                seen.add((y, x))
                order.append((y, x))
            d = (d + rng.choice([-1, 0, 0, 0, 1])) % 8  # mostly straight on
            y, x = y + DIRECTIONS[d][0], x + DIRECTIONS[d][1]
            if not (0 <= y < 16 and 0 <= x < 16):
                break
        else:
            lines.append(((y, x), d))
            if rng.random() < 0.4:
                lines.append(((y, x), (d + rng.choice([-2, 2])) % 8))
    return order


def stage(order, n):
    out = np.zeros((16, 16, 4))
    shown = order[:int(len(order) * (FIRST + (1 - FIRST) * n / (STAGES - 1)))]
    for y, x in shown:
        for dy, dx in [(1, 0), (0, 1)]:  # the lip, under what's already drawn
            if 0 <= y + dy < 16 and 0 <= x + dx < 16 and out[y + dy, x + dx, 3] == 0:
                out[y + dy, x + dx] = (200, 200, 200, 28)
        out[y, x] = (16, 14, 12, 210)
    return out


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", type=int, default=2)
    a = ap.parse_args()
    order = grow(np.random.default_rng(a.seed))
    strip = np.concatenate([stage(order, n) for n in range(STAGES)], 1)
    Image.fromarray(strip.round().astype("uint8"), "RGBA").save(CRACKS)
    OUT.mkdir(exist_ok=True)
    stone = Image.fromarray(source("Stone").astype("uint8")).convert("RGBA")
    sheet = Image.new("RGBA", (16 * STAGES, 16))
    for n in range(STAGES):
        sheet.paste(stone, (n * 16, 0))
    sheet.alpha_composite(Image.open(CRACKS))
    sheet.resize((sheet.width * 8, sheet.height * 8), Image.NEAREST).save(OUT / "cracks_contact.png")
    print("drew", STAGES, "crack stages; check", OUT / "cracks_contact.png")
