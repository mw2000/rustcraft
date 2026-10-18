use std::collections::BTreeMap;
use std::fmt::Write;

use bevy::math::{I64Vec2, I64Vec3, Vec3Swizzles};
use serde::{Deserialize, Serialize};

use crate::block::{Block, BlockType};
use crate::chunks::generate::trees::hash;

// What a template's layers write for a cell that leaves the world as it is, and for one that
// clears it to air, carving interiors out of the ground.
const LEAVE: char = '.';
const CLEAR: char = '-';
// What separates a palette entry's choices made afresh for each cell, and those made once for
// the whole structure, and what comes before a choice's weight.
const EACH: char = '|';
const WHOLE: char = '/';
const WEIGHT: char = '*';
/// The kind of marker where a structure has a chest of loot: generation puts a chest there, and
/// the server fills it from the loot table named after the structure the first time it's opened
/// (`loot`, §5.6).
pub const LOOT: &str = "loot";

/// How far above its floor any structure reaches, and how far under it the layers sunk into
/// the ground go, so chunks and LOD nodes know how far up and down to look for them.
pub const STRUCTURE_HEIGHT: i64 = 27;
pub const STRUCTURE_DEPTH: i64 = 6;

/// A structure type, as its file in `assets/structures/` declares it: where it's placed, and the
/// blocks it's built of.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    pub name: String,
    pub placement: Placement,
    pub template: Template,
}

/// Where a structure type stands: on a grid of square cells `cell` blocks across, with one in a
/// cell `chance` of the time where the ground suits it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub cell: i64,
    pub chance: f64,
    /// The biomes it stands in, by name.
    pub biomes: Vec<String>,
    /// How far above the sea its floor must be.
    pub min_above_sea: i64,
    /// The most the ground under it may rise and fall across its footprint.
    pub max_rise: i64,
    /// Whether it climbs from its spot to the top of the hill it's on, or stands where it lands.
    pub hilltop: bool,
    /// For a structure underground, the least and most its floor is under the lowest ground
    /// over it, see `structures::place`. Underground, `min_above_sea` is how far over the sea
    /// that ground must be, and `max_rise` how much it may rise and fall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<[i64; 2]>,
}

/// A structure's blocks: a box of cells, each a block, air cleared out of the ground, or left as
/// the world has it.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "TemplateFile")]
pub struct Template {
    /// Across x, up and across z.
    pub size: I64Vec3,
    /// The layer standing at ground level: those below it are sunk into the ground.
    pub floor: i64,
    /// What fills the ground up to its lowest layer where the ground falls away under it.
    pub foundation: BlockType,
    // What each cell can be, the first air.
    palette: Vec<Choice>,
    // x fastest, then z, then y, each an index into `palette`; None leaves the world as it is
    cells: Vec<Option<u16>>,
    /// Points of interest by kind, such as `loot` for a chest (M2.3).
    pub markers: Vec<(String, I64Vec3)>,
}

// Templates are alike when every cell is, whatever order their palettes are in.
impl PartialEq for Template {
    fn eq(&self, other: &Self) -> bool {
        let choices = |t: &'_ Self| -> Vec<Option<Choice>> {
            t.cells
                .iter()
                .map(|cell| cell.map(|i| t.palette[i as usize].clone()))
                .collect()
        };
        (self.size, self.floor, self.foundation, &self.markers)
            == (other.size, other.floor, other.foundation, &other.markers)
            && choices(self) == choices(other)
    }
}

/// What a palette key stands for: a block, or a choice of blocks by weight, which a structure
/// makes from its seed (`Placed::variant`) afresh for each cell (`a*3|b`, weathering) or once for
/// the whole of it (`a/b`, a variant). Every choice made for the whole structure draws the same
/// number, so keys whose choices have the same weights in the same order choose alike, as a
/// roof's stairs and slabs both of dark planks or both of planks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    options: Vec<(Block, u32)>,
    whole: bool,
}

impl Choice {
    fn one(block: Block) -> Self {
        Self {
            options: vec![(block, 1)],
            whole: false,
        }
    }

