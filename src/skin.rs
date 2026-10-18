//! Players' skins (M-A4): pictures of a player's model in the layout Minecraft's skins have, 64
//! by 64 pixels or the older 64 by 32, so any skin a player has works on our model. A skin's
//! PNG file is checked wherever it arrives: as a player chooses it, on the server they send it
//! to, and on every client it's passed on to. Only the layout is Minecraft's: the default skins
//! are our own, drawn by `skins.py` (INV-8).

use std::fmt;
use std::io::Cursor;
use std::sync::Arc;

use bevy::ecs::component::Component;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::player::name::PlayerName;

/// The most a skin's file can be, in bytes, which is plenty for 64 by 64 pixels.
pub const MAX_SKIN_BYTES: usize = 32 * 1024;
/// A skin's width, and its height in the current layout, in pixels.
pub const SKIN_SIZE: usize = 64;

/// The skins players wear until they choose their own, each its name under
/// `assets/textures/skins/` and the arms it's drawn for.
pub const DEFAULT_SKINS: [(&str, ArmStyle); 4] = [
    ("ranger", ArmStyle::Classic),
    ("miner", ArmStyle::Classic),
    ("sailor", ArmStyle::Slim),
    ("botanist", ArmStyle::Slim),
];

/// How wide a model's arms are: 4 pixels, or 3 for slim ones.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArmStyle {
    #[default]
    Classic,
    Slim,
}

/// A player's own skin: a PNG file in the layout, which is checked as it's made and as it's
/// read from anywhere, so every one there is can be drawn, and the arms it's drawn with. A
/// player without one wears one of the `DEFAULT_SKINS`, by their name.
#[derive(Component, Clone, PartialEq, Eq)]
pub struct Skin {
    png: Arc<[u8]>,
    arms: ArmStyle,
    hash: u64,
}

impl Skin {
    /// `png` as a skin with `arms`, if it's one.
    pub fn new(png: Vec<u8>, arms: ArmStyle) -> Result<Self, SkinError> {
        decode(&png)?;
        Ok(Self {
            hash: fnv1a(&png),
            png: png.into(),
            arms,
        })
    }

    pub fn png(&self) -> &[u8] {
        &self.png
    }

    pub fn arms(&self) -> ArmStyle {
        self.arms
    }

    /// What tells the file apart from others, for keeping one copy of each.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// Its pixels, in the current layout.
    pub fn pixels(&self) -> SkinPixels {
        decode(&self.png).expect("a skin's checked as it's made").0
    }
}

impl fmt::Debug for Skin {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Skin({:016x}, {:?})", self.hash, self.arms)
    }
}

// Sent and saved as its file's bytes and its arms, and checked again as it's read.
#[derive(Serialize)]
struct SkinRef<'a> {
    png: &'a [u8],
    arms: ArmStyle,
}

#[derive(Deserialize)]
struct SkinOwned {
    png: Vec<u8>,
    arms: ArmStyle,
}

impl Serialize for Skin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let png = &self.png;
        SkinRef {
            png,
            arms: self.arms,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Skin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let SkinOwned { png, arms } = SkinOwned::deserialize(deserializer)?;
        // a byte too many is refused before it's decoded
        if png.len() > MAX_SKIN_BYTES {
            return Err(serde::de::Error::custom(SkinError::TooBig(png.len())));
        }
        Skin::new(png, arms).map_err(serde::de::Error::custom)
    }
}

/// Why a file isn't a skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinError {
    /// It's more bytes than `MAX_SKIN_BYTES`.
    TooBig(usize),
    NotPng,
    /// Its width and height, which aren't a skin's.
    WrongSize(u32, u32),
    /// It's a PNG file, but a broken one.
    Damaged,
}

impl fmt::Display for SkinError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SkinError::TooBig(bytes) => write!(
                f,
                "it's {} KB, more than the {} KB a skin can be",
                bytes.div_ceil(1024),
                MAX_SKIN_BYTES / 1024
            ),
            SkinError::NotPng => write!(f, "it isn't a PNG image"),
            SkinError::WrongSize(width, height) => {
                write!(f, "it's {width}×{height}, not 64×64 or 64×32")
            }
            SkinError::Damaged => write!(f, "the image is damaged"),
        }
    }
}

