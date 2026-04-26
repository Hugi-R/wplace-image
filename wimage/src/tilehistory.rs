use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc, Duration as ChronoDuration};
use png::{BitDepth, ColorType, Encoder};

use crate::image::{CompressedImage, PalettedImage};
use crate::imageprocessing;
use crate::palette;

pub const ERR_TILE_HISTORY_NO_IMAGES: &str = "TileHistory has no images";
pub const ERR_NO_IMAGES_FOR_VERSION: &str = "No images for requested version";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DateHours(pub u32);

// Number of hours since the epoch "2025-01-01T00:00:00Z"
impl DateHours {
    /// Epoch: 2025-01-01 00:00:00 UTC
    pub const EPOCH: &'static str = "2025-01-01T00:00:00Z";

    pub fn min() -> Self {
        DateHours(0)
    }

    pub fn max() -> Self {
        DateHours(u32::MAX)
    }

    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        let epoch = DateTime::parse_from_rfc3339(Self::EPOCH)
            .unwrap()
            .with_timezone(&Utc);
        let duration = dt.signed_duration_since(epoch);
        let hours = duration.num_hours() as u32;
        DateHours(hours)
    }

    pub fn to_datetime(&self) -> DateTime<Utc> {
        let epoch = DateTime::parse_from_rfc3339(Self::EPOCH)
            .unwrap()
            .with_timezone(&Utc);
        epoch + ChronoDuration::hours(self.0 as i64)
    }

    pub fn week(&self) -> u32 {
        self.0 / (24 * 7)
    }

    pub fn day(&self) -> u32 {
        self.0 / 24
    }
}

/// Represents the history of a single tile, containing multiple versions of the tile image at different timestamps.
/// Each version is stored as a compressed diff image, keyed by the DateHours timestamp of when that version was created.
/// By convention, if the first key is 0, then that version is a full image. Otherwise, all versions are diffs that need to be applied on top of an empty tile.
pub struct TileHistory {
    pub imgs: HashMap<DateHours, CompressedImage>
}

impl TileHistory {
    /// Deserialize a TileHistory from bytes. For the format, see the to_bytes() method.
    pub fn from_bytes(data: &[u8]) -> anyhow::Result<TileHistory> {
        let mut th = TileHistory {
            imgs: HashMap::new(),
        };
        let mut offset = 0;
        while offset < data.len() {
            if offset + 8 > data.len() {
                return Err(anyhow::anyhow!("data too short for TileHistory entry"));
            }
            let date_hours = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            let block_size = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            if offset + block_size > data.len() {
                return Err(anyhow::anyhow!("data too short for TileHistory image data"));
            }
            th.imgs.insert(DateHours(date_hours as u32), CompressedImage(data[offset..(offset+block_size)].to_vec()));
            offset += block_size;
        }
        Ok(th)
    }

    /// Get the compressed image for the given date_hours, if it exists. Returns an error if there is no entry for that date_hours.
    /// Convenience function to avoid having to deserialize the entire TileHistory if you just want to get a single version of the tile (usually for debugging).
    pub fn raw_get(data: &[u8], date_hours: DateHours) -> anyhow::Result<CompressedImage> {
        if data.len() < 8 {
            return Err(anyhow::anyhow!("data too short for TileHistory entry"));
        }
        let mut offset = 0;
        while offset < data.len() {
            if offset + 8 > data.len() {
                return Err(anyhow::anyhow!("data too short for TileHistory entry"));
            }
            let entry_date_hours = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            let block_size = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            if offset + block_size > data.len() {
                return Err(anyhow::anyhow!("data too short for TileHistory image data"));
            }
            if entry_date_hours == date_hours.0 as usize {
                return Ok(CompressedImage(data[offset..(offset+block_size)].to_vec()));
            }
            offset += block_size;
        }
        Err(anyhow::anyhow!("TileHistory entry not found"))
    }

    /// Get a list of all DateHours entries in the TileHistory, in the order they appear in the byte data. Returns an empty list if the data is too short or malformed.
    /// Convenience function to avoid having to deserialize the entire TileHistory.
    pub fn raw_list(data: &[u8]) -> Vec<DateHours> {
        let mut out = Vec::new();
        if data.len() < 8 {
            return out;
        }
        let mut offset = 0;
        while offset < data.len() {
            if offset + 8 > data.len() {
                return out;
            }
            let entry_date_hours = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            let block_size = u32::from_le_bytes([data[offset+0], data[offset+1], data[offset+2], data[offset+3]]) as usize;
            offset += 4;
            if offset + block_size > data.len() {
                return out;
            }
            out.push(DateHours(entry_date_hours as u32));
            offset += block_size;
        }
        out
    }

