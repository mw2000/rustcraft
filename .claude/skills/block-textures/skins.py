#!/usr/bin/env python3
"""Draw the default player skins into assets/textures/skins/<name>.png, in the 64x64 layout Minecraft's skins use, so a
player's own skin and ours draw on the same model (src/skin.rs, src/client/skins.rs), and write out/skins_contact.png
to check them: each skin's sheet, and its model's front and back laid flat. Usage: skins.py

The layout is the format only: these are our own characters (INV-8), never Steve or Alex. Each box of the model is
unwrapped as the layout has it, its top and bottom side by side above its right side, front, left side and back, the
player's right on the left of the front. Each character paints each face of each part from a few shades, by the row
down the part (a sleeve's top rows, boots at a leg's bottom), with a little noise so faces aren't flat; the overlay
(a hood, a hard hat, a straw hat) is drawn over the base where it covers it and left clear elsewhere."""
from pathlib import Path

import numpy as np
from PIL import Image

HERE = Path(__file__).parent
OUT = HERE / "out"
SKINS = HERE / "../../../assets/textures/skins"

# Each part's texture in the base layer and the overlay, and its box, in pixels: wide, tall and deep.
PARTS = dict(
    head=((0, 0), (32, 0), (8, 8, 8)),
    body=((16, 16), (16, 32), (8, 12, 4)),
    right_arm=((40, 16), (40, 32), (4, 12, 4)),
    left_arm=((32, 48), (48, 48), (4, 12, 4)),
    right_leg=((0, 16), (0, 32), (4, 12, 4)),
    left_leg=((16, 48), (0, 48), (4, 12, 4)),
)


def faces(u, v, w, h, d):
    """Each face of a box at (u, v), as (x, y, wide, tall) on the sheet."""
    return dict(top=(u + d, v, w, d), bottom=(u + d + w, v, w, d), right=(u, v + d, d, h),
                front=(u + d, v + d, w, h), left=(u + d + w, v + d, d, h), back=(u + 2 * d + w, v + d, w, h))


def shade(colour, by):
    return tuple(int(np.clip(c * by, 0, 255)) for c in colour)


class Skin:
    def __init__(self, slim, seed):
        self.pixels = np.zeros((64, 64, 4), np.uint8)
        self.slim = slim
        self.rng = np.random.default_rng(seed)

    def paint(self, part, layer, paint):
        """Paints every face of `part`'s box in `layer` ("base" or "over"): `paint(face, x, y, wide, tall)`
        gives the colour at (x, y) across and down the face, or None to leave it clear."""
        base, over, (w, h, d) = PARTS[part]
        if part.endswith("arm") and self.slim:
            w = 3
        u, v = base if layer == "base" else over
        for face, (fx, fy, fw, fh) in faces(u, v, w, h, d).items():
            for y in range(fh):
                for x in range(fw):
                    colour = paint(face, x, y, fw, fh)
                    if colour is None:
                        continue
                    # a little noise, so no face is flat
                    jitter = 1.0 + self.rng.normal(0, 0.035)
                    self.pixels[fy + y, fx + x] = (*shade(colour[:3], jitter), colour[3] if len(colour) > 3 else 255)

    def save(self, name):
        SKINS.mkdir(parents=True, exist_ok=True)
        Image.fromarray(self.pixels, "RGBA").save(SKINS / f"{name}.png", optimize=True)


def face(skin, hair, eyes, *, fringe=2, moustache=None, mouth=None):
    """A head: hair over the top, the back and the sides' upper rows, and a face on the front with eyes on its
    fifth row, whites outside, and a mouth, under a moustache if there's one."""
    mouth = mouth or shade(skin, 0.72)

    def paint(side, x, y, w, h):
        if side == "top":
            return hair
        if side == "bottom":
            return shade(skin, 0.85)
        if side == "back":
            return hair if y < 7 else shade(skin, 0.9)
        if side in ("left", "right"):
            # hair down to the ears, over the back half of each side
            near_back = x < 4 if side == "right" else x >= 4
            if y < fringe or (near_back and y < 6):
                return hair
            return skin
        # the front
        if y < fringe:
            return hair
        if y == 4 and x in (1, 6):
            return (236, 236, 236)
        if y == 4 and x in (2, 5):
            return eyes
        if y == 3 and x in (1, 2, 5, 6):
            return shade(hair, 0.85)
        if moustache and y == 5 and 2 <= x <= 5:
            return moustache
        if y == 6 and x in (3, 4):
            return mouth
        if y == 5 and x in (3, 4):
            return shade(skin, 0.92)
        return skin
    return paint


