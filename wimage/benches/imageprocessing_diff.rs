use criterion::{black_box, criterion_group, criterion_main, Criterion};
use wimage::{PalettedImage, diff_paletted, apply_diff_paletted};

/// Create a 1000x1000 image with all pixels set to the same value
fn create_uniform_image(size: usize, pixel_value: u8) -> PalettedImage {
    PalettedImage {
        width: size,
        height: size,
        indices: vec![pixel_value; size * size],
    }
}

/// Create an image that's 20% different from a base image
fn create_20percent_different(base: &PalettedImage) -> PalettedImage {
    let mut modified = base.clone();
    let total_pixels = base.width * base.height;
    
    // Change every 5th pixel (approximately 20%)
    for i in (0..total_pixels).step_by(5) {
        // Change to a different value (just increment, wrapping at 254 to avoid DIFF_NO_CHANGE)
        modified.indices[i] = (modified.indices[i].wrapping_add(1)) % 254;
    }
    
    modified
}

fn benchmark_diff_paletted(c: &mut Criterion) {
    let size = 1000;
    let mut group = c.benchmark_group("diff_paletted");
    
    // Case 1: Identical images
    group.bench_function("identical_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 42));
        let new = black_box(create_uniform_image(size, 42));
        
        b.iter(|| {
            diff_paletted(&base, &new)
        });
    });
    
    // Case 2: Completely different images
    group.bench_function("fully_different_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 10));
        let new = black_box(create_uniform_image(size, 200));
        
        b.iter(|| {
            diff_paletted(&base, &new)
        });
    });
    
    // Case 3: 20% different
    group.bench_function("20percent_different_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 50));
        let modified = black_box(create_20percent_different(&base));
        
        b.iter(|| {
            diff_paletted(&base, &modified)
        });
    });
    
    group.finish();
}

fn benchmark_apply_diff_paletted(c: &mut Criterion) {
    let size = 1000;
    let mut group = c.benchmark_group("apply_diff_paletted");
    
    // Case 1: Apply diff from identical images (no actual changes)
    group.bench_function("apply_identical_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 42));
        let new = black_box(create_uniform_image(size, 42));
        let (_, diff) = diff_paletted(&base, &new);
        let diff = black_box(diff);
        
        b.iter(|| {
            apply_diff_paletted(&base, &diff)
        });
    });
    
    // Case 2: Apply diff from fully different images
    group.bench_function("apply_fully_different_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 10));
        let new = black_box(create_uniform_image(size, 200));
        let (_, diff) = diff_paletted(&base, &new);
        let diff = black_box(diff);
        
        b.iter(|| {
            apply_diff_paletted(&base, &diff)
        });
    });
    
    // Case 3: Apply diff from 20% different image
    group.bench_function("apply_20percent_different_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 50));
        let modified = black_box(create_20percent_different(&base));
        let (_, diff) = diff_paletted(&base, &modified);
        let diff = black_box(diff);
        
        b.iter(|| {
            apply_diff_paletted(&base, &diff)
        });
    });
    
    group.finish();
}

fn benchmark_roundtrip(c: &mut Criterion) {
    let size = 1000;
    let mut group = c.benchmark_group("roundtrip_diff_apply");
    
    // Roundtrip: diff and then apply
    group.bench_function("roundtrip_20percent_different_1000x1000", |b| {
        let base = black_box(create_uniform_image(size, 50));
        let modified = black_box(create_20percent_different(&base));
        
        b.iter(|| {
            let (_, diff) = diff_paletted(&base, &modified);
            apply_diff_paletted(&base, &diff)
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_diff_paletted,
    benchmark_apply_diff_paletted,
    benchmark_roundtrip
);
criterion_main!(benches);
