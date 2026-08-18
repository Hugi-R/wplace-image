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

    /// Add the palleted image to the history at the given date_hours.
    /// The datehour must be equal or after the last datehour in the history due to the diff logic, otherwise it will return an error.
    /// If there are previous versions, it will calculate the diff and store it.
    /// Returns true if the image was added. False if the image was identical to the previous version and was not added.
    pub fn set(&mut self, date_hours: DateHours, paletted: crate::PalettedImage) -> anyhow::Result<bool> {
        // Check last date_hours in history
        if let Some(last_date) = self.imgs.keys().max() {
            if date_hours < *last_date {
                return Err(anyhow::anyhow!("Cannot add image for date_hours {} because it is before the last date_hours {} in the history", date_hours.0, last_date.0));
            }
        }

        // calculate diff with previous version
        let prev_paletted = self.get(DateHours(date_hours.0.saturating_sub(1)))?;


        if !prev_paletted.is_some() {
            // first version, store as full image
            let compressed = paletted.to_compressed_bytes()?;
            self.imgs.insert(date_hours, compressed);
            Ok(true)
        } else {
            let prev_paletted = prev_paletted.unwrap();
            let (any_diff, diff_paletted) = imageprocessing::diff_paletted(&prev_paletted, &paletted);
            if any_diff {
                let compressed = diff_paletted.to_compressed_bytes()?;
                self.imgs.insert(date_hours, compressed);
            }
            Ok(any_diff)
        }
    }

    pub fn list(&self) -> Vec<DateHours> {
        let mut out: Vec<DateHours> = self.imgs.keys().cloned().collect();
        out.sort();
        out
    }

    pub fn has(&self, date_hours: DateHours) -> bool {
        self.imgs.contains_key(&date_hours)
    }

    /// Get the tile image for a specific timestamp by applying all diffs up to that timestamp on top of an empty tile.
    pub fn get(&self, until: DateHours) -> anyhow::Result<Option<PalettedImage>> {
        if self.imgs.is_empty() {
            return Ok(None);
        }

        // hasmap are not ordered, so we need to sort the keys
        let mut keys = self.imgs.keys().cloned().collect::<Vec<DateHours>>();
        keys.sort();
        // Keep keys that are <= until
        keys = keys.into_iter().filter(|k| *k <= until).collect::<Vec<DateHours>>();
        if keys.len() == 0 {
            return Ok(None);
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
        Ok(Some(base_paletted))
    }

    /// Checks that the first image is a full image.
    /// And that no diff image is fully unchanged (all pixels are DIFF_NO_CHANGE).
    /// If filter_datehours is provided, keep only the images that are in the filter_datehours list, and remove all other images from the history (DateHours 0 is always kept if it exists).
    /// If rebuild is true, rebuild the history from scratch (get all images in order, then set them in order).
    /// Returns true if the history is valid, false otherwise, and mutates the history to fix it.
    pub fn validate_and_fix(&mut self, filter_datehours: Option<Vec<DateHours>>, rebuild: bool) -> anyhow::Result<bool> {
        if self.imgs.is_empty() {
            return Ok(true);
        }

        let mut no_changes = true;

        // Apply filter if provided
        let keys_count = self.imgs.len();
        if let Some(filter) = filter_datehours {
            self.imgs.retain(|k, _| filter.contains(k) || *k == DateHours(0));
            no_changes = self.imgs.len() == keys_count;
        }
        if self.imgs.is_empty() {
            return Ok(true);
        }

        // hasmap are not ordered, so we need to sort the keys
        let mut keys = self.imgs.keys().cloned().collect::<Vec<DateHours>>();
        keys.sort();

        // Check that the first image is a full image
        let first_key = keys[0];
        let paletted = self.imgs.get(&first_key).unwrap().to_paletted()?;
        let full_image = paletted.indices.iter().all(|&v| v != palette::DIFF_NO_CHANGE);
        if !full_image {
            // First image is not a full image, we need to fix it by creating a new TileHistory with a full image at the first key and diffs for the rest of the keys.
            let fixed = paletted.indices.iter().map(|&v| if v == palette::DIFF_NO_CHANGE { palette::TRANSPARENT } else { v }).collect::<Vec<u8>>();
            let paletted_fixed = PalettedImage { width: paletted.width, height: paletted.height, indices: fixed };
            self.imgs.insert(first_key, paletted_fixed.to_compressed_bytes()?);
            no_changes = false;
        }

        // Check all other diffs to not be fully unchanged
        for key in keys.iter().skip(1) {
            let paletted = self.imgs.get(key).unwrap().to_paletted()?;
            let all_unchanged = paletted.indices.iter().all(|&v| v == palette::DIFF_NO_CHANGE);
            if all_unchanged {
                // This diff is fully unchanged, we can remove it from the history.
                self.imgs.remove(key);
                no_changes = false;
            }
        }

        // Rebuild
        // This is only usefull if, for some reason, a full image/incorrect diff was manually added to the history
        if rebuild {
            let mut new_history = TileHistory { imgs: HashMap::new() };
            let mut keys = self.imgs.keys().cloned().collect::<Vec<DateHours>>();
            keys.sort();
            for key in keys {
                let paletted = self.get(key)?.unwrap();
                new_history.set(key, paletted)?;
            }
            *self = new_history;
            no_changes = false;
        }

        Ok(no_changes)
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

/// Apply `src` (a tile image or diff image) to both the emitted frame `dst` and the
/// accumulated canvas, but only where the resulting value differs from the canvas.
/// Pixels already matching the canvas stay transparent in the frame, so the APNG
/// only ever records real canvas changes. Returns whether any pixel changed.
fn apply_diff_to_canvas(
    src: &PalettedImage,
    dst: &mut PalettedImage,
    canvas: &mut PalettedImage,
    tile_x_offset: i64,
    tile_y_offset: i64,
    background: u8,
) -> bool {
    assert!(dst.width == canvas.width && dst.height == canvas.height);
    let offset_x = tile_x_offset as usize * src.width;
    let offset_y = tile_y_offset as usize * src.height;

    let mut changed = false;
    for y in 0..src.height {
        let src_row = y * src.width;
        let dst_row = (y + offset_y) * dst.width + offset_x;
        for x in 0..src.width {
            let v = src.indices[src_row + x];
            if v == palette::DIFF_NO_CHANGE {
                continue;
            }
            let value = if v == palette::TRANSPARENT { background } else { v };
            let pos = dst_row + x;
            if canvas.indices[pos] != value {
                canvas.indices[pos] = value;
                dst.indices[pos] = value;
                changed = true;
            }
        }
    }
    changed
}

/// Build the diff-only frame for `date`, rendering each stored image against the
/// accumulated canvas `current`. Frame 0 is always emitted as the full canvas (the
/// APNG base frame); any later frame in which no pixel differs from the canvas is
/// dropped (returns None).
fn build_apng_frame(
    history: &HashMap<(u16, u16), TileHistory>,
    current: &mut PalettedImage,
    date: DateHours,
    frame_index: usize,
    min_x: u16,
    min_y: u16,
    max_x: u16,
    max_y: u16,
) -> Option<PalettedImage> {
    let mut frame_img = init_img_from_tile_coords(
        min_x as i64, min_y as i64, max_x as i64, max_y as i64, palette::TRANSPARENT,
    );
    let mut changed = false;
    for y in min_y..(max_y + 1) {
        for x in min_x..(max_x + 1) {
            if let Some(th) = history.get(&(x, y)) {
                if let Some(img_data) = th.imgs.get(&date) {
                    let img = img_data.to_paletted().unwrap();
                    changed |= apply_diff_to_canvas(
                        &img,
                        &mut frame_img,
                        current,
                        (x - min_x) as i64,
                        (y - min_y) as i64,
                        palette::WHITE,
                    );
                }
            }
        }
    }
    if frame_index == 0 {
        Some(current.clone())
    } else if changed {
        Some(frame_img)
    } else {
        None
    }
}

/// Rewrite the `acTL` chunk's `num_frames` field to the given count and fix its CRC.
/// The animated frame count is only known after generation (empty frames are
/// skipped), but the PNG encoder requires it up front, so a placeholder is patched
/// here after `writer.finish()`.
fn patch_apng_frame_count(out: &mut [u8], count: u32) -> anyhow::Result<()> {
    let mut offset = 8; // PNG signature
    while offset < out.len() {
        if offset + 4 > out.len() {
            anyhow::bail!("truncated PNG chunk header at offset {offset}");
        }
        let length = u32::from_be_bytes(out[offset..offset + 4].try_into().unwrap()) as usize;
        let type_start = offset + 4;
        let data_start = offset + 8;
        let data_end = data_start + length;
        let crc_end = data_end + 4;
        if crc_end > out.len() {
            anyhow::bail!("truncated PNG chunk at offset {offset}");
        }
        if &out[type_start..data_start] == b"acTL" {
            if length < 8 {
                anyhow::bail!("malformed acTL chunk");
            }
            out[data_start..data_start + 4].copy_from_slice(&count.to_be_bytes());
            let mut crc = crc32_update(0, &out[type_start..data_start]);
            crc = crc32_update(crc, &out[data_start..data_end]);
            out[data_end..crc_end].copy_from_slice(&crc.to_be_bytes());
            return Ok(());
        }
        offset = crc_end;
    }
    anyhow::bail!("acTL chunk not found")
}

/// Streaming CRC-32 (IEEE, reflected polynomial 0xEDB88320), as required by the
/// PNG spec for chunk CRCs. Chain calls by feeding the previous result back in,
/// starting with 0. Used to fix the acTL CRC after patching its frame count.
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut crc = crc ^ 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    crc ^ 0xFFFF_FFFF
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

    // -- crc32 tests --

    #[test]
    fn crc32_matches_known_vector() {
        // Standard CRC-32 check value for the ASCII string "123456789".
        assert_eq!(crc32_update(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn crc32_chains_across_calls() {
        let data: Vec<u8> = b"acTL".to_vec().into_iter()
            .chain(2u32.to_be_bytes())
            .chain(0u32.to_be_bytes())
            .collect();
        let one_shot = crc32_update(0, &data);
        let first = crc32_update(0, b"acTL");
        let chained = crc32_update(crc32_update(first, &2u32.to_be_bytes()), &0u32.to_be_bytes());
        assert_eq!(one_shot, chained);
    }

    // -- apply_diff_to_canvas tests --

    #[test]
    fn canvas_writes_changed_pixels_only() {
        let src = make_paletted_with_changes(2, 2, palette::WHITE, &[(0, 10), (3, 20)]);
        let mut frame = make_paletted(2, 2, palette::TRANSPARENT);
        let mut canvas = make_paletted(2, 2, palette::WHITE);
        assert!(apply_diff_to_canvas(&src, &mut frame, &mut canvas, 0, 0, palette::WHITE));
        assert_eq!(canvas.indices, vec![10, 5, 5, 20]);
        assert_eq!(frame.indices, vec![10, 0, 0, 20]);
    }

    #[test]
    fn canvas_identical_image_is_noop() {
        let src = make_paletted(2, 2, palette::WHITE);
        let mut frame = make_paletted(2, 2, palette::TRANSPARENT);
        let mut canvas = make_paletted(2, 2, palette::WHITE);
        assert!(!apply_diff_to_canvas(&src, &mut frame, &mut canvas, 0, 0, palette::WHITE));
        assert_eq!(frame.indices, vec![0; 4]);
        assert_eq!(canvas.indices, vec![palette::WHITE; 4]);
    }

    #[test]
    fn canvas_no_change_pixels_preserve_canvas() {
        // DIFF_NO_CHANGE must not overwrite the canvas and emits transparent.
        let src = make_paletted_with_changes(2, 2, palette::DIFF_NO_CHANGE, &[(0, 9)]);
        let mut frame = make_paletted(2, 2, palette::TRANSPARENT);
        let mut canvas = make_paletted_with_changes(2, 2, palette::WHITE, &[(1, 3)]);
        assert!(apply_diff_to_canvas(&src, &mut frame, &mut canvas, 0, 0, palette::WHITE));
        assert_eq!(frame.indices, vec![9, 0, 0, 0]);
        assert_eq!(canvas.indices, vec![9, 3, 5, 5]);
    }

    #[test]
    fn canvas_transparent_maps_to_background() {
        let src = make_paletted(2, 2, palette::TRANSPARENT);
        let mut frame = make_paletted(2, 2, palette::TRANSPARENT);
        let mut canvas = make_paletted(2, 2, palette::BLACK);
        assert!(apply_diff_to_canvas(&src, &mut frame, &mut canvas, 0, 0, palette::WHITE));
        assert_eq!(frame.indices, vec![palette::WHITE; 4]);
        assert_eq!(canvas.indices, vec![palette::WHITE; 4]);
    }

    #[test]
    fn canvas_places_tiles_at_offset() {
        // 2x1 grid of 2x2 tiles => canvas 4x2; change a pixel in the right tile.
        let left = make_paletted(2, 2, palette::WHITE);
        let right = make_paletted_with_changes(2, 2, palette::WHITE, &[(0, 7)]);
        let mut frame = make_paletted(4, 2, palette::TRANSPARENT);
        let mut canvas = make_paletted(4, 2, palette::WHITE);
        assert!(!apply_diff_to_canvas(&left, &mut frame, &mut canvas, 0, 0, palette::WHITE));
        assert!(apply_diff_to_canvas(&right, &mut frame, &mut canvas, 1, 0, palette::WHITE));
        assert_eq!(canvas.indices, vec![5, 5, 7, 5, 5, 5, 5, 5]);
        assert_eq!(frame.indices, vec![0, 0, 7, 0, 0, 0, 0, 0]);
    }

    // -- build_apng_frame tests --

    fn th_from_entries(entries: &[(u32, PalettedImage)]) -> TileHistory {
        let mut th = TileHistory { imgs: HashMap::new() };
        for (date, img) in entries {
            th.imgs.insert(DateHours(*date), img.to_compressed_bytes().unwrap());
        }
        th
    }

    #[test]
    fn build_frame_0_is_full_canvas() {
        let big = make_paletted(1000, 1000, 10);
        let mut history = HashMap::new();
        history.insert((0u16, 0u16), th_from_entries(&[(0, big)]));
        let mut current = init_img_from_tile_coords(0, 0, 0, 0, palette::WHITE);
        let frame = build_apng_frame(&history, &mut current, DateHours(0), 0, 0, 0, 0, 0).unwrap();
        assert_eq!(frame.indices, vec![10; 1_000_000]);
        assert_eq!(current.indices, vec![10; 1_000_000]);
    }

    #[test]
    fn build_frame_identical_to_canvas_is_none() {
        let mut history = HashMap::new();
        history.insert((0u16, 0u16), th_from_entries(&[
            (0, make_paletted(1000, 1000, 10)),
            (5, make_paletted(1000, 1000, 10)),
        ]));
        let mut current = init_img_from_tile_coords(0, 0, 0, 0, palette::WHITE);
        assert!(build_apng_frame(&history, &mut current, DateHours(0), 0, 0, 0, 0, 0).is_some());
        assert!(build_apng_frame(&history, &mut current, DateHours(5), 1, 0, 0, 0, 0).is_none());
        assert_eq!(current.indices, vec![10; 1_000_000]);
    }

    #[test]
    fn build_frame_full_base_diff_from_canvas_emits_changed_pixels() {
        // makebase-style boundary base: mostly identical to the canvas, a few pixels differ.
        let base = make_paletted_with_changes(1000, 1000, 10, &[(0, 7), (1000, 7)]);
        let mut history = HashMap::new();
        history.insert((0u16, 0u16), th_from_entries(&[
            (0, make_paletted(1000, 1000, 10)),
            (168, base),
        ]));
        let mut current = init_img_from_tile_coords(0, 0, 0, 0, palette::WHITE);
        build_apng_frame(&history, &mut current, DateHours(0), 0, 0, 0, 0, 0);
        let frame = build_apng_frame(&history, &mut current, DateHours(168), 1, 0, 0, 0, 0).unwrap();
        assert_eq!(frame.indices[0], 7);
        assert_eq!(frame.indices[1000], 7);
        assert_eq!(frame.indices[1], palette::TRANSPARENT);
        assert_eq!(current.indices[0], 7);
    }

    #[test]
    fn build_frame_with_no_image_at_date_is_none() {
        let mut history = HashMap::new();
        history.insert((0u16, 0u16), th_from_entries(&[(0, make_paletted(1000, 1000, 10))]));
        let mut current = init_img_from_tile_coords(0, 0, 0, 0, palette::WHITE);
        build_apng_frame(&history, &mut current, DateHours(0), 0, 0, 0, 0, 0);
        assert!(build_apng_frame(&history, &mut current, DateHours(99), 1, 0, 0, 0, 0).is_none());
        assert_eq!(current.indices, vec![10; 1_000_000]);
    }

    // -- patch_apng_frame_count tests --

    fn png_chunk(chunk_type: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(chunk_type);
        out.extend_from_slice(data);
        let mut crc = crc32_update(0, chunk_type);
        crc = crc32_update(crc, data);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn ihdr_data(width: u32, height: u32) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&width.to_be_bytes());
        d.extend_from_slice(&height.to_be_bytes());
        d.extend_from_slice(&[8, 3, 0, 0, 0]); // bit depth 8, indexed color, no interlace
        d
    }

    #[test]
    fn patch_updates_actl_count_and_crc() {
        let mut apng = b"\x89PNG\r\n\x1a\n".to_vec();
        apng.extend_from_slice(&png_chunk(b"IHDR", &ihdr_data(2, 2)));
        let actl_data: Vec<u8> = 1u32.to_be_bytes().into_iter()
            .chain(0u32.to_be_bytes())
            .collect();
        apng.extend_from_slice(&png_chunk(b"acTL", &actl_data)); // placeholder count 1
        apng.extend_from_slice(&png_chunk(b"IEND", &[]));

        patch_apng_frame_count(&mut apng, 3).unwrap();

        // Walk the chunks and verify the acTL count and CRC.
        let mut offset = 8;
        let mut seen_actl = false;
        while offset < apng.len() {
            let length = u32::from_be_bytes(apng[offset..offset + 4].try_into().unwrap()) as usize;
            let ty = &apng[offset + 4..offset + 8];
            if ty == b"acTL" {
                seen_actl = true;
                let nf = u32::from_be_bytes(apng[offset + 8..offset + 12].try_into().unwrap());
                assert_eq!(nf, 3);
                let mut crc = crc32_update(0, ty);
                crc = crc32_update(crc, &apng[offset + 8..offset + 8 + length]);
                let stored = u32::from_be_bytes(
                    apng[offset + 8 + length..offset + 12 + length].try_into().unwrap(),
                );
                assert_eq!(stored, crc);
            }
            offset += 12 + length;
        }
        assert!(seen_actl);
    }

    #[test]
    fn patch_errors_without_actl_chunk() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&png_chunk(b"IHDR", &ihdr_data(2, 2)));
        png.extend_from_slice(&png_chunk(b"IEND", &[]));
        assert!(patch_apng_frame_count(&mut png, 1).is_err());
    }

    #[test]
    fn patch_errors_on_truncated_chunk() {
        let mut bad = b"\x89PNG\r\n\x1a\n".to_vec();
        bad.extend_from_slice(&[0u8, 0, 0, 50]); // claims a 50-byte chunk that never follows
        assert!(patch_apng_frame_count(&mut bad, 1).is_err());
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

    #[test]
    fn set_before_all_existing_errors() {
        // History has only DateHours(5). Inserting DateHours(3) has no
        // previous version to diff against, so it must store as a full image.
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(5), make_paletted(2, 2, 42)).unwrap();
        assert!(th.set(DateHours(3), make_paletted(2, 2, 99)).is_err());
        assert_eq!(th.imgs.len(), 1);
    }

    #[test]
    fn set_between_versions_diffs_errors() {
        // History has 0 and 5. Inserting 3 should diff against 0.
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(5), make_paletted(2, 2, 100)).unwrap();

        // Insert a version between 0 and 5
        assert!(th.set(DateHours(3), make_paletted(2, 2, 77)).is_err());
        assert_eq!(th.imgs.len(), 2);
    }

    #[test]
    fn set_overwrites_existing_version() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(1), make_paletted(2, 2, 100)).unwrap();

        // Overwrite version 1 with a different image
        th.set(DateHours(1), make_paletted(2, 2, 200)).unwrap();
        assert_eq!(th.imgs.len(), 2);

        let result = th.get(DateHours(1)).unwrap().unwrap();
        assert!(result.indices.iter().all(|&v| v == 200));
    }

    #[test]
    fn get_with_non_zero_first_entry() {
        // First entry is at DateHours(5), not 0.
        let mut th = TileHistory { imgs: HashMap::new() };
        let img = make_paletted_with_changes(2, 2, 42, &[(0, 10)]);
        th.set(DateHours(5), img.clone()).unwrap();

        let result = th.get(DateHours(5)).unwrap().unwrap();
        assert_eq!(result.indices, img.indices);
    }

    #[test]
    fn get_empty_history_none() {
        let th = TileHistory { imgs: HashMap::new() };
        let result = th.get(DateHours(0));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn get_base_version_only() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();

        let result = th.get(DateHours(0)).unwrap().unwrap();
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
        let result = th.get(DateHours(1)).unwrap().unwrap();
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

        let result = th.get(DateHours(1)).unwrap().unwrap();
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
        // so requesting DateHours(5) finds no keys <= 5 and returns none.
        let base = make_paletted(2, 2, 42);
        let compressed = base.to_compressed_bytes().unwrap();
        let th = TileHistory {
            imgs: [(DateHours(10), compressed)].into_iter().collect(),
        };
        let result = th.get(DateHours(5));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn get_roundtrip() {
        let mut th = TileHistory { imgs: HashMap::new() };

        let expected_base = make_paletted_with_changes(4, 4, 42, &[(0, 10), (7, 20), (15, 30)]);
        th.set(DateHours(0), expected_base.clone()).unwrap();

        let expected_v1 = make_paletted_with_changes(4, 4, 42, &[(0, 50), (3, 60), (10, 70)]);
        th.set(DateHours(1), expected_v1.clone()).unwrap();

        // Get version 0
        let result_v0 = th.get(DateHours(0)).unwrap().unwrap();
        assert_eq!(result_v0.indices, expected_base.indices);

        // Get version 1
        let result_v1 = th.get(DateHours(1)).unwrap().unwrap();
        assert_eq!(result_v1.indices, expected_v1.indices);
    }

    #[test]
    fn get_base_version_when_later_versions_exist() {
        let mut th = TileHistory { imgs: HashMap::new() };
        th.set(DateHours(0), make_paletted(2, 2, 42)).unwrap();
        th.set(DateHours(1), make_paletted(2, 2, 100)).unwrap();

        // Get base version even though later versions exist
        let result = th.get(DateHours(0)).unwrap().unwrap();
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

        let orig_get0 = th.get(DateHours(0)).unwrap().unwrap();
        let restored_get0 = restored.get(DateHours(0)).unwrap().unwrap();
        assert_eq!(restored_get0.indices, orig_get0.indices);

        let orig_get1 = th.get(DateHours(1)).unwrap().unwrap();
        let restored_get1 = restored.get(DateHours(1)).unwrap().unwrap();
        assert_eq!(restored_get1.indices, orig_get1.indices);
    }
}