def rows(*bands):
    """Paints a part by its row down it: `bands` are (rows, colour), the last one to the bottom, tops and bottoms
    taking the first's and the last's colours."""
    def paint(side, x, y, w, h):
        if side == "top":
            return bands[0][1]
        if side == "bottom":
            return shade(bands[-1][1], 0.8)
        for until, colour in bands:
            if y < until:
                return colour
        return bands[-1][1]
    return paint


def ranger():
    skin = Skin(slim=False, seed=1)
    tone, hair, eyes = (198, 140, 100), (78, 50, 30), (62, 96, 52)
    tunic, belt, trousers, boots = (62, 104, 52), (92, 62, 34), (110, 86, 58), (60, 42, 28)
    skin.paint("head", "base", face(tone, hair, eyes))
    # a green hood, open round the face
    def hood(side, x, y, w, h):
        if side == "front":
            return (*shade(tunic, 0.9), 255) if y == 0 or x in (0, 7) and y < 6 else None
        if side == "bottom":
            return None
        return shade(tunic, 0.92 if side == "top" else 1.0)
    skin.paint("head", "over", hood)

    def body(side, x, y, w, h):
        if side in ("front", "back") and y == 7:
            return belt
        if side == "front" and y == 7 and x in (3, 4):
            return (196, 168, 80)
        if side == "front" and y < 7 and x in (3, 4) and y % 2 == 1:
            return shade(tunic, 0.75)
        return rows((7, tunic), (8, belt), (12, tunic))(side, x, y, w, h)
    skin.paint("body", "base", body)
    for arm in ("right_arm", "left_arm"):
        skin.paint(arm, "base", rows((9, tunic), (10, shade(tunic, 0.8)), (12, tone)))
    for leg in ("right_leg", "left_leg"):
        skin.paint(leg, "base", rows((8, trousers), (12, boots)))
    return skin


def miner():
    skin = Skin(slim=False, seed=2)
    tone, hair, eyes = (124, 84, 58), (30, 24, 22), (52, 34, 24)
    shirt, overalls, trousers, boots = (120, 128, 140), (54, 78, 132), (54, 78, 132), (92, 62, 34)
    skin.paint("head", "base", face(tone, hair, eyes, fringe=1, moustache=(58, 44, 38)))
    hat = (232, 150, 40)

    def hard_hat(side, x, y, w, h):
        if side == "bottom":
            return None
        if side == "top":
            return hat
        if side == "front" and y == 1 and x in (3, 4):
            return (255, 240, 150)
        return shade(hat, 0.85 if y == 2 else 1.0) if y < 3 else None
    skin.paint("head", "over", hard_hat)

    def body(side, x, y, w, h):
        # the overalls' bib and straps over a grey shirt
        if side == "front" and (y >= 4 or x in (1, 6)) and 1 <= x <= 6:
            if y == 5 and x in (3, 4):
                return shade(overalls, 0.7)
            return overalls
        if side == "back" and x in (1, 6) or y >= 9:
            return overalls
        return rows((12, shirt))(side, x, y, w, h)
    skin.paint("body", "base", body)
    for arm in ("right_arm", "left_arm"):
        skin.paint(arm, "base", rows((6, shirt), (10, tone), (12, (176, 150, 96))))
    for leg in ("right_leg", "left_leg"):
        skin.paint(leg, "base", rows((9, trousers), (12, boots)))
    return skin


