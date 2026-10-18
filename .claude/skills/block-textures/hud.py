#!/usr/bin/env python3
"""Draw the survival HUD's icons into assets/textures/hud.png, a strip of 9px icons in the order src/hud.rs reads them:
a heart, half a heart and an empty one, the same for food, then a bubble, then a chestplate for the armour bar, whole,
half and empty. Writes out/hud_contact.png to check them.
Usage: hud.py

They're small pictures like the item sprites (items.py): a dark outline round shades lit from the upper left. A half
heart is its left half, as hearts fill from the left, and half of the food its right, as the food bar fills from the
right. Half the armour is its left half, as its bar fills from the left as the hearts do."""
import numpy as np
from PIL import Image

from generate import HERE, OUT

HUD = HERE / "../../../assets/textures/hud.png"

HEART = [
    ".##...##.",
    "#hr#.#rr#",
    "#hrrrrrr#",
    "#rrrrrrd#",
    ".#rrrrd#.",
    "..#rrd#..",
    "...#d#...",
    "....#....",
    ".........",
]
FOOD = [
    "....###..",
    "...#hrr#.",
    "..#hrrrr#",
    "..#rrrrd#",
    "..#rrrd#.",
    ".##rdd#..",
    "#bb###...",
    "#bbb#....",
    ".##......",
]
BUBBLE = [
    ".........",
    "...###...",
    "..#hrr#..",
    ".#hrrrr#.",
    ".#rrrrr#.",
    ".#rrrrd#.",
    "..#rdd#..",
    "...###...",
    ".........",
]
ARMOUR = [
    "##.....##",
    "#h#...#r#",
    "#hrr#rrd#",
    ".#hrrrd#.",
    ".#hrrrd#.",
    ".#rrrrd#.",
    ".#rrrdd#.",
    "..#####..",
    ".........",
]
# What an empty icon's filled with, inside its outline.
EMPTY = (46, 40, 40, 220)


def icon(rows, colours, filled=lambda col: True):
    """`rows` painted in `colours` where `filled` says a column is, and as an empty one elsewhere."""
    out = np.zeros((9, 9, 4))
    for y, row in enumerate(rows):
        for x, ch in enumerate(row):
            if ch != ".":
                out[y, x] = colours[ch] if ch == "#" or filled(x) else EMPTY
    return out


heart = {"#": (24, 6, 6, 255), "h": (255, 156, 150, 255), "r": (218, 34, 34, 255), "d": (140, 12, 14, 255)}
food = {"#": (36, 18, 6, 255), "h": (230, 168, 110, 255), "r": (188, 110, 56, 255), "d": (128, 66, 30, 255),
        "b": (236, 228, 204, 255)}
armour = {"#": (28, 30, 36, 255), "h": (236, 240, 246, 255), "r": (176, 184, 196, 255), "d": (112, 120, 134, 255)}
bubble = {"#": (36, 84, 160, 255), "h": (255, 255, 255, 255), "r": (150, 204, 255, 200), "d": (96, 156, 230, 220)}

ICONS = [
    icon(HEART, heart), icon(HEART, heart, lambda x: x < 4), icon(HEART, heart, lambda x: False),
    icon(FOOD, food), icon(FOOD, food, lambda x: x >= 4), icon(FOOD, food, lambda x: False),
    icon(BUBBLE, bubble),
    icon(ARMOUR, armour), icon(ARMOUR, armour, lambda x: x < 4), icon(ARMOUR, armour, lambda x: False),
]

if __name__ == "__main__":
    strip = np.concatenate(ICONS, 1)
    Image.fromarray(strip.astype("uint8"), "RGBA").save(HUD)
    OUT.mkdir(exist_ok=True)
    sheet = Image.new("RGBA", (9 * len(ICONS) + 3 * (len(ICONS) + 1), 15), (120, 150, 90, 255))
    for i in range(len(ICONS)):
        sheet.alpha_composite(Image.open(HUD).crop((i * 9, 0, i * 9 + 9, 9)), (3 + i * 12, 3))
    sheet.resize((sheet.width * 8, sheet.height * 8), Image.NEAREST).save(OUT / "hud_contact.png")
    print("drew", len(ICONS), "icons; check", OUT / "hud_contact.png")
