use std::fs;
use std::hint::black_box;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use wimage::{downscale_mode_weighted, downscale_mode_weighted_2x2, PalettedImage};

const REPEAT: usize = 20;
const BLOCK_SIZES: [usize; 2] = [2, 4];

fn average_duration<F: FnMut()>(mut f: F, repeat: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..repeat {
        f();
    }
    let elapsed = start.elapsed();
    elapsed.as_secs_f64() * 1000.0 / repeat as f64
}

fn make_weights() -> [u32; 256] {
    let mut weights = [0u32; 256];
    for i in 0..256 {
        weights[i] = 100;
    }
    weights[0] = 0; // ignore transparent pixels 
    weights
}

fn main() -> anyhow::Result<()> {
    let weights = make_weights();
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

    println!("file,method,block_size,out_pixels,avg_ms");

    for entry in entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy();
        let bytes = fs::read(&path).with_context(|| format!("reading png file {}", path.display()))?;
        let paletted = PalettedImage::from_png(Cursor::new(bytes))?;

        for &block_size in BLOCK_SIZES.iter() {
            let out_pixels = paletted.width / block_size * paletted.height / block_size;
            let avg_ms = average_duration(
                || {
                    let out = downscale_mode_weighted(&paletted, &weights, block_size);
                    black_box(&out);
                },
                REPEAT,
            );

            println!(
                "{},{},{},{},{:.4}",
                name, "downscale_mode_weighted", block_size, out_pixels, avg_ms
            );
        }

        let out_pixels = paletted.width / 2 * paletted.height / 2;
        let avg_ms = average_duration(
            || {
                let out = downscale_mode_weighted_2x2(&paletted, &weights);
                black_box(&out);
            },
            REPEAT,
        );

        println!(
            "{},{},{},{},{:.4}",
            name, "downscale_mode_weighted_2x2", 2, out_pixels, avg_ms
        );
    }

    Ok(())
}
