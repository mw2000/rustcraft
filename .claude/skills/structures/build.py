"""A small library for writing structure templates by script (the `structures` skill).

A `Build` is a box of cells, x across (east), y up (layer 0 the lowest) and z down the page
(south), as the template format has them. Cells hold block text as the palette writes it: a
block with its state (`stone_brick_stairs[facing=east,half=top]`), a choice made for each cell
(`stone_bricks*3|mossy_stone_bricks`) or once for the whole structure (`planks/dark_planks`),
`AIR` to clear the world to air, or None to leave it as it is. Palette keys are given out as
`write` writes the file, so scripts name blocks rather than juggle letters.

    from build import Build, AIR
    b = Build(9, 12, 7, floor=1)
    b.fill((0, 0, 0), (8, 0, 6), "cobblestone")
    b.write("hut", placement, foundation="cobblestone", about="A hut.")
"""

from pathlib import Path

AIR = "air"
ASSETS = Path(__file__).resolve().parents[3] / "assets" / "structures"

# The way each facing turns a quarter clockwise seen from above, and its opposite.
CLOCKWISE = {"north": "east", "east": "south", "south": "west", "west": "north"}
OPPOSITE = {"north": "south", "south": "north", "east": "west", "west": "east"}
# One step that way, across x and z.
STEP = {"north": (0, -1), "south": (0, 1), "east": (1, 0), "west": (-1, 0)}


def stairs(material, facing, top=False):
    """A stair of `material` (`sandstone`, `dark_planks`, ...) climbing towards `facing`,
    turned over if `top`."""
    name = {"stone_bricks": "stone_brick", "bricks": "brick"}.get(material, material)
    return f"{name}_stairs[facing={facing}{',half=top' if top else ''}]"


def slab(material, top=False):
    name = {"stone_bricks": "stone_brick", "bricks": "brick"}.get(material, material)
    return f"{name}_slab{'[half=top]' if top else ''}"


