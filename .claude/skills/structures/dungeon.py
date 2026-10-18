"""Writes assets/structures/dungeon.toml: a small vault of mossy cobblestone deep underground.

A room nine blocks square and four high, its walls and floor mossy and plain cobblestone, a
ring of stone brick under its ceiling, pillars of cobblestone wall in its corners, two
chests against its walls, a lantern and a torch or two. A `spawner` marker in its middle keeps the
place for a spawner (M3.3). Short passages lead out of two sides into the rock, so caves
passing near meet them.

    python3 .claude/skills/structures/dungeon.py
"""

from build import AIR, Build, slab

SIZE, LAYERS = 13, 6
R0, R1 = 2, 10
MOSSY = "mossy_cobblestone*3|cobblestone*2"

b = Build(SIZE, LAYERS, SIZE, floor=0)
b.hollow((R0, 0, R0), (R1, LAYERS - 1, R1), MOSSY)
b.fill((R0 + 1, 0, R0 + 1), (R1 - 1, 0, R1 - 1), "mossy_cobblestone*3|cobblestone*2|gravel")
b.walls((R0, 4, R0), (R1, 4, R1), "stone_bricks|mossy_stone_bricks|cracked_stone_bricks")
b.corners((R0 + 1, 1, R0 + 1), (R1 - 1, 4, R1 - 1), "cobblestone_wall")
mid = SIZE // 2
b.mark("spawner", mid, 1, mid)
b.mark("loot", mid, 1, R0 + 1)
b.mark("loot", R1 - 1, 1, mid + 1)
b.set(mid - 1, 1, R0 + 1, slab("cobblestone"))
b.set(R0 + 1, 3, mid, "lantern[hanging=true]")
b.set(R0 + 1, 4, mid, "mossy_cobblestone")
# and two torches burnt down by the pillars
b.set(R0 + 2, 1, R1 - 1, "torch")
b.set(R1 - 1, 1, R0 + 2, "torch|air")
# passages out to the rock, west and south, with rubble in them
b.fill((0, 1, mid), (R0, 2, mid), AIR)
b.fill((mid, 1, R1), (mid, 2, SIZE - 1), AIR)
b.set(1, 1, mid, "gravel|air*2")
b.set(mid, 1, SIZE - 1, "cobblestone|air*2")

b.write(
    "dungeon",
    {
        "cell": 256,
        "chance": 0.5,
        "biomes": ["forest", "base", "foothills", "taiga", "savanna", "jungle", "desert", "glacier"],
        "min_above_sea": 2,
        "max_rise": 48,
        "hilltop": False,
        "depth": [40, 120],
    },
    foundation="cobblestone",
    about="A small vault of mossy cobblestone deep underground.",
    script="dungeon.py",
)