/// A skin's pixels in the current layout, 64 by 64, RGBA, rows from the top: an old skin's
/// left limbs copied from its right ones, as that layout draws them, the base layer made
/// opaque and the overlay cut out where it's clear.
pub struct SkinPixels(Box<[u8]>);

impl SkinPixels {
    pub fn rgba(&self) -> &[u8] {
        &self.0
    }

    /// The pixel at `x` across and `y` down.
    pub fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * SKIN_SIZE + x) * 4;
        self.0[at..at + 4].try_into().unwrap()
    }

    fn alpha_mut(&mut self, x: usize, y: usize) -> &mut u8 {
        &mut self.0[(y * SKIN_SIZE + x) * 4 + 3]
    }

    // The pixels of the rectangle at `x`, `y`, `width` by `height`.
    fn rect(&self, (x, y, width, height): Rect) -> impl Iterator<Item = (usize, usize)> + '_ {
        (y..y + height).flat_map(move |y| (x..x + width).map(move |x| (x, y)))
    }
}

// Across, down, wide and tall, in pixels.
type Rect = (usize, usize, usize, usize);

// The base layer, which is drawn opaque whatever its alpha, and the head's overlay, the hat.
const BASE: [Rect; 3] = [(0, 0, 32, 16), (0, 16, 64, 16), (16, 48, 32, 16)];
const HAT: Rect = (32, 0, 32, 16);
// In slim skins, the column right of the right arm's back is left clear, and in classic ones
// it's the back's last column.
const SLIM_GAP: Rect = (54, 20, 2, 12);

/// Decodes a skin's PNG file to its pixels in the current layout and the arms its alpha says
/// it's drawn for. Refuses a file over `MAX_SKIN_BYTES`, or that isn't a 64×64 or 64×32 PNG,
/// before decoding its pixels, and decodes them with no more memory than they need.
pub fn decode(png: &[u8]) -> Result<(SkinPixels, ArmStyle), SkinError> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.len() > MAX_SKIN_BYTES {
        return Err(SkinError::TooBig(png.len()));
    }
    if !png.starts_with(SIGNATURE) {
        return Err(SkinError::NotPng);
    }
    let limits = png::Limits { bytes: 1 << 20 };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(png), limits);
    // to 8-bit RGBA or grey and alpha, whatever it's stored as
    decoder.set_transformations(png::Transformations::ALPHA | png::Transformations::STRIP_16);
    let info = decoder.read_header_info().map_err(|_| SkinError::Damaged)?;
    let (width, height) = (info.width, info.height);
    if width != SKIN_SIZE as u32 || !(height == 64 || height == 32) {
        return Err(SkinError::WrongSize(width, height));
    }
    let mut reader = decoder.read_info().map_err(|_| SkinError::Damaged)?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or(SkinError::Damaged)?];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|_| SkinError::Damaged)?;
    let mut rgba = vec![0; SKIN_SIZE * SKIN_SIZE * 4];
    let pixels = SKIN_SIZE * height as usize;
    match frame.color_type {
        png::ColorType::Rgba => rgba[..pixels * 4].copy_from_slice(&buffer[..pixels * 4]),
        png::ColorType::GrayscaleAlpha => {
            let (grey, _) = buffer.as_chunks::<2>();
            for (to, &[value, alpha]) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(grey) {
                *to = [value, value, value, alpha];
            }
        }
        _ => return Err(SkinError::Damaged),
    }
    let mut skin = SkinPixels(rgba.into());
    let legacy = height == 32;
    let arms = if !legacy && skin.rect(SLIM_GAP).all(|(x, y)| skin.at(x, y)[3] == 0) {
        ArmStyle::Slim
    } else {
        ArmStyle::Classic
    };
    if legacy {
        // the left arm and leg are the right ones turned about, as old skins drew them
        mirror_limb(&mut skin, (0, 16), (16, 48));
        mirror_limb(&mut skin, (40, 16), (32, 48));
        // and many filled the hat with a colour meaning none
        let hat = skin.rect(HAT);
        if hat.into_iter().all(|(x, y)| skin.at(x, y)[3] == 255) {
            let hat: Vec<_> = skin.rect(HAT).collect();
            for (x, y) in hat {
                *skin.alpha_mut(x, y) = 0;
            }
        }
    }
    for rect in BASE {
        let base: Vec<_> = skin.rect(rect).collect();
        for (x, y) in base {
            *skin.alpha_mut(x, y) = 255;
        }
    }
    Ok((skin, arms))
}