    // The block chosen in a structure of seed `seed` for its cell `at`, as written unturned.
    fn pick(&self, seed: u64, at: I64Vec3) -> Block {
        let [(block, _)] = self.options[..] else {
            let total: u32 = self.options.iter().map(|&(_, weight)| weight).sum();
            let draw = if self.whole {
                hash(seed, 0, 0)
            } else {
                hash(hash(seed, at.x, at.y), at.z, 1)
            };
            let mut left = (draw % u64::from(total)) as u32;
            for &(block, weight) in &self.options {
                if left < weight {
                    return block;
                }
                left -= weight;
            }
            unreachable!("the draw is under the weights' total")
        };
        block
    }
}

impl std::str::FromStr for Choice {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let whole = text.contains(WHOLE);
        if whole && text.contains(EACH) {
            return Err(format!(
                "\"{text}\" chooses both for each cell ({EACH}) and once ({WHOLE})"
            ));
        }
        let options = text
            .split([EACH, WHOLE])
            .map(|option| {
                let (block, weight) = match option.split_once(WEIGHT) {
                    Some((block, weight)) => (
                        block,
                        weight
                            .parse()
                            .ok()
                            .filter(|&w| w > 0)
                            .ok_or(format!("\"{weight}\" isn't a weight"))?,
                    ),
                    None => (option, 1),
                };
                Ok((block.parse()?, weight))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { options, whole })
    }
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, (block, weight)) in self.options.iter().enumerate() {
            if i > 0 {
                write!(f, "{}", if self.whole { WHOLE } else { EACH })?;
            }
            write!(f, "{block}")?;
            if *weight != 1 {
                write!(f, "{WEIGHT}{weight}")?;
            }
        }
        Ok(())
    }
}

// The file as written, before it's checked and its layers read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    size: [i64; 3],
    floor: i64,
    foundation: String,
    // each key's block with its state, as `Block`'s text has it: `stone_brick_stairs[facing=east]`,
    // or a choice of them, see `Choice`
    palette: BTreeMap<char, String>,
    #[serde(default)]
    markers: BTreeMap<String, Vec<[i64; 3]>>,
    layers: Vec<String>,
}

impl TryFrom<TemplateFile> for Template {
    type Error = String;

    fn try_from(file: TemplateFile) -> Result<Self, String> {
        let size = I64Vec3::from_array(file.size);
        let mut palette = vec![Choice::one(Block::AIR)];
        let mut keys = BTreeMap::new();
        for (&key, text) in &file.palette {
            if key == LEAVE || key == CLEAR {
                return Err(format!("'{key}' is kept for leaving or clearing a cell"));
            }
            keys.insert(key, palette.len() as u16);
            palette.push(text.parse::<Choice>()?);
        }
        if file.layers.len() as i64 != size.y {
            return Err(format!("{} layers for a size of {size}", file.layers.len()));
        }
        let mut cells = Vec::with_capacity((size.x * size.y * size.z) as usize);
        for (y, layer) in file.layers.iter().enumerate() {
            let rows: Vec<&str> = layer.lines().filter(|row| !row.is_empty()).collect();
            if rows.len() as i64 != size.z
                || rows.iter().any(|r| r.chars().count() as i64 != size.x)
            {
                return Err(format!("layer {y} isn't {} by {}", size.x, size.z));
            }
            for key in rows.iter().flat_map(|row| row.chars()) {
                cells.push(match key {
                    LEAVE => None,
                    CLEAR => Some(0),
                    key => Some(
                        *keys
                            .get(&key)
                            .ok_or(format!("'{key}' isn't in the palette"))?,
                    ),
                });
            }
        }
        if !(0..size.y).contains(&file.floor)
            || file.floor > STRUCTURE_DEPTH
            || size.y - file.floor > STRUCTURE_HEIGHT
        {
            return Err(format!("floor {} doesn't fit the size {size}", file.floor));
        }
        let mut markers = Vec::new();
        for (kind, points) in file.markers {
            for point in points.into_iter().map(I64Vec3::from_array) {
                if point.cmplt(I64Vec3::ZERO).any() || point.cmpge(size).any() {
                    return Err(format!("{kind} marker {point} is outside the template"));
                }
                markers.push((kind.clone(), point));
            }
        }
        Ok(Self {
            size,
            floor: file.floor,
            foundation: BlockType::from_name(&file.foundation)
                .ok_or(format!("no block {}", file.foundation))?,
            palette,
            cells,
            markers,
        }
        .with_chests())
    }
}

