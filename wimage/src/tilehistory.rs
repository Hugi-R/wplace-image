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

    pub fn set(&mut self, date_hours: DateHours, paletted: crate::PalettedImage) -> anyhow::Result<()> {
        if (date_hours.0 == 0) || self.imgs.is_empty() {
            // first version, store as full image
            let compressed = paletted.to_compressed_bytes()?;
            self.imgs.insert(date_hours, compressed);
            Ok(())
        } else {
            // calculate diff with previous version
            let mut prev_date_hours = None;
            for key in self.imgs.keys() {
                if *key < date_hours {
                    if prev_date_hours.is_none() || *key > prev_date_hours.unwrap() {
                        prev_date_hours = Some(*key);
                    }
                }
            }
            let prev_date_hours = prev_date_hours.ok_or(anyhow::anyhow!("No previous version found for diff"))?;
            let prev_img = self.imgs.get(&prev_date_hours).ok_or(anyhow::anyhow!("Previous version not found"))?;
            let prev_paletted = prev_img.to_paletted()?;
            let (_, diff_paletted) = imageprocessing::diff_paletted(&prev_paletted, &paletted);
            let compressed = diff_paletted.to_compressed_bytes()?;
            self.imgs.insert(date_hours, compressed);
            Ok(())
        }
    }

    pub fn list(&self) -> Vec<DateHours> {
        let mut out: Vec<DateHours> = self.imgs.keys().cloned().collect();
        out.sort();
        out
    }

    /// Get the tile image for a specific timestamp by applying all diffs up to that timestamp on top of an empty tile.
    pub fn get(&self, until: DateHours) -> anyhow::Result<PalettedImage> {
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

// ===================== Tests for TileHistory =====================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_paletted(width: usize, height: usize, value: u8) -> PalettedImage {
        PalettedImage { width, height, indices: vec![value; width * height] }
    }

    fn make_paletted_with_changes(width: usize, height: usize, base: u8, changes: &[(usize, u8)]) -> PalettedImage {
        let mut img = make_paletted(width, height, base);
        for &(idx, val) in changes {
            img.indices[idx] = val;
        }
        img
    }

    // -- set tests --

    #[test]
    fn set_base_image() {
        let mut th = TileHistory { imgs: HashMap::new() };
        let img = make_paletted(2, 2, 42);
        assert!(th.set(DateHours(0), img).is_ok());
        assert_eq!(th.imgs.len(), 1);
        assert!(th.imgs.contains_key(&DateHours(0)));
    }

    #[test]
    fn set_image_without_base_becomes_base() {
        let mut th = TileHistory { imgs: HashMap::new() };
        let img = make_paletted(2, 2, 42);
        // No base image exists, so treat as base image
        assert!(th.set(DateHours(1), img).is_ok());
        assert_eq!(th.imgs.len(), 1);
        assert!(th.imgs.contains_key(&DateHours(1)));
    }

    #[test]
    fn set_diff_image_after_base_succeeds() {
        let mut th = TileHistory { imgs: HashMap::new() };
        let base = make_paletted(2, 2, 42);
        th.set(DateHours(0), base).unwrap();

        let diff = make_paletted(2, 2, 100);
        assert!(th.set(DateHours(1), diff).is_ok());
        assert_eq!(th.imgs.len(), 2);
    }

    #[test]
    fn set_multiple_versions() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(1), make_paletted(2, 2, 100)).unwrap();
        th.set(DateHours(2), make_paletted(2, 2, 200)).unwrap();
        assert_eq!(th.imgs.len(), 3);
    }

    // -- get tests --

    #[test]
    fn get_empty_history_fails() {
        let th = TileHistory { imgs: HashMap::new() };
        let result = th.get(DateHours(0));
        assert!(result.is_err());
    }

    #[test]
    fn get_base_version_only() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();

        let result = th.get(DateHours(0)).unwrap();
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
        assert!(result.indices.iter().all(|&v| v == 42));
    }

    #[test]
    fn get_after_diff_applies() {
        let mut th = TileHistory { imgs: HashMap::new() };
        // Base: all 42s
        let base = make_paletted(2, 2, 42);
        th.set(DateHours(0), base).unwrap();

        // Diff: all pixels changed to 100
        let diff = make_paletted(2, 2, 100);
        th.set(DateHours(1), diff).unwrap();

        // Get version at DateHours(1) should return the reconstructed image
        let result = th.get(DateHours(1)).unwrap();
        assert!(result.indices.iter().all(|&v| v == 100));
    }

    #[test]
    fn get_partial_diff() {
        let mut th = TileHistory { imgs: HashMap::new() };
        // Base: all 42s
        let base = make_paletted(4, 4, 42);
        th.set(DateHours(0), base).unwrap();

        // Diff: only 3 pixels changed
        let mut diff = make_paletted(4, 4, 42);
        diff.indices[0] = 10;
        diff.indices[5] = 20;
        diff.indices[15] = 30;
        th.set(DateHours(1), diff).unwrap();

        let result = th.get(DateHours(1)).unwrap();
        assert_eq!(result.indices[0], 10);
        assert_eq!(result.indices[5], 20);
        assert_eq!(result.indices[15], 30);
        // Unchanged pixels should remain 42
        for i in 0..16 {
            if i != 0 && i != 5 && i != 15 {
                assert_eq!(result.indices[i], 42, "Pixel {} should be 42", i);
            }
        }
    }

    #[test]
    fn get_no_images_for_version() {
        // Build a TileHistory that only has a version at DateHours(10),
        // so requesting DateHours(5) finds no keys <= 5 and returns an error.
        let base = make_paletted(2, 2, 42);
        let compressed = base.to_compressed_bytes().unwrap();
        let th = TileHistory {
            imgs: [(DateHours(10), compressed)].into_iter().collect(),
        };
        let result = th.get(DateHours(5));
        assert!(result.is_err());
    }

    #[test]
    fn get_roundtrip() {
        let mut th = TileHistory { imgs: HashMap::new() };

        let expected_base = make_paletted_with_changes(4, 4, 42, &[(0, 10), (7, 20), (15, 30)]);
        th.set(DateHours(0), expected_base.clone()).unwrap();

        let expected_v1 = make_paletted_with_changes(4, 4, 42, &[(0, 50), (3, 60), (10, 70)]);
        th.set(DateHours(1), expected_v1.clone()).unwrap();

        // Get version 0
        let result_v0 = th.get(DateHours(0)).unwrap();
        assert_eq!(result_v0.indices, expected_base.indices);

        // Get version 1
        let result_v1 = th.get(DateHours(1)).unwrap();
        assert_eq!(result_v1.indices, expected_v1.indices);
    }

    #[test]
    fn get_base_version_when_later_versions_exist() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(1), make_paletted(2, 2, 100)).unwrap();

        // Get base version even though later versions exist
        let result = th.get(DateHours(0)).unwrap();
        assert!(result.indices.iter().all(|&v| v == 42));
    }

    // -- to_bytes / from_bytes roundtrip --

    #[test]
    fn roundtrip_to_from_bytes() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(1), make_paletted(2, 2, 100)).unwrap();

        let bytes = th.to_bytes();
        let restored = TileHistory::from_bytes(&bytes).unwrap();

        let orig_get0 = th.get(DateHours(0)).unwrap();
        let restored_get0 = restored.get(DateHours(0)).unwrap();
        assert_eq!(restored_get0.indices, orig_get0.indices);

        let orig_get1 = th.get(DateHours(1)).unwrap();
        let restored_get1 = restored.get(DateHours(1)).unwrap();
        assert_eq!(restored_get1.indices, orig_get1.indices);
    }
}