class Build:
    def __init__(self, sx, sy, sz, floor=0):
        self.size = (sx, sy, sz)
        self.floor = floor
        self.cells = {}
        self.markers = {}

    def inside(self, x, y, z):
        sx, sy, sz = self.size
        return 0 <= x < sx and 0 <= y < sy and 0 <= z < sz

    def set(self, x, y, z, block):
        """Sets one cell; one outside the box is left out, so shapes may run over its edge."""
        if self.inside(x, y, z):
            self.cells[(x, y, z)] = block

    def get(self, x, y, z):
        return self.cells.get((x, y, z))

    def fill(self, a, b, block, where=None):
        """Every cell of the box from corner `a` to `b`, both in it, for which `where(x, y, z)`
        holds if it's given."""
        (x0, y0, z0), (x1, y1, z1) = a, b
        for x in range(min(x0, x1), max(x0, x1) + 1):
            for y in range(min(y0, y1), max(y0, y1) + 1):
                for z in range(min(z0, z1), max(z0, z1) + 1):
                    if where is None or where(x, y, z):
                        self.set(x, y, z, block)

    def walls(self, a, b, block):
        """The four walls of the box from `a` to `b`, without its floor or roof."""
        (x0, _, z0), (x1, _, z1) = a, b
        self.fill(a, b, block, lambda x, y, z: x in (x0, x1) or z in (z0, z1))

    def corners(self, a, b, block):
        """The four corner columns of the box from `a` to `b`."""
        (x0, _, z0), (x1, _, z1) = a, b
        self.fill(a, b, block, lambda x, y, z: x in (x0, x1) and z in (z0, z1))

    def hollow(self, a, b, block):
        """A room: the walls, floor and roof of the box from `a` to `b`, cleared inside."""
        self.fill(a, b, block)
        (x0, y0, z0), (x1, y1, z1) = a, b
        self.fill((x0 + 1, y0 + 1, z0 + 1), (x1 - 1, y1 - 1, z1 - 1), AIR)

    def gable(self, x0, x1, z0, z1, y, material, along="x", overhang=1, gable=None):
        """A pitched roof over the walls from `x0`, `z0` to `x1`, `z1`, its eaves at layer `y`
        hanging `overhang` past them, its ridge running `along` x or z, of `material` stairs
        with a slab along the ridge where the span is odd, and its gable ends over the end walls
        filled with `gable` if given. Returns the layer of its ridge."""
        if along == "x":
            lo, hi, a0, a1, up = z0, z1, x0, x1, ("south", "north")
        else:
            lo, hi, a0, a1, up = x0, x1, z0, z1, ("east", "west")
        lo, hi = lo - overhang, hi + overhang

        def put(a, layer, across, block):
            self.set(*((a, layer, across) if along == "x" else (across, layer, a)), block)

        step = 0
        while lo + step <= hi - step:
            layer = y + step
            near, far = lo + step, hi - step
            for a in range(a0 - overhang, a1 + overhang + 1):
                if near == far:
                    put(a, layer, near, slab(material))
                else:
                    put(a, layer, near, stairs(material, up[0]))
                    put(a, layer, far, stairs(material, up[1]))
            if gable:
                for across in range(near + 1, far):
                    for a in (a0, a1):
                        put(a, layer, across, gable)
            step += 1
        return y + step - 1

    def hip(self, x0, x1, z0, z1, y, material, overhang=1, cap=None):
        """A hipped roof, sloping down to every side, over the walls from `x0`, `z0` to `x1`,
        `z1` with its eaves at layer `y`, of `material` stairs, finished with `cap` (a slab by
        default) where it narrows to a line or a point. Returns the layer of its top."""
        x0, x1, z0, z1 = x0 - overhang, x1 + overhang, z0 - overhang, z1 + overhang
        layer = y
        while x0 <= x1 and z0 <= z1:
            if x0 == x1 or z0 == z1:
                self.fill((x0, layer, z0), (x1, layer, z1), cap or slab(material))
                return layer
            for x in range(x0, x1 + 1):
                self.set(x, layer, z0, stairs(material, "south"))
                self.set(x, layer, z1, stairs(material, "north"))
            for z in range(z0 + 1, z1):
                self.set(x0, layer, z, stairs(material, "east"))
                self.set(x1, layer, z, stairs(material, "west"))
            x0, x1, z0, z1 = x0 + 1, x1 - 1, z0 + 1, z1 - 1
            layer += 1
        return layer - 1

    def door(self, x, y, z, facing, hinge="left"):
        """A door two blocks tall standing on layer `y`, facing out of the room `facing`."""
        self.set(x, y, z, f"door[facing={facing},hinge={hinge}]")
        self.set(x, y + 1, z, f"door[facing={facing},half=top,hinge={hinge}]")

    def ladder(self, x, z, y0, y1, facing):
        """A ladder up from layer `y0` to `y1` on the wall behind it, facing `facing`, out of
        that wall."""
        for y in range(y0, y1 + 1):
            self.set(x, y, z, f"ladder[facing={facing}]")

    def mark(self, kind, x, y, z):
        self.markers.setdefault(kind, []).append((x, y, z))

    def write(self, name, placement, foundation, about, script):
        """Writes the template to `assets/structures/<name>.toml`, with `placement` as a dict of
        its fields in order, and `about` and the script that wrote it in its first comment."""
        table = ["[placement]"] + [f"{k} = {_value(v)}" for k, v in placement.items()]
        self._write(ASSETS / f"{name}.toml", name, table, foundation, about, script)

    def write_piece(self, name, role, styles, foundation, about, script):
        """Writes the template as a village piece, `assets/structures/village/<name>.toml`: a
        `[piece]` table of its `role` (house, hall, smithy, farm, pen, tower, well or lamp) and
        the village `styles` that build it in place of a placement, as villages place their
        pieces along their streets themselves."""
        table = ["[piece]", f"role = {_value(role)}", f"styles = {_value(list(styles))}"]
        # a piece is as tall as what it builds, so the layout knows its height
        top = max(y for (_, y, _), block in self.cells.items() if block is not None)
        self.size = (self.size[0], top + 1, self.size[2])
        path = ASSETS / "village" / f"{name}.toml"
        path.parent.mkdir(exist_ok=True)
        self._write(path, name, table, foundation, about, script)

    def _write(self, path, name, table, foundation, about, script):
        sx, sy, sz = self.size
        palette = {}
        for block in sorted({b for b in self.cells.values() if b not in (None, AIR)}, key=str):
            spare = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
            first = block.split("[")[0].split("*")[0]
            # keys in the order a template saved in the game would give them, by name
            for key in [c for c in first if c.isalnum()] + list(spare):
                if key not in palette.values():
                    palette[block] = key
                    break
            else:
                raise ValueError("more blocks than keys")

        out = [f"# {about} Written by .claude/skills/structures/{script}.", f'name = "{name}"', ""]
        out += table
        out += ["", "[template]", f"size = [{sx}, {sy}, {sz}]", f"floor = {self.floor}"]
        out.append(f'foundation = "{foundation}"')
        keys = sorted(palette.items(), key=lambda kv: kv[1])
        out.append("palette = { " + ", ".join(f'{k} = "{b}"' for b, k in keys) + " }")
        if self.markers:
            kinds = ", ".join(
                f"{kind} = [" + ", ".join(f"[{x}, {y}, {z}]" for x, y, z in sorted(at)) + "]"
                for kind, at in sorted(self.markers.items())
            )
            out.append("markers = { " + kinds + " }")
        out.append("# bottom to top, each layer's rows running +z down the page and +x across")
        out.append("layers = [")
        for y in range(sy):
            out += [f"  # {y}", "  '''"]
            for z in range(sz):
                row = ""
                for x in range(sx):
                    block = self.cells.get((x, y, z))
                    row += "." if block is None else "-" if block == AIR else palette[block]
                out.append(row)
            out.append("''',")
        out.append("]")
        path.write_text("\n".join(out) + "\n")
        print(f"wrote {path}")


def _value(v):
    """A value as TOML writes it."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, str):
        return f'"{v}"'
    if isinstance(v, (list, tuple)):
        return "[" + ", ".join(_value(i) for i in v) + "]"
    return str(v)
