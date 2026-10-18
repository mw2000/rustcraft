#!/usr/bin/env python3
"""Draw the furnace screen's flame and arrow into assets/textures/furnace.png, four 16px tiles: the flame out and lit,
and the arrow empty and full, in the order of the `FLAME_OUT`.. constants in src/inventory/furnace.rs. The screen
shows the lit flame and the full arrow over the others as far as the fire and the smelting have got, so each pair
shares its shape. Writes out/furnace_contact.png to check them on a slot's grey. Usage: ui.py"""
import numpy as np
from PIL import Image

from generate import HERE, OUT
from items import lit, outline

FURNACE = HERE / "../../../assets/textures/furnace.png"

y, x = np.mgrid[:16, :16]


def flame():
    # a teardrop narrowing to a flickering tip, with a tongue licking up beside it
    body = ((x - 7.5) / 4.2) ** 2 + ((y - 10.5) / 4.0) ** 2 < 1.0
    tip = (y >= 1) & (y <= 10) & (np.abs(x - 7.0 + (10 - y) * 0.2) < (y - 1) * 0.42)
    tongue = (y >= 4) & (y <= 10) & (np.abs(x - 10.5 + (10 - y) * 0.15) < (y - 4) * 0.3)
    return body | tip | tongue


def arrow():
    # pointing right, from the input across to the output
    shaft = (y >= 6) & (y <= 9) & (x >= 1) & (x <= 9)
    head = (x >= 9) & (x <= 15) & (np.abs(y - 7.5) <= 15 - x)
    return shaft | head


def tile(shape, colours, glow=None):
    out = np.zeros((16, 16, 4))
    lit(shape, colours, out)
    if glow is not None:
        # hotter towards the bottom of the flame: yellow at its heart, red at its tips
        heart = shape & (((x - 7.5) / 2.6) ** 2 + ((y - 11.5) / 2.6) ** 2 < 1.0)
        out[heart] = (*glow, 255)
    outline(out)
    return out


EMPTY = np.array([(56, 56, 56), (72, 72, 72), (88, 88, 88)], float)
FIRE = np.array([(200, 60, 20), (240, 130, 30), (255, 200, 80)], float)
FULL = np.array([(200, 200, 200), (232, 232, 232), (255, 255, 255)], float)

if __name__ == "__main__":
    tiles = [tile(flame(), EMPTY), tile(flame(), FIRE, glow=(255, 236, 140)), tile(arrow(), EMPTY), tile(arrow(), FULL)]
    strip = np.concatenate(tiles, 1)
    Image.fromarray(np.clip(strip.round(), 0, 255).astype("uint8"), "RGBA").save(FURNACE)
    OUT.mkdir(exist_ok=True)
    sheet = Image.new("RGBA", (16 * 4 + 4 * 5, 24), (107, 107, 107, 255))
    for i in range(4):
        sheet.alpha_composite(Image.open(FURNACE).crop((i * 16, 0, i * 16 + 16, 16)), (4 + i * 20, 4))
    sheet.resize((sheet.width * 6, sheet.height * 6), Image.NEAREST).save(OUT / "furnace_contact.png")
    print("drew the furnace's flame and arrow; check", OUT / "furnace_contact.png")
