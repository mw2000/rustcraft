"""Helpers for writing village pieces (the `structures` skill): the parts the three village
styles share, from timber frames and windows with shutters to steep roofs and chimneys.

Every piece keeps the contract the village layout codes against (SKILL.md, "Village pieces"):
its front faces south (+z, the last row), layer `F - 1` is the ground's surface, which the piece
lays itself, its last row is an apron level with the street, and its `entrance` marker stands
in that row at the floor, outside the way in.

    from village import Piece, F
    b = Piece(7, 14, 8)
    b.ground("grass")
    b.write_piece("timber_cottage", "house", ["timber"], "cobblestone", "A cottage.", "village_timber.py")
"""

from build import AIR, Build, slab, stairs

# Every piece's floor: layer 0 is the ground's surface, layer 1 the first air over it.
F = 1

# Flowers for boxes and borders, now and then none.
FLOWERS = "poppy*3|dandelion*3|cornflower*2|air*2"
# Grass and flowers growing along a wall or a fence.
VERGE = "short_grass*4|tall_grass*2|poppy|dandelion|air*6"


class Piece(Build):
    """A village piece: a `Build` with its floor at `F`, and helpers for the parts villages share."""

    def __init__(self, sx, sy, sz):
        super().__init__(sx, sy, sz, floor=F)

    @property
    def front(self):
        """The apron row, the street's side."""
        return self.size[2] - 1

    def ground(self, block, a=None, b=None):
        """Lays the ground's surface, layer `F - 1`, over the whole footprint or the box from `a`
        to `b` (x, z)."""
        sx, _, sz = self.size
        (x0, z0), (x1, z1) = a or (0, 0), b or (sx - 1, sz - 1)
        self.fill((x0, F - 1, z0), (x1, F - 1, z1), block)

    def verge(self, block=VERGE, where=None):
        """Grass and flowers on the ground wherever nothing stands, so a yard isn't bare: every
        cell of the floor layer still unset over grass, for which `where(x, z)` holds if given."""
        sx, _, sz = self.size
        for x in range(sx):
            for z in range(sz):
                if self.get(x, F, z) is None and self.get(x, F - 1, z) == "grass":
                    if where is None or where(x, z):
                        self.set(x, F, z, block)

    def frame(self, x0, x1, z0, z1, y0, y1, infill, xs=(), zs=(), post="log", plate=True):
        """Timber-framed walls round the box from `x0`, `z0` to `x1`, `z1`, layers `y0` to `y1`:
        `infill` panels between upright `post`s at the corners and at `xs` along the north and
        south walls and `zs` along the east and west, and a plate of logs laid along each wall
        at `y1` if `plate`."""
        for y in range(y0, y1 + 1):
            self.walls((x0, y, z0), (x1, y, z1), infill)
        for y in range(y0, y1 + 1):
            for x in (x0, x1, *xs):
                self.set(x, y, z0, post)
                self.set(x, y, z1, post)
            for z in (z0, z1, *zs):
                self.set(x0, y, z, post)
                self.set(x1, y, z, post)
        if plate:
            self.beam_ring(x0, x1, z0, z1, y1)

    def beam_ring(self, x0, x1, z0, z1, y, corner="log"):
        """Logs laid along the four walls at layer `y`, upright at the corners."""
        for x in range(x0 + 1, x1):
            self.set(x, y, z0, "log[axis=x]")
            self.set(x, y, z1, "log[axis=x]")
        for z in range(z0 + 1, z1):
            self.set(x0, y, z, "log[axis=z]")
            self.set(x1, y, z, "log[axis=z]")
        for x in (x0, x1):
            for z in (z0, z1):
                self.set(x, y, z, corner)

    def window(self, x, y, z, out, high=1, shutters=None, box=None, glass="glass_pane"):
        """A window of `glass` in the wall at `x`, `z`, `high` blocks tall from layer `y`, looking
        out `out`, with open `shutters` (trapdoors, or a choice with them) against the wall either
        side of it and a `box` of flowers under its sill if given (the block it stands on)."""
        dx, dz = {"north": (0, -1), "south": (0, 1), "east": (1, 0), "west": (-1, 0)}[out]
        for h in range(high):
            self.set(x, y + h, z, glass)
            if shutters:
                for s in (-1, 1):
                    self.set(x + s * dz + dx, y + h, z + s * dx + dz, shutters.format(facing=out))
        if box:
            self.set(x + dx, y - 1, z + dz, box.format(facing=out))
            self.set(x + dx, y, z + dz, FLOWERS)

    def hang(self, x, y, z):
        """A lantern hung under the block over layer `y`."""
        self.set(x, y, z, "lantern[hanging=true]")

    def king_post(self, x, z, y0, y1, infill, post="log"):
        """An upright `post` up the middle of a gable, from layer `y0` to `y1`, wherever the
        gable's `infill` is (never into the roof over it)."""
        for y in range(y0, y1 + 1):
            if self.get(x, y, z) == infill:
                self.set(x, y, z, post)

    def bracket_lamp(self, x, y, z, out, material="dark_planks"):
        """A lantern at layer `y` hung from a bracket on the wall behind it, which faces `out`:
        an upturned stair of `material` jutting out of the wall over it."""
        back = {"north": "south", "south": "north", "east": "west", "west": "east"}[out]
        self.set(x, y + 1, z, stairs(material, back, top=True))
        self.hang(x, y, z)

    def roof(self, x0, x1, z0, z1, y, stair, full, cap, along="x", overhang=1, ends=0,
             steep=False, gable=None, ridge=None, into=None):
        """A pitched roof over the walls from `x0`, `z0` to `x1`, `z1`, its eaves at layer `y`
        hanging `overhang` past the side walls and `ends` past the end walls, its ridge running
        `along` x or z. `stair(facing)` names a stair climbing that way; a `steep` roof climbs two
        layers to each row in, a stair over a `full` block, and a shallow one a layer, of stairs
        alone. `cap` finishes a ridge one block wide, `gable` fills the end walls' triangles, and
        `ridge` (a block) is laid along the top if given. Returns the layer of the top. With
        `into`, a pair of dicts, its cells go there instead, its surface and its gables apart, for
        `roofs` to join it to others."""
        if along == "x":
            lo, hi, a0, a1, up = z0 - overhang, z1 + overhang, x0, x1, ("south", "north")
        else:
            lo, hi, a0, a1, up = x0 - overhang, x1 + overhang, z0, z1, ("east", "west")

        def put(a, layer, across, block, part=0):
            at = (a, layer, across) if along == "x" else (across, layer, a)
            if into is None:
                self.set(*at, block)
            elif self.inside(*at):
                into[part][at] = block

        layer, top = y, y
        while True:
            k = layer - y
            inset = (k + 1) // 2 if steep else k
            near, far = lo + inset, hi - inset
            if near > far:
                break
            lip = not steep or k % 2 == 0
            for a in range(a0 - ends, a1 + ends + 1):
                if near == far:
                    put(a, layer, near, cap if lip else full)
                elif lip:
                    put(a, layer, near, stair(up[0]))
                    put(a, layer, far, stair(up[1]))
                else:
                    put(a, layer, near, full)
                    put(a, layer, far, full)
            if gable:
                for across in range(near + 1, far):
                    for a in (a0, a1):
                        put(a, layer, across, gable, 1)
            top = layer
            layer += 1
        if ridge:
            k = top - y
            inset = (k + 1) // 2 if steep else k
            for across in {lo + inset, hi - inset}:
                for a in range(a0 - ends, a1 + ends + 1):
                    put(a, top + 1, across, ridge)
            return top + 1
        return top

    def spire(self, x0, x1, z0, z1, y, stair, full, cap, overhang=1, steep=True):
        """A pyramid roof over the walls from `x0`, `z0` to `x1`, `z1`, its eaves at layer `y`,
        climbing two layers to each ring in if `steep` (a stair over a `full` block), finished
        with `cap` at its point or ridge. Returns the layer of the top."""
        x0, x1, z0, z1 = x0 - overhang, x1 + overhang, z0 - overhang, z1 + overhang
        layer, k = y, 0
        while x0 <= x1 and z0 <= z1:
            lip = not steep or k % 2 == 0
            if x0 == x1 or z0 == z1:
                self.fill((x0, layer, z0), (x1, layer, z1), cap if lip else full)
                if not lip:
                    self.fill((x0, layer + 1, z0), (x1, layer + 1, z1), cap)
                    return layer + 1
                return layer
            for x in range(x0, x1 + 1):
                self.set(x, layer, z0, stair("south") if lip else full)
                self.set(x, layer, z1, stair("north") if lip else full)
            for z in range(z0 + 1, z1):
                self.set(x0, layer, z, stair("east") if lip else full)
                self.set(x1, layer, z, stair("west") if lip else full)
            if not steep or k % 2 == 0:
                x0, x1, z0, z1 = x0 + 1, x1 - 1, z0 + 1, z1 - 1
            layer += 1
            k += 1
        return layer - 1

    def roofs(self, *specs):
        """Roofs meeting in valleys, as over an L-shaped house: each spec a dict of `roof`'s
        arguments. Where two cover the same column the higher of them there is kept, and the
        gables stay as walls under them. Returns the layer of the highest top."""
        parts, tops = [], []
        for spec in specs:
            into = ({}, {})
            tops.append(self.roof(**spec, into=into))
            parts.append(into)
        height = {}
        for i, (surface, _) in enumerate(parts):
            for (x, y, z) in surface:
                if y >= height.get((x, z), (-1, 0))[0]:
                    height[(x, z)] = (y, i)
        placed = set()
        for i, (surface, _) in enumerate(parts):
            for (x, y, z), block in surface.items():
                if height[(x, z)][1] == i:
                    self.set(x, y, z, block)
                    placed.add((x, y, z))
        # the gables stay as walls, under whichever roof covers them
        for _, gables in parts:
            for (x, y, z), block in gables.items():
                over = height.get((x, z))
                if (x, y, z) not in placed and (over is None or y < over[0]):
                    self.set(x, y, z, block)
        return max(tops)

    def stack(self, x, z, y0, y1, block, breast=(), shoulder=None, pot=None):
        """A chimney at `x`, `z` from layer `y0` up to `y1`, its breast widened over the cells
        `breast` ((x, z) pairs) up to layer `shoulder`, and a `pot` on top if given."""
        self.fill((x, y0, z), (x, y1, z), block)
        if shoulder:
            for bx, bz in breast:
                self.fill((bx, y0, bz), (bx, shoulder, bz), block)
        if pot:
            self.set(x, y1 + 1, z, pot)

    def chimney(self, x, z, y0, y1, block, pot=None):
        """A chimney stack at `x`, `z` from layer `y0` to `y1`, with a `pot` on top if given."""
        self.fill((x, y0, z), (x, y1, z), block)
        if pot:
            self.set(x, y1 + 1, z, pot)

    def step_up(self, x, z, material, facing="north"):
        """A stair at the floor in front of a door, climbing `facing` into it."""
        self.set(x, F, z, stairs(material, facing))

    def table(self, x, z, y=F + 1, top="dark_planks_slab[half=top]", leg="dark_planks_fence"):
        """A table: a fence post under a slab, standing on layer `y`, by default on a floor
        raised a block on a plinth."""
        self.set(x, y, z, leg)
        self.set(x, y + 1, z, top)


def thatch_or_planks(facing):
    """A roof's stair, of thatch or dark planks for the whole roof."""
    return f"thatch_stairs[facing={facing}]/dark_planks_stairs[facing={facing}]"


__all__ = ["AIR", "F", "FLOWERS", "Piece", "VERGE", "slab", "stairs", "thatch_or_planks"]