    /// Serialize the TileHistory to bytes. The format is a sequence of entries, where each entry consists of:
    /// [u32 little-endian date_hours][u32 little-endian block_size][block_size bytes of compressed image data]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let sorted_date = {
            let mut v: Vec<DateHours> = self.imgs.keys().cloned().collect();
            v.sort();
            v
        };
        for date_hours in sorted_date {
            let img = self.imgs.get(&date_hours).unwrap();
            out.extend_from_slice(&date_hours.0.to_le_bytes());
            let img_data = &img.0;
            out.extend_from_slice(&(img_data.len() as u32).to_le_bytes());
            out.extend_from_slice(img_data);
        }
        out
    }

    pub fn add(&mut self, date_hours: DateHours, paletted: crate::PalettedImage) -> anyhow::Result<()> {
        let compressed = paletted.to_compressed_bytes()?;
        self.imgs.insert(date_hours, compressed);
        Ok(())
    }

    pub fn get(&self, date_hours: DateHours) -> anyhow::Result<crate::PalettedImage> {
        let compressed = self.imgs.get(&date_hours).ok_or(anyhow::anyhow!("Version not found"))?;
        crate::PalettedImage::from_compressed_bytes(&compressed.0)
    }

    pub fn list(&self) -> Vec<DateHours> {
        let mut out: Vec<DateHours> = self.imgs.keys().cloned().collect();
        out.sort();
        out
    }

    /// Get the tile image for a specific timestamp by applying all diffs up to that timestamp on top of an empty tile.
    pub fn image(&self, until: DateHours) -> anyhow::Result<PalettedImage> {
        if self.imgs.is_empty() {
            return Err(anyhow::anyhow!(ERR_TILE_HISTORY_NO_IMAGES));
        }

        // hasmap are not ordered, so we need to sort the keys
        let mut keys = self.imgs.keys().cloned().collect::<Vec<DateHours>>();
        keys.sort();
        // Keep keys that are <= until
        keys = keys.into_iter().filter(|k| *k <= until).collect::<Vec<DateHours>>();
        if keys.len() == 0 {
            return Err(anyhow::anyhow!(ERR_NO_IMAGES_FOR_VERSION));
        }

        // Load base image
        let base_data = self.imgs.get(&keys[0]).unwrap();
        let mut base_paletted = base_data.to_paletted()?;

        // Apply diffs
        for key in keys.iter().skip(1) {
            let version_data = self.imgs.get(key).unwrap();
            let version_paletted = version_data.to_paletted()?;

            base_paletted = imageprocessing::apply_diff_paletted(&base_paletted, &version_paletted);
        }
        Ok(base_paletted)
    }
}

pub fn init_img_from_tile_coords(x1: i64, y1: i64, x2: i64, y2: i64, background: u8) -> PalettedImage {
    assert!(x2 >= x1 && y2 >= y1);

    let height = ((y2+1)-y1)*1000;
    let width = ((x2+1)-x1)*1000;
    assert!((height*width) < (30000*30000)); // That's already 900MB of indices! Also few things will display a bigger image.
    PalettedImage { width: width as usize, height: height as usize, indices: vec![background; (width*height) as usize] }
}

pub fn apng_from_history(history: HashMap<(u16, u16), TileHistory>, frame_delay_ms: u16) -> anyhow::Result<Vec<u8>> {
    assert!(history.len() >= 1, "need at least one tile history to create APNG");
    let mut date_set: HashSet<DateHours> = HashSet::new();
    let mut min_x: u16 = u16::MAX;
    let mut min_y: u16 = u16::MAX;
    let mut max_x: u16 = 0;
    let mut max_y: u16 = 0;


    for (x, y) in history.keys() {
        let (x, y) = (*x, *y);
        let th = history.get(&(x, y)).unwrap();
        for date in th.imgs.keys() {
            date_set.insert(*date);
        }
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
    }

    let sorted_dates: Vec<DateHours> = {
        let mut v: Vec<DateHours> = date_set.into_iter().collect();
        v.sort_by_key(|d| d.0);
        v
    };

    let target_img = init_img_from_tile_coords(min_x as i64, min_y as i64, max_x as i64, max_y as i64, palette::WHITE);

    assert!(sorted_dates.len() >= 1, "need at least one frame for APNG");
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, target_img.width as u32, target_img.height as u32);
    encoder.set_color(ColorType::Indexed);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);

    // Build palette (RGB triples) and tRNS (alpha table)
    let pal = &palette::PNG_PALETTE_NO_DIFF;
    encoder.set_palette(&pal.0);
    encoder.set_trns(pal.1.as_slice());
    encoder.set_animated(sorted_dates.len() as u32, 0)?;
    encoder.set_blend_op(png::BlendOp::Over)?;
    encoder.set_frame_delay(frame_delay_ms, 1000)?;
    let mut writer = encoder.write_header()?;

    let mut first_frame = true;
    for date in sorted_dates {
        let mut frame_img = if first_frame { 
            first_frame = false;
            target_img.clone()
        } else { 
            init_img_from_tile_coords(min_x as i64, min_y as i64, max_x as i64, max_y as i64, palette::TRANSPARENT)
        };
        for y in min_y..(max_y+1) {
            for x in min_x..(max_x+1) {
                if let Some(th) = history.get(&(x, y)) {
                    if let Some(img_data) = th.imgs.get(&date) {
                        let img = img_data.to_paletted().unwrap();
                        apply_diff_img(&img, &mut frame_img, (x - min_x) as i64, (y - min_y) as i64, palette::WHITE);
                    }
                }
            }
        }

        writer.write_image_data(&frame_img.indices)?;
    }

    writer.finish()?;
    Ok(out)
}

fn apply_diff_img(src: &PalettedImage, dst: &mut PalettedImage, tile_x_offset: i64, tile_y_offset: i64, background: u8) {
    let offset_x = (tile_x_offset * 1000) as usize;
    let offset_y = (tile_y_offset * 1000) as usize;
    for y in 0..src.height {
        let src_row_start = y * src.width;
        let dst_row_start = (y + offset_y) * dst.width + offset_x;
        
        for x in 0..src.width {
            let v = src.indices[src_row_start + x];
            if v != palette::DIFF_NO_CHANGE {
                if v == palette::TRANSPARENT {
                    dst.indices[dst_row_start + x] = background;
                } else {
                    dst.indices[dst_row_start + x] = v;
                }
            } else {
                dst.indices[dst_row_start + x] = palette::TRANSPARENT;
            }
        }
    }
}