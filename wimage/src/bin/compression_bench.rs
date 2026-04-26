use std::fs;
use std::hint::black_box;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use wimage::{PalettedImage};
use wimage::image::{paletted_to_compressed_bytes_level};

const REPEAT: usize = 10;

fn average_duration<F: FnMut()>(mut f: F, repeat: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..repeat {
        f();
    }
    let elapsed = start.elapsed();
    elapsed.as_secs_f64() * 1000.0 / repeat as f64
}

fn main() -> anyhow::Result<()> {
    let testdata_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata");
    let mut entries: Vec<_> = fs::read_dir(&testdata_dir)
        .with_context(|| format!("reading testdata directory {}", testdata_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        })
        .collect();

    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        let bytes = fs::read(&path).with_context(|| format!("reading png file {}", path.display()))?;
        let png_size = bytes.len();
        let paletted = PalettedImage::from_png(Cursor::new(bytes))?;

        println!("\nFile: {} ({}x{} -> {} indices, png_size: {} bytes)", name, paletted.width, paletted.height, paletted.indices.len(), png_size);
        println!("level,size_bytes,compression_ratio,avg_ms");

        for level in 0..=17 {
            let compressed = paletted_to_compressed_bytes_level(&paletted, level)?;
            let size = compressed.len();
            black_box(&compressed);

            let avg_ms = average_duration(
                || {
                    let compressed = paletted_to_compressed_bytes_level(&paletted, level).unwrap();
                    black_box(&compressed);
                },
                REPEAT,
            );

            println!("{},{},{:.4},{}", level, size, size as f64 / png_size as f64, avg_ms);
        }
    }

    Ok(())
}
