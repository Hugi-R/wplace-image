use criterion::{black_box, criterion_group, criterion_main, Criterion};

use wimage::palette::{index_from_rgba, rgba_from_index, PALETTE_SIZE};

/// Build a buffer of RGBA pixels sampled from the palette (like decoded PNGs).
fn palette_pixels(count: usize) -> Vec<[u8; 4]> {
    (0..count).map(|i| rgba_from_index((i % PALETTE_SIZE) as u8)).collect()
}

/// Build a buffer that mixes palette colors with unknown colors at ~50/50.
fn mixed_pixels(count: usize) -> Vec<[u8; 4]> {
    (0..count)
        .map(|i| {
            if i % 2 == 0 {
                rgba_from_index((i % PALETTE_SIZE) as u8)
            } else {
                [i as u8, (i >> 8) as u8, (i >> 16) as u8, (i >> 24) as u8]
            }
        })
        .collect()
}

fn lookup_all(buf: &[[u8; 4]]) -> u32 {
    let mut idx = 0u32;
    for rgba in buf {
        idx = (idx << 1).wrapping_add(u32::from(index_from_rgba(*rgba)));
    }
    idx
}

fn bench(c: &mut Criterion) {
    let pixels = palette_pixels(1 << 20);
    let mixed = mixed_pixels(1 << 20);

    let mut group = c.benchmark_group("index_from_rgba");
    group.throughput(criterion::Throughput::Elements(pixels.len() as u64));
    group.bench_function("palette_only", |b| b.iter(|| black_box(lookup_all(black_box(&pixels)))));
    group.bench_function("mixed_unknown", |b| b.iter(|| black_box(lookup_all(black_box(&mixed)))));
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);