def sailor():
    skin = Skin(slim=True, seed=3)
    tone, hair, eyes = (238, 196, 166), (176, 84, 40), (60, 100, 150)
    navy, white, scarf, shoes = (36, 46, 88), (232, 232, 226), (190, 44, 40), (240, 240, 236)
    head = face(tone, hair, eyes)

    def long_hair(side, x, y, w, h):
        # down the back past the neck, and over the sides' backs
        if side == "back" or side in ("left", "right") and (x < 3 if side == "right" else x >= 5):
            return hair
        return head(side, x, y, w, h)
    skin.paint("head", "base", long_hair)

    def body(side, x, y, w, h):
        if side == "front" and y < 3 and 2 <= x <= 5 and (y < 2 or 3 <= x <= 4):
            return scarf
        if y >= 10:
            return navy
        return white if y % 2 == 0 else navy
    skin.paint("body", "base", body)
    for arm in ("right_arm", "left_arm"):
        skin.paint(arm, "base", lambda s, x, y, w, h: tone if y >= 5 else (white if y % 2 == 0 else navy))
    for leg in ("right_leg", "left_leg"):
        skin.paint(leg, "base", rows((10, navy), (12, shoes)))
    # the hair's ends over the shoulders, in the jacket's layer
    skin.paint("body", "over", lambda s, x, y, w, h: hair if s == "back" and y < 3 else None)
    return skin


def botanist():
    skin = Skin(slim=True, seed=4)
    tone, hair, eyes = (176, 128, 92), (24, 20, 20), (40, 30, 24)
    shirt, apron, trousers, boots = (52, 138, 132), (110, 152, 64), (168, 146, 104), (66, 92, 48)
    skin.paint("head", "base", face(tone, hair, eyes, fringe=2))
    straw = (226, 196, 112)

    def sun_hat(side, x, y, w, h):
        if side == "bottom":
            return None
        if side == "top":
            return straw
        if y == 2:
            return (150, 70, 60)
        return shade(straw, 0.9) if y < 2 else None
    skin.paint("head", "over", sun_hat)

    def body(side, x, y, w, h):
        if side == "front" and y >= 3 and 1 <= x <= 6:
            if y == 6 and 2 <= x <= 5:
                return shade(apron, 0.75)
            return apron
        if side in ("left", "right") and y == 7:
            return apron
        return rows((12, shirt))(side, x, y, w, h)
    skin.paint("body", "base", body)
    for arm in ("right_arm", "left_arm"):
        skin.paint(arm, "base", rows((7, shirt), (12, tone)))
    for leg in ("right_leg", "left_leg"):
        skin.paint(leg, "base", rows((8, trousers), (12, boots)))
    return skin


DRAW = dict(ranger=ranger, miner=miner, sailor=sailor, botanist=botanist)


def flat(pixels, slim, back=False):
    """The model's front, or back, laid flat: 16 wide, 32 tall."""
    arm = 3 if slim else 4
    sheet = np.zeros((32, 16, 4), np.uint8)

    def put(part, at, layer):
        base, over, (w, h, d) = PARTS[part]
        if part.endswith("arm"):
            w = arm
        u, v = base if layer == "base" else over
        fx, fy, fw, fh = faces(u, v, w, h, d)["back" if back else "front"]
        region = pixels[fy:fy + fh, fx:fx + fw]
        if back:
            region = region[:, ::-1]
        x, y = at
        under = sheet[y:y + fh, x:x + fw]
        sheet[y:y + fh, x:x + fw] = np.where(region[..., 3:] > 127, region, under)
    # the player's right on the viewer's left from the front
    for layer in ("base", "over"):
        put("head", (4, 0), layer)
        put("body", (4, 8), layer)
        put("right_arm", (4 - arm, 8), layer)
        put("left_arm", (12, 8), layer)
        put("right_leg", (4, 20), layer)
        put("left_leg", (8, 20), layer)
    return sheet[:, ::-1] if back else sheet


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    contact = Image.new("RGBA", (len(DRAW) * 120 + 8, 76), (107, 107, 107, 255))
    for i, (name, draw) in enumerate(DRAW.items()):
        skin = draw()
        skin.save(name)
        contact.alpha_composite(Image.fromarray(skin.pixels, "RGBA"), (8 + i * 120, 6))
        contact.alpha_composite(Image.fromarray(flat(skin.pixels, skin.slim), "RGBA"), (76 + i * 120, 6))
        contact.alpha_composite(Image.fromarray(flat(skin.pixels, skin.slim, back=True), "RGBA"), (94 + i * 120, 6))
    contact.resize((contact.width * 4, contact.height * 4), Image.NEAREST).save(OUT / "skins_contact.png")
    print("drew", len(DRAW), "skins; check", OUT / "skins_contact.png")
