use std::io::{Read, Write, Cursor, BufRead, Seek};

use anyhow::anyhow;
use png::{Decoder, Encoder, ColorType, BitDepth};

use crate::palette::{index_from_rgba, PALETTE};

/// In-memory representation of a paletted image
pub struct PalettedImage {
    pub width: usize,
    pub height: usize,
    pub indices: Vec<u8>,
}

impl Clone for PalettedImage {
    fn clone(&self) -> Self {
        PalettedImage { width: self.width, height: self.height, indices: self.indices.clone() }
    }
}

impl PalettedImage {
    pub fn from_png<R: BufRead + Seek>(reader: R) -> anyhow::Result<Self> {
        png_to_paletted(reader)
    }

    pub fn to_png(&self) -> anyhow::Result<Vec<u8>> {
        let mut png: Vec<u8> = Vec::new();
        paletted_to_png(self, &mut png, true)?;
        Ok(png)
    }

    pub fn to_png_diff(&self) -> anyhow::Result<Vec<u8>> {
        let mut png: Vec<u8> = Vec::new();
        paletted_to_png(self, &mut png, false)?;
        Ok(png)
    }

    pub fn to_compressed_bytes(&self) -> anyhow::Result<CompressedImage> {
        Ok(CompressedImage(paletted_to_compressed_bytes(self)?))
    }
}

#[derive(Debug, Clone)]
pub struct CompressedImage(pub Vec<u8>);

impl CompressedImage {
    pub fn to_paletted(&self) -> anyhow::Result<PalettedImage> {
        compressed_bytes_to_paletted(&self.0)
    }
}

/// Convert a paletted image back to a PNG.
pub fn paletted_to_png<W: Write>(paletted: &PalettedImage, out: W, ignore_diff: bool) -> anyhow::Result<()> {
    {
        let mut encoder = Encoder::new(out, paletted.width as u32, paletted.height as u32);
        encoder.set_color(ColorType::Indexed);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest); // Fast or Fastest are good choices

        // Build palette (RGB triples) and tRNS (alpha table)
        let mut palette_bytes = Vec::with_capacity(256 * 3);
        let mut trns = Vec::with_capacity(256);
        let palette = if ignore_diff {
            let mut pal = PALETTE.clone();
            pal[crate::palette::DIFF_NO_CHANGE as usize] = [0,0,0,0];
            pal
        } else {
            PALETTE.clone()
        };
        for rgba in palette.iter() {
            palette_bytes.push(rgba[0]);
            palette_bytes.push(rgba[1]);
            palette_bytes.push(rgba[2]);
            trns.push(rgba[3]);
        }
        encoder.set_palette(palette_bytes);
        encoder.set_trns(trns.as_slice());

        let mut writer = encoder.write_header()?;
        writer.write_image_data(&paletted.indices)?;
    }
    Ok(())
}

/// Convert a paletted image to zstd-compressed bytes.
///
/// Uncompressed format:
/// [u32 little-endian width][u32 little-endian height][width*height bytes of u8 indices]
pub fn paletted_to_compressed_bytes(paletted: &PalettedImage) -> anyhow::Result<Vec<u8>> {
    // Serialize metadata + indices
    let mut out = Vec::with_capacity(8 + paletted.indices.len());
    out.extend(&((paletted.width as u32).to_le_bytes()));
    out.extend(&((paletted.height as u32).to_le_bytes()));
    out.extend(&paletted.indices);

    // Compress with zstd
    let mut enc = zstd::Encoder::new(Vec::new(), 7)?; // level 7 is good compression and speed. Avoid 4-6 and > 10, very bad speed
    enc.write_all(&out)?;
    let compressed = enc.finish()?;

    Ok(compressed)
}

/// Read the zstd-compressed paletted byte array (the format written by
/// `paletted_to_compressed_bytes`) and convert it to a paletted image representation.
pub fn compressed_bytes_to_paletted(compressed: &[u8]) -> anyhow::Result<PalettedImage> {
    let mut dec = zstd::Decoder::new(Cursor::new(compressed))?;
    let mut decompressed = Vec::new();
    dec.read_to_end(&mut decompressed)?;

    if decompressed.len() < 8 {
        return Err(anyhow!("decompressed data too short"));
    }
    let width = u32::from_le_bytes([decompressed[0], decompressed[1], decompressed[2], decompressed[3]]);
    let height = u32::from_le_bytes([decompressed[4], decompressed[5], decompressed[6], decompressed[7]]);
    let expected = (width as usize) * (height as usize);
    if decompressed.len() != 8 + expected {
        return Err(anyhow!("decompressed length mismatch: expected {} bytes of indices, got {}", expected, decompressed.len() - 8));
    }
    let indices = decompressed[8..].to_vec();

    Ok(PalettedImage {
        width: width as usize,
        height: height as usize,
        indices,
    })
}

/// Convert a PNG stream (from BufReader) to a paletted image representation.
pub fn png_to_paletted<R: BufRead + Seek>(reader: R) -> anyhow::Result<PalettedImage> {
    let decoder = Decoder::new(reader);
    let mut png_reader = decoder.read_info()?;
    // Require reader-provided buffer size; otherwise return an error.
    let buf_size = png_reader.output_buffer_size()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "PNG decoder did not provide an output buffer size"))?;
    let mut buf = vec![0u8; buf_size];
    png_reader.next_frame(&mut buf)?;
    let info = png_reader.info();
    let rgba = expand_to_rgba8(&info.color_type, &info.bit_depth, &buf, info)?;

    let width = info.width as usize;
    let height = info.height as usize;
    if rgba.len() != width * height * 4 {
        return Err(anyhow!("unexpected rgba length {} for {}x{}", rgba.len(), width, height).into());
    }

    // Map RGBA -> palette index
    let mut indices = Vec::with_capacity(width * height);
    for px in rgba.chunks_exact(4) {
        let mut rgba4 = [0u8;4];
        rgba4.copy_from_slice(px);
        let idx = index_from_rgba(rgba4);
        indices.push(idx);
    }

    Ok(PalettedImage {
        width: width,
        height: height,
        indices,
    })
}

// -- helpers ---------------------------------------------------------------

fn expand_to_rgba8(color: &ColorType, bit_depth: &BitDepth, buf: &[u8], info: &png::Info) -> anyhow::Result<Vec<u8>> {
    match color {
        ColorType::Rgba => {
            match bit_depth {
                BitDepth::Eight => Ok(buf.to_vec()),
                BitDepth::Sixteen => {
                    // downsample: RGBA 16-bit -> take the high byte of each 16-bit sample
                    // 8 bytes per pixel (R_hi,R_lo,G_hi,G_lo,B_hi,B_lo,A_hi,A_lo)
                    let mut out = Vec::with_capacity(buf.len() / 2);
                    for px in buf.chunks_exact(8) {
                        out.push(px[0]); out.push(px[2]); out.push(px[4]); out.push(px[6]);
                    }
                    Ok(out)
                }
                _ => Err(anyhow!("unsupported bit depth for Rgba: {:?}", bit_depth).into())
            }
        }
        ColorType::Indexed => {
            // buf contains indices, we only support 8-bit for wplace
            let pixel_count = (info.width as usize) * (info.height as usize);
            let indices = match bit_depth {
                BitDepth::Eight => buf.to_vec(),
                _ => return Err(anyhow!("unsupported bit depth for Indexed: {:?}", bit_depth).into()),
            };

            // palette present in info.palette as RGB triples
            let palette = info.palette.as_ref().ok_or(anyhow!("indexed PNG without palette"))?;
            let trns = info.trns.as_ref();
            let mut out = Vec::with_capacity(pixel_count * 4);
            for &idx in indices.iter() {
                let i = idx as usize;
                let base = i * 3;
                if base + 2 >= palette.len() {
                    // default to magenta/debug
                    out.push(255); out.push(0); out.push(255);
                } else {
                    out.push(palette[base]);
                    out.push(palette[base + 1]);
                    out.push(palette[base + 2]);
                }
                let a = trns.and_then(|t| t.get(i)).cloned().unwrap_or(255);
                out.push(a);
            }
            Ok(out)
        }
        _ => Err(anyhow!("unsupported color type: {:?}", color).into())
    }
}