impl Default for Placement {
    /// Somewhere to start from for a structure saved in the game, to be edited afterwards: as
    /// often as the watchtower, in forest on any ground fairly flat.
    fn default() -> Self {
        Self {
            cell: 640,
            chance: 0.5,
            biomes: vec!["forest".into()],
            min_above_sea: 2,
            max_rise: 4,
            hilltop: false,
            depth: None,
        }
    }
}

impl Structure {
    /// The structure as a file in `assets/structures/` has it, its layers as text grids, which
    /// reads back as the same structure if it's one that loads. Blocks are keyed by the first
    /// letter of their names that's free, so they read as what they are.
    pub fn to_toml(&self) -> String {
        let template = &self.template;
        let size = template.size;
        // a loot marker's chest is the marker's, so its cell is left for loading to put it in
        let mut cells = template.cells.clone();
        for (_, at) in template.markers.iter().filter(|(kind, _)| kind == LOOT) {
            cells[template.index(*at)] = None;
        }
        let mut palette: Vec<(char, &Choice)> = Vec::new();
        for choice in cells
            .iter()
            .flatten()
            .map(|&i| &template.palette[i as usize])
        {
            if *choice == Choice::one(Block::AIR) || palette.iter().any(|(_, c)| *c == choice) {
                continue;
            }
            let spare = ('a'..='z').chain('A'..='Z').chain('0'..='9');
            let key = choice.options[0]
                .0
                .kind
                .name()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .chain(spare)
                .find(|key| !palette.iter().any(|(k, _)| k == key))
                .expect("fewer blocks than keys");
            palette.push((key, choice));
        }
        palette.sort_unstable_by_key(|&(key, _)| key);
        let key_of = |cell: Option<u16>| match cell.map(|i| &template.palette[i as usize]) {
            None => LEAVE,
            Some(choice) if *choice == Choice::one(Block::AIR) => CLEAR,
            Some(choice) => palette.iter().find(|(_, c)| *c == choice).unwrap().0,
        };
        let entries = |items: Vec<String>| format!("{{ {} }}", items.join(", "));

        let placement = toml::to_string(&self.placement).expect("a placement is plain data");
        let mut out = format!(
            "name = {:?}\n\n[placement]\n{placement}\n[template]\n",
            self.name
        );
        let _ = writeln!(out, "size = [{}, {}, {}]", size.x, size.y, size.z);
        let _ = writeln!(out, "floor = {}", template.floor);
        let _ = writeln!(out, "foundation = {:?}", template.foundation.name());
        let keys = palette
            .iter()
            .map(|(key, choice)| format!("{key} = {:?}", choice.to_string()));
        let _ = writeln!(out, "palette = {}", entries(keys.collect()));
        let mut markers: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (kind, at) in &template.markers {
            let point = format!("[{}, {}, {}]", at.x, at.y, at.z);
            markers.entry(kind).or_default().push(point);
        }
        if !markers.is_empty() {
            let kinds = markers
                .iter()
                .map(|(kind, points)| format!("{kind} = [{}]", points.join(", ")));
            let _ = writeln!(out, "markers = {}", entries(kinds.collect()));
        }
        out += "# bottom to top, each layer's rows running +z down the page and +x across\n";
        out += "layers = [\n";
        for (y, layer) in cells.chunks((size.x * size.z) as usize).enumerate() {
            let _ = writeln!(out, "  # {y}\n  '''");
            for row in layer.chunks(size.x as usize) {
                out.extend(row.iter().map(|&cell| key_of(cell)));
                out.push('\n');
            }
            out += "''',\n";
        }
        out + "]\n"
    }
}

impl Template {
    /// A template of `size` from its cells, x fastest, then z, then y, as `cell` reads them. It's
    /// only checked once it's written and read back, see `Structure::to_toml`.
    pub fn new(
        size: I64Vec3,
        floor: i64,
        foundation: BlockType,
        cells: Vec<Option<Block>>,
        markers: Vec<(String, I64Vec3)>,
    ) -> Self {
        let mut palette = vec![Choice::one(Block::AIR)];
        let cells = cells
            .into_iter()
            .map(|cell| {
                let choice = Choice::one(cell?);
                let index = palette
                    .iter()
                    .position(|c| *c == choice)
                    .unwrap_or_else(|| {
                        palette.push(choice);
                        palette.len() - 1
                    });
                Some(index as u16)
            })
            .collect();
        Self {
            size,
            floor,
            foundation,
            palette,
            cells,
            markers,
        }
        .with_chests()
    }

