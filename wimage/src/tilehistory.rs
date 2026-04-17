use std::{collections::{HashMap}, u16};
use crate::image::{CompressedImage};

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
    pub x: u16,
    pub y: u16,
    pub imgs: HashMap<DateHours, CompressedImage>
}

impl TileHistory {
    /// Deserialize a TileHistory from bytes. For the format, see the to_bytes() method.
    pub fn from_bytes(x: u16, y: u16, data: &[u8]) -> anyhow::Result<TileHistory> {
        if data.len() < 8 {
            return Err(anyhow::anyhow!("data too short for TileHistory"));
        }
        let mut th = TileHistory {
            x,
            y,
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

    /// Get the compressed image for the given date_hours, if it exists. Returns None if there is no entry for that date_hours.
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
}