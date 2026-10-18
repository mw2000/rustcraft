use std::collections::BTreeMap;

use bevy::math::{I64Vec2, I64Vec3, Vec3Swizzles};
use serde::Deserialize;

use crate::block::BlockType;

// What a template's layers write for a cell that leaves the world as it is, and for one that
// clears it to air, carving interiors out of the ground.
const LEAVE: char = '.';
const CLEAR: char = '-';
/// The kind of marker where a structure has a chest of loot: generation puts a chest there, and
/// the server fills it from the loot table named after the structure the first time it's opened
/// (`loot`, §5.6).
pub const LOOT: &str = "loot";

/// How far above its floor any structure reaches, and how far under it the layers sunk into
/// the ground go, so chunks and LOD nodes know how far up and down to look for them.
pub const STRUCTURE_HEIGHT: i64 = 27;
pub const STRUCTURE_DEPTH: i64 = 4;

/// A structure type, as its file in `assets/structures/` declares it: where it's placed, and the
/// blocks it's built of.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    pub name: String,
    pub placement: Placement,
    pub template: Template,
}

/// Where a structure type stands: on a grid of square cells `cell` blocks across, with one in a
/// cell `chance` of the time where the ground suits it.
#[derive(Debug, Deserialize)]
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
}

/// A structure's blocks: a box of cells, each a block, air cleared out of the ground, or left as
/// the world has it.
#[derive(Debug, Deserialize)]
#[serde(try_from = "TemplateFile")]
pub struct Template {
    /// Across x, up and across z.
    pub size: I64Vec3,
    /// The layer standing at ground level: those below it are sunk into the ground.
    pub floor: i64,
    /// What fills the ground up to its lowest layer where the ground falls away under it.
    pub foundation: BlockType,
    // x fastest, then z, then y; None leaves the world as it is, and air clears it
    cells: Vec<Option<BlockType>>,
    /// Points of interest by kind, such as `loot` for a chest (M2.3).
    pub markers: Vec<(String, I64Vec3)>,
}

// The file as written, before it's checked and its layers read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    size: [i64; 3],
    floor: i64,
    foundation: String,
    palette: BTreeMap<char, String>,
    #[serde(default)]
    markers: BTreeMap<String, Vec<[i64; 3]>>,
    layers: Vec<String>,
}

impl TryFrom<TemplateFile> for Template {
    type Error = String;

    fn try_from(file: TemplateFile) -> Result<Self, String> {
        let size = I64Vec3::from_array(file.size);
        let block = |name: &str| BlockType::from_name(name).ok_or(format!("no block {name}"));
        let mut palette = BTreeMap::new();
        for (&key, name) in &file.palette {
            if key == LEAVE || key == CLEAR {
                return Err(format!("'{key}' is kept for leaving or clearing a cell"));
            }
            palette.insert(key, block(name)?);
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
                    CLEAR => Some(BlockType::Air),
                    key => Some(
                        *palette
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
                if kind == LOOT {
                    let index = (point.y * size.z + point.z) * size.x + point.x;
                    cells[index as usize] = Some(BlockType::Chest);
                }
                markers.push((kind.clone(), point));
            }
        }
        Ok(Self {
            size,
            floor: file.floor,
            foundation: block(&file.foundation)?,
            cells,
            markers,
        })
    }
}

impl Template {
    /// Its footprint turned `rotation` quarter turns, across x and z.
    pub fn footprint(&self, rotation: u8) -> I64Vec2 {
        if rotation.is_multiple_of(2) {
            self.size.xz()
        } else {
            self.size.zx()
        }
    }

    /// The cell at `at` in the template turned `rotation` quarter turns about its middle, from
    /// its lowest corner: None to leave the world as it is, air to clear it.
    pub fn cell(&self, rotation: u8, at: I64Vec3) -> Option<BlockType> {
        let at = self.unturn(rotation, at);
        let size = self.size;
        if at.cmplt(I64Vec3::ZERO).any() || at.cmpge(size).any() {
            return None;
        }
        self.cells[((at.y * size.z + at.z) * size.x + at.x) as usize]
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
        assert_eq!(Some(BlockType::Cobblestone), t.cell(0, I64Vec3::ZERO));
        assert_eq!(
            Some(BlockType::StoneBricks),
            t.cell(0, I64Vec3::new(1, 0, 0))
        );
        assert_eq!(None, t.cell(0, I64Vec3::new(2, 0, 0)));
        assert_eq!(Some(BlockType::Air), t.cell(0, I64Vec3::new(1, 0, 1)));
        assert_eq!(None, t.cell(0, I64Vec3::new(1, 1, 1)));
        // outside it, the world is left as it is
        assert_eq!(None, t.cell(0, I64Vec3::new(3, 0, 0)));
        assert_eq!(vec![("loot".to_string(), I64Vec3::new(2, 1, 0))], t.markers);
        // and its loot marker is a chest
        assert_eq!(Some(BlockType::Chest), t.cell(0, I64Vec3::new(2, 1, 0)));
    }

    #[test]
    fn test_bad_templates_are_refused() {
        // too few rows, an unknown key, and too few layers
        assert!(template("layers = ['''\ncs.\n''', '''\nccc\nc.c\n''']").is_err());
        assert!(template("layers = ['''\ncx.\n---\n''', '''\nccc\nc.c\n''']").is_err());
        assert!(template("layers = ['''\ncs.\n---\n''']").is_err());
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
                        assert_eq!(t.cell(0, at), t.cell(rotation, turned));
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