    // Each loot marker's cell made a chest.
    fn with_chests(mut self) -> Self {
        let chest = Choice::one(BlockType::Chest.into());
        let index = self.palette.len() as u16;
        let mut used = false;
        for (_, at) in self.markers.iter().filter(|(kind, _)| kind == LOOT) {
            let cell = self.index(*at);
            self.cells[cell] = Some(index);
            used = true;
        }
        if used {
            self.palette.push(chest);
        }
        self
    }

    fn index(&self, at: I64Vec3) -> usize {
        ((at.y * self.size.z + at.z) * self.size.x + at.x) as usize
    }

    /// Its footprint turned `rotation` quarter turns, across x and z.
    pub fn footprint(&self, rotation: u8) -> I64Vec2 {
        if rotation.is_multiple_of(2) {
            self.size.xz()
        } else {
            self.size.zx()
        }
    }

    /// The cell at `at` in the template turned `rotation` quarter turns about its middle, from
    /// its lowest corner, as a structure of seed `seed` chooses it, its block turned with it:
    /// None to leave the world as it is, air to clear it.
    pub fn cell(&self, rotation: u8, at: I64Vec3, seed: u64) -> Option<Block> {
        let at = self.unturn(rotation, at);
        if at.cmplt(I64Vec3::ZERO).any() || at.cmpge(self.size).any() {
            return None;
        }
        let choice = &self.palette[self.cells[self.index(at)]? as usize];
        Some(choice.pick(seed, at).turned(rotation))
    }

    /// Where the template's own `at` lands once it's turned `rotation` quarter turns.
    pub fn turn(&self, rotation: u8, at: I64Vec3) -> I64Vec3 {
        let (x, z) = (self.size.x - 1, self.size.z - 1);
        match rotation % 4 {
            0 => at,
            1 => I64Vec3::new(z - at.z, at.y, at.x),
            2 => I64Vec3::new(x - at.x, at.y, z - at.z),
            _ => I64Vec3::new(at.z, at.y, x - at.x),
        }
    }