// Copies the 4×12×4 limb whose texture's at `from` to `to`, as the same limb on the other side:
// each face flipped across, and its two sides swapped.
fn mirror_limb(skin: &mut SkinPixels, from: (usize, usize), to: (usize, usize)) {
    // each face, as where it is in the limb's texture, and where the mirrored one takes it from
    let faces = [
        // top and bottom
        ((4, 0, 4, 4), 4),
        ((8, 0, 4, 4), 8),
        // the outer side, from the inner one, the front, the inner side, the back
        ((0, 4, 4, 12), 8),
        ((4, 4, 4, 12), 4),
        ((8, 4, 4, 12), 0),
        ((12, 4, 4, 12), 12),
    ];
    for ((x, y, width, height), source) in faces {
        for dy in 0..height {
            for dx in 0..width {
                let pixel = skin.at(from.0 + source + width - 1 - dx, from.1 + y + dy);
                let at = ((to.1 + y + dy) * SKIN_SIZE + to.0 + x + dx) * 4;
                skin.0[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }
}

/// Which of the `DEFAULT_SKINS` the player called `name` wears, the same everywhere.
pub fn default_skin(name: &PlayerName) -> usize {
    fnv1a(name.as_str().as_bytes()) as usize % DEFAULT_SKINS.len()
}

// FNV-1a, which is the same on every machine and every build, as `DefaultHasher` needn't be.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG of `width` by `height` pixels, coloured by `colour` of each pixel's place.
    pub(crate) fn png_of(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let data: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| colour(x, y))
            .collect();
        writer.write_image_data(&data).unwrap();
        writer.finish().unwrap();
        bytes
    }

    // Each pixel coloured by where it is, so a copy can be traced back to where it came from,
    // and see-through over the whole overlay and the gap slim arms leave.
    fn traced(x: u32, y: u32) -> [u8; 4] {
        let overlay =
            (32..48).contains(&y) || y >= 48 && !(16..48).contains(&x) || x >= 32 && y < 16;
        let alpha = if overlay { 0 } else { 255 };
        [x as u8 * 4, y as u8 * 4, 7, alpha]
    }

    #[test]
    fn test_skins_must_be_small_pngs_of_either_layout() {
        let skin = png_of(64, 64, traced);
        assert!(decode(&skin).is_ok());
        assert!(decode(&png_of(64, 32, traced)).is_ok());
        for (width, height) in [(32, 32), (64, 48), (128, 128), (64, 65)] {
            assert_eq!(
                Some(SkinError::WrongSize(width, height)),
                decode(&png_of(width, height, traced)).err()
            );
        }
        assert_eq!(Some(SkinError::NotPng), decode(b"GIF89a....").err());
        assert_eq!(Some(SkinError::NotPng), decode(&[]).err());
        // cut short, or with its pixels garbled
        assert_eq!(Some(SkinError::Damaged), decode(&skin[..60]).err());
        let mut garbled = skin.clone();
        let middle = garbled.len() / 2;
        garbled[middle..middle + 40].fill(0x5a);
        assert!(decode(&garbled).is_err());
        // too many bytes, even if they'd make a skin, are refused before they're decoded
        let mut big = skin.clone();
        big.resize(MAX_SKIN_BYTES + 1, 0);
        assert_eq!(
            Some(SkinError::TooBig(MAX_SKIN_BYTES + 1)),
            decode(&big).err()
        );
        assert!(Skin::new(big, ArmStyle::Classic).is_err());
        // and a huge image claimed in a header is refused by its size alone
        let mut huge = png_of(64, 64, traced);
        huge[16..24].copy_from_slice(&[0, 0, 0x40, 0, 0, 0, 0x40, 0]);
        assert!(decode(&huge).is_err());
    }

    #[test]
    fn test_a_skin_round_trips_and_is_checked_again_as_its_read() {
        let skin = Skin::new(png_of(64, 64, traced), ArmStyle::Slim).unwrap();
        let bytes = postcard::to_stdvec(&skin).unwrap();
        let back: Skin = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(skin, back);
        assert_eq!(skin.hash(), back.hash());
        // what isn't a skin isn't read as one
        let mut garbled = bytes.clone();
        let middle = garbled.len() / 2;
        garbled[middle..middle + 40].fill(0x5a);
        assert!(postcard::from_bytes::<Skin>(&garbled).is_err());
        let not = SkinRef {
            png: b"not a png",
            arms: ArmStyle::Classic,
        };
        let bytes = postcard::to_stdvec(&not).unwrap();
        assert!(postcard::from_bytes::<Skin>(&bytes).is_err());
    }

    #[test]
    fn test_the_base_is_opaque_and_the_overlay_cut_out_where_its_clear() {
        let (pixels, _) = decode(&png_of(64, 64, |x, y| [x as u8, y as u8, 0, 0])).unwrap();
        // the head's front, the body's and the left leg's are drawn whatever their alpha
        for (x, y) in [(8, 8), (20, 20), (20, 52), (36, 52)] {
            assert_eq!(255, pixels.at(x, y)[3], "{x}, {y}");
        }
        // the hat, jacket, sleeves and trousers keep theirs
        for (x, y) in [(40, 8), (20, 36), (44, 36), (4, 52), (52, 52)] {
            assert_eq!(0, pixels.at(x, y)[3], "{x}, {y}");
        }
        let half = decode(&png_of(64, 64, |x, y| [x as u8, y as u8, 0, 128])).unwrap();
        assert_eq!(128, half.0.at(40, 8)[3]);
    }

    #[test]
    fn test_an_old_skins_left_limbs_are_its_right_ones_turned_about() {
        let (pixels, arms) = decode(&png_of(64, 32, traced)).unwrap();
        assert_eq!(ArmStyle::Classic, arms);
        let from = |x: usize, y: usize| [x as u8 * 4, y as u8 * 4, 7];
        let rgb = |x, y| -> [u8; 3] { pixels.at(x, y)[..3].try_into().unwrap() };
        // the left leg's front is the right's flipped across
        assert_eq!(from(7, 20), rgb(20, 52));
        assert_eq!(from(4, 31), rgb(23, 63));
        // its inner side is the right's inner side, flipped, and its outer the right's outer
        assert_eq!(from(11, 20), rgb(16, 52));
        assert_eq!(from(3, 20), rgb(24, 52));
        // and its top the right's, flipped
        assert_eq!(from(7, 16), rgb(20, 48));
        // the left arm's back is the right's flipped
        assert_eq!(from(55, 21), rgb(44, 53));
        assert_eq!(from(52, 21), rgb(47, 53));
        // the rest of the new half is clear, as old skins have no overlay on their bodies
        assert_eq!(0, pixels.at(20, 36)[3]);
        assert_eq!(0, pixels.at(4, 52)[3]);
        // a hat filled in solid was meant as none
        let (solid, _) = decode(&png_of(64, 32, |x, y| [x as u8, y as u8, 0, 255])).unwrap();
        assert_eq!(0, solid.at(40, 8)[3]);
        let (hat, _) = decode(&png_of(64, 32, traced)).unwrap();
        assert_eq!(0, hat.at(40, 8)[3]);
    }

    #[test]
    fn test_slim_arms_are_told_by_the_gap_they_leave() {
        let (_, arms) = decode(&png_of(64, 64, traced)).unwrap();
        assert_eq!(ArmStyle::Classic, arms);
        let slim = |x, y| {
            let gap = (54..56).contains(&x) && (20..32).contains(&y);
            if gap {
                [0; 4]
            } else {
                traced(x, y)
            }
        };
        let (pixels, arms) = decode(&png_of(64, 64, slim)).unwrap();
        assert_eq!(ArmStyle::Slim, arms);
        // the gap's base layer, made opaque afterwards
        assert_eq!(255, pixels.at(54, 20)[3]);
    }

    #[test]
    fn test_the_default_skin_is_chosen_by_name_the_same_every_time() {
        let skin = |name| default_skin(&PlayerName::new(name));
        assert_eq!(skin("Ada"), skin("Ada"));
        // fixed, so a player looks the same in every game and every version
        assert_eq!(fnv1a(b"Ada") as usize % DEFAULT_SKINS.len(), skin("Ada"));
        assert_eq!(0xaf63_dc4c_8601_ec8c, fnv1a(b"a"));
        let names = [
            "Ada", "Sam", "host", "guest", "guest2", "Zoë", "mihir", "Player",
        ];
        let mut worn: Vec<_> = names.iter().map(|&name| skin(name)).collect();
        worn.sort_unstable();
        worn.dedup();
        assert!(worn.len() >= 3, "{worn:?}");
    }
}