    // `turn` undone: the template's own cell that lands at `at`.
    fn unturn(&self, rotation: u8, at: I64Vec3) -> I64Vec3 {
        let (x, z) = (self.size.x - 1, self.size.z - 1);
        match rotation % 4 {
            0 => at,
            1 => I64Vec3::new(at.z, at.y, z - at.x),
            2 => I64Vec3::new(x - at.x, at.y, z - at.z),
            _ => I64Vec3::new(x - at.z, at.y, at.x),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(layers: &str) -> Result<Template, String> {
        let file: TemplateFile = toml::from_str(&format!(
            "size = [3, 2, 2]\nfloor = 1\nfoundation = \"cobblestone\"\n\
             palette = {{ c = \"cobblestone\", s = \"stone_bricks\" }}\n\
             markers = {{ loot = [[2, 1, 0]] }}\n{layers}"
        ))
        .map_err(|e| e.to_string())?;
        Template::try_from(file)
    }

    #[test]
    fn test_layers_read_as_blocks_cleared_cells_and_cells_left_alone() {
        let t = template("layers = ['''\ncs.\n---\n''', '''\nccc\nc.c\n''']").unwrap();
        assert_eq!(I64Vec3::new(3, 2, 2), t.size);
        assert_eq!(
            Some(BlockType::Cobblestone.into()),
            t.cell(0, I64Vec3::ZERO, 0)
        );
        assert_eq!(
            Some(BlockType::StoneBricks.into()),
            t.cell(0, I64Vec3::new(1, 0, 0), 0)
        );
        assert_eq!(None, t.cell(0, I64Vec3::new(2, 0, 0), 0));
        assert_eq!(Some(Block::AIR), t.cell(0, I64Vec3::new(1, 0, 1), 0));
        assert_eq!(None, t.cell(0, I64Vec3::new(1, 1, 1), 0));
        // outside it, the world is left as it is
        assert_eq!(None, t.cell(0, I64Vec3::new(3, 0, 0), 0));
        assert_eq!(vec![("loot".to_string(), I64Vec3::new(2, 1, 0))], t.markers);
        // and its loot marker is a chest
        assert_eq!(
            Some(BlockType::Chest.into()),
            t.cell(0, I64Vec3::new(2, 1, 0), 0)
        );
    }

    #[test]
    fn test_bad_templates_are_refused() {
        // too few rows, an unknown key, and too few layers
        assert!(template("layers = ['''\ncs.\n''', '''\nccc\nc.c\n''']").is_err());
        assert!(template("layers = ['''\ncx.\n---\n''', '''\nccc\nc.c\n''']").is_err());
        assert!(template("layers = ['''\ncs.\n---\n''']").is_err());
    }

    #[test]
    fn test_a_template_written_out_reads_back_the_same_with_its_blocks_keyed_by_name() {
        let template = template("layers = ['''\ncs.\n---\n''', '''\nccc\nc.c\n''']").unwrap();
        let structure = Structure {
            name: "hut".into(),
            placement: Placement::default(),
            template,
        };
        let text = structure.to_toml();
        assert!(text.contains("palette = { c = \"cobblestone\", s = \"stone_bricks\" }"));
        assert!(text.contains("markers = { loot = [[2, 1, 0]] }"));
        assert!(text.contains("'''\ncs.\n---\n'''"), "{text}");
        assert_eq!(structure, toml::from_str::<Structure>(&text).unwrap());

        // a block whose initial is taken takes the next free letter of its name
        let blocks = [
            BlockType::Stone,
            BlockType::Sand,
            BlockType::Snow,
            BlockType::Water,
        ];
        let cells = blocks.into_iter().map(|b| Some(b.into())).collect();
        let size = I64Vec3::new(4, 1, 1);
        let template = Template::new(size, 0, BlockType::Stone, cells, Vec::new());
        let structure = Structure {
            name: "row".into(),
            placement: Placement::default(),
            template,
        };
        let text = structure.to_toml();
        assert!(text.contains("'''\nsanw\n'''"), "{text}");
        assert_eq!(structure, toml::from_str::<Structure>(&text).unwrap());
    }

    #[test]
    fn test_blocks_keep_their_state_through_the_palette_and_turn_with_the_template() {
        let file: TemplateFile = toml::from_str(
            "size = [2, 1, 1]\nfloor = 0\nfoundation = \"cobblestone\"\n\
             palette = { s = \"stone_brick_stairs[facing=east,half=top]\", l = \"log[axis=x]\" }\n\
             layers = ['''\nsl\n''']",
        )
        .unwrap();
        let t = Template::try_from(file).unwrap();
        let stairs: Block = "stone_brick_stairs[facing=east,half=top]".parse().unwrap();
        assert_eq!(Some(stairs), t.cell(0, I64Vec3::ZERO, 0));
        // a quarter turn takes east to south, and logs along x to along z
        let turned: Block = "stone_brick_stairs[facing=south,half=top]".parse().unwrap();
        assert_eq!(Some(turned), t.cell(1, t.turn(1, I64Vec3::ZERO), 0));
        let log: Block = "log[axis=z]".parse().unwrap();
        assert_eq!(Some(log), t.cell(1, t.turn(1, I64Vec3::X), 0));
        let structure = Structure {
            name: "steps".into(),
            placement: Placement::default(),
            template: t,
        };
        let text = structure.to_toml();
        assert!(
            text.contains("s = \"stone_brick_stairs[facing=east,half=top]\""),
            "{text}"
        );
        assert_eq!(structure, toml::from_str::<Structure>(&text).unwrap());
        // a state the block can't have is refused
        let bad = "size = [1, 1, 1]\nfloor = 0\nfoundation = \"stone\"\n\
                   palette = { s = \"stone[facing=east]\" }\nlayers = ['''\ns\n''']";
        assert!(Template::try_from(toml::from_str::<TemplateFile>(bad).unwrap()).is_err());
    }

    // A floor of weathered stone bricks, sixteen square, with a stair and a slab at its corner
    // each of planks or dark planks.
    fn weathered() -> Template {
        let row = "w".repeat(16);
        let rows = format!("rs{}\n", &row[2..]) + &format!("{row}\n").repeat(15);
        let file: TemplateFile = toml::from_str(&format!(
            "size = [16, 1, 16]\nfloor = 0\nfoundation = \"stone\"\n\
             palette = {{ w = \"stone_bricks*3|mossy_stone_bricks|cracked_stone_bricks\", \
             r = \"planks_stairs[facing=east]/dark_planks_stairs[facing=east]\", \
             s = \"planks_slab/dark_planks_slab\" }}\nlayers = ['''\n{rows}''']"
        ))
        .unwrap();
        Template::try_from(file).unwrap()
    }

    #[test]
    fn test_choices_are_made_by_weight_from_the_seed_for_each_cell_or_once_for_the_whole() {
        let t = weathered();
        let blocks = |seed: u64| -> Vec<Block> {
            (0..256)
                .map(|i| t.cell(0, I64Vec3::new(i % 16, 0, i / 16), seed).unwrap())
                .collect()
        };
        // the same seed chooses the same, and another seed otherwise
        assert_eq!(blocks(5), blocks(5));
        assert_ne!(blocks(5), blocks(6));
        // each cell by weight: stone bricks three times in five, the others once in five each
        let floor = blocks(5);
        let share = |kind: BlockType| floor.iter().filter(|b| b.kind == kind).count();
        assert!(
            (120..=190).contains(&share(BlockType::StoneBricks)),
            "{floor:?}"
        );
        assert!(share(BlockType::MossyStoneBricks) >= 25);
        assert!(share(BlockType::CrackedStoneBricks) >= 25);
        // and once for the whole, the stair and the slab alike, turned as ever
        let mut dark = 0;
        for seed in 0..64 {
            let (stair, slab) = (blocks(seed)[0], blocks(seed)[1]);
            let planks = stair.kind == BlockType::PlanksStairs;
            assert_eq!(planks, slab.kind == BlockType::PlanksSlab, "seed {seed}");
            dark += !planks as usize;
            let turned = t.cell(1, t.turn(1, I64Vec3::ZERO), seed).unwrap();
            assert_eq!(stair.turned(1), turned);
        }
        assert!((16..=48).contains(&dark), "{dark} of 64 dark");
    }

    #[test]
    fn test_choices_are_written_out_as_they_were_read_and_bad_ones_refused() {
        let structure = Structure {
            name: "floor".into(),
            placement: Placement::default(),
            template: weathered(),
        };
        let text = structure.to_toml();
        assert!(
            text.contains("\"stone_bricks*3|mossy_stone_bricks|cracked_stone_bricks\""),
            "{text}"
        );
        assert!(text.contains("\"planks_slab/dark_planks_slab\""), "{text}");
        assert_eq!(structure, toml::from_str::<Structure>(&text).unwrap());
        for bad in ["stone|sand/dirt", "stone*0|sand", "stone*x|sand", "stone|"] {
            assert!(bad.parse::<Choice>().is_err(), "{bad}");
        }
    }

    #[test]
    fn test_turning_moves_every_cell_into_the_turned_footprint_and_back() {
        let t = template("layers = ['''\ncs.\n---\n''', '''\nccc\nc.c\n''']").unwrap();
        for rotation in 0..4 {
            let footprint = t.footprint(rotation);
            for y in 0..2 {
                for z in 0..2 {
                    for x in 0..3 {
                        let at = I64Vec3::new(x, y, z);
                        let turned = t.turn(rotation, at);
                        assert!(turned.xz().cmplt(footprint).all() && turned.min_element() >= 0);
                        assert_eq!(at, t.unturn(rotation, turned));
                        assert_eq!(t.cell(0, at, 0), t.cell(rotation, turned, 0));
                    }
                }
            }
        }
        // a quarter turn takes +x to +z, seen from above
        assert_eq!(I64Vec2::new(2, 3), t.footprint(1));
        assert_eq!(I64Vec3::new(1, 0, 0), t.turn(1, I64Vec3::ZERO));
        assert_eq!(I64Vec3::new(1, 0, 2), t.turn(1, I64Vec3::new(2, 0, 0)));
    }
}
