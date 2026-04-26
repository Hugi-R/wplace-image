use crate::{PalettedImage};

impl PalettedImage {
    /// Downscale the image by a factor of `block_size` using a weighted mode of the pixels in each block.
    pub fn downscale_mode_weighted(&self, weights: &[u32; 256], block_size: usize) -> PalettedImage {
        if block_size == 2 {
            // faster
            downscale_mode_weighted_2x2(self, weights)
        } else {
            downscale_mode_weighted(self, weights, block_size)
        }
    }

    /// Create a diff paletted image: pixels that are the same in `self` and `new` are set to DIFF_COLOR,
    /// pixels that differ are taken from `new`.
    /// This allows to store only the differences between two images.
    pub fn diff(&self, new: &PalettedImage) -> (bool, PalettedImage) {
        diff_paletted(self, new)
    }

    /// Apply a diff paletted image (produced by `diff`) to `self`, producing the updated paletted image.
    /// Essentially an uncompressing of the diff.
    pub fn apply_diff(&self, diff: &PalettedImage) -> PalettedImage {
        apply_diff_paletted(self, diff)
    }
}

/// Create a diff paletted image: pixels that are the same in `base` and `new` are set to DIFF_COLOR,
/// pixels that differ are taken from `new`.
/// This allows to store only the differences between two images.
/// ZSTD can achieve significant compression if the images are similar. (Lots of DIFF_COLOR)
pub fn diff_paletted(base: &PalettedImage, new: &PalettedImage) -> (bool, PalettedImage) {
    assert!(base.width == new.width && base.height == new.height);

    let mut any_diff = false;
    let indices: Vec<u8> = base.indices.iter()
        .zip(new.indices.iter())
        .map(|(b, n)| if b == n { crate::palette::DIFF_NO_CHANGE } else { any_diff = true; *n })
        .collect();

    (any_diff, PalettedImage { width: base.width, height: base.height, indices })
}

/// Apply a diff paletted image (produced by `diff_paletted`) to a base paletted image,
/// producing the updated paletted image.
/// Essentially an uncompressing of the diff.
/// Branchless using bitwise operations and masking. This is significantly faster (~10x)
pub fn apply_diff_paletted(base: &PalettedImage, diff: &PalettedImage) -> PalettedImage {
    assert!(base.width == diff.width && base.height == diff.height);

    let indices = base.indices.iter()
        .zip(diff.indices.iter())
        .map(|(b, d)| {
            let is_no_change = (*d == crate::palette::DIFF_NO_CHANGE) as u8;
            let mask = is_no_change.wrapping_neg(); // 0xFF if no-change, 0x00 if changed
            (b & mask) | (d & !mask)
        })
        .collect();

    PalettedImage { width: base.width, height: base.height, indices }
}

/// Downscale the image by a factor of `block_size` using a weighted mode of the pixels in each block.
pub fn downscale_mode_weighted(
    img: &PalettedImage,
    weights: &[u32; 256],
    block_size: usize,
) -> PalettedImage {
    let src_w = img.width;
    let src_h = img.height;
    let src_idx = &img.indices;
    assert!(block_size >= 2);
    assert!(block_size <= 8);
    assert!(src_h % block_size == 0);
    assert!(src_w % block_size == 0);
    let out_w = src_w / block_size;
    let out_h = src_h / block_size;

    let mut out = vec![0u8; out_w * out_h];

    // Reused scratch (small, cache-friendly)
    let mut scores = [0u32; 256];
    let mut stamp = [0u32; 256];
    let mut cur_stamp: u32 = 1;

    for oy in 0..out_h {
        let sy0 = oy * block_size;
        for ox in 0..out_w {
            let sx0 = ox * block_size;

            let mut touched = [0u8; 64]; // worst case all different
            let mut touched_len = 0usize;

            // Vote over the block
            for dy in 0..block_size {
                let row = (sy0 + dy) * src_w;
                let base = row + sx0;
                for dx in 0..block_size {
                    let idx = src_idx[base + dx] as usize;

                    if stamp[idx] != cur_stamp {
                        stamp[idx] = cur_stamp;
                        scores[idx] = weights[idx];
                        touched[touched_len] = idx as u8;
                        touched_len += 1;
                    } else {
                        scores[idx] += weights[idx];
                    }
                }
            }

            // Argmax over touched indices only
            let mut best = touched[0] as usize;
            let mut best_score = scores[best];
            for i in 1..touched_len {
                let c = touched[i] as usize;
                let s = scores[c];
                // tie-break: keep existing, or pick lower index, or center pixel, your choice
                if s > best_score {
                    best = c;
                    best_score = s;
                }
            }

            out[oy * out_w + ox] = best as u8;
            cur_stamp = cur_stamp.wrapping_add(1);
            if cur_stamp == 0 {
                // extremely unlikely here, but keep it safe
                stamp.fill(0);
                cur_stamp = 1;
            }
        }
    }

    PalettedImage {
        width: out_w,
        height: out_h,
        indices: out,
    }
}

/// Specialized version of downscale_mode_weighted for block_size = 2, which is a common case and can be optimized.
pub fn downscale_mode_weighted_2x2(
    src: &PalettedImage,
    weights: &[u32; 256],
) -> PalettedImage {
    assert!(src.height % 2 == 0);
    assert!(src.width % 2 == 0);
    let out_w = src.width / 2;
    let out_h = src.height / 2;

    let mut out = vec![0u8; out_w * out_h];

    for oy in 0..out_h {
        let sy0 = oy * 2;
        let row0_base = sy0 * src.width;
        let row1_base = (sy0 + 1) * src.width;
        for ox in 0..out_w {
            let sx0 = ox * 2;

            let mut scores = [0u32; 4];
            let colors = [
                src.indices[row0_base + sx0],
                src.indices[row0_base + sx0 + 1],
                src.indices[row1_base + sx0],
                src.indices[row1_base + sx0 + 1],
            ];

            scores[0] = weights[colors[0] as usize];

            if colors[1] == colors[0] {
                scores[0] += weights[colors[1] as usize];
            } else {
                scores[1] = weights[colors[1] as usize];
            }

            if colors[2] == colors[0] {
                scores[0] += weights[colors[2] as usize];
            } else if colors[2] == colors[1] {
                scores[1] += weights[colors[2] as usize];
            } else {
                scores[2] = weights[colors[2] as usize];
            }

             if colors[3] == colors[0] {
                scores[0] += weights[colors[3] as usize];
            } else if colors[3] == colors[1] {
                scores[1] += weights[colors[3] as usize];
            } else if colors[3] == colors[2] {
                scores[2] += weights[colors[3] as usize];
            } else {
                scores[3] = weights[colors[3] as usize];
            }

            out[oy * out_w + ox] = colors[argmax(&scores)];
        }
    }

    PalettedImage {
        width: out_w,
        height: out_h,
        indices: out,
    }
}

fn argmax(a: &[u32]) -> usize {
    let mut best_idx = 0usize;
    let mut best_val = a[0];
    for (i, &v) in a.iter().enumerate().skip(1) {
        if v > best_val {
            best_val = v;
            best_idx = i;
        }
    }
    best_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a test image with a specific size and initial pixel value
    fn create_test_image(width: usize, height: usize, pixel_value: u8) -> PalettedImage {
        PalettedImage {
            width,
            height,
            indices: vec![pixel_value; width * height],
        }
    }

    /// Helper to get a diff constant that won't conflict with regular pixel values
    fn diff_no_change() -> u8 {
        crate::palette::DIFF_NO_CHANGE
    }

    // ============= Tests for diff_paletted =============

    #[test]
    fn test_diff_paletted_identical_images() {
        let base = create_test_image(10, 10, 42);
        let new = create_test_image(10, 10, 42);

        let (any_diff, diff) = diff_paletted(&base, &new);

        assert!(!any_diff, "Expected no differences for identical images");
        assert_eq!(diff.width, 10);
        assert_eq!(diff.height, 10);
        // All pixels should be marked as DIFF_NO_CHANGE
        assert!(diff.indices.iter().all(|&x| x == diff_no_change()));
    }

    #[test]
    fn test_diff_paletted_completely_different() {
        let base = create_test_image(10, 10, 0);
        let new = create_test_image(10, 10, 100);

        let (any_diff, diff) = diff_paletted(&base, &new);

        assert!(any_diff, "Expected differences for completely different images");
        assert_eq!(diff.width, 10);
        assert_eq!(diff.height, 10);
        // All pixels should be 100 (taken from 'new')
        assert!(diff.indices.iter().all(|&x| x == 100));
    }

    #[test]
    fn test_diff_paletted_partial_differences() {
        let base = create_test_image(4, 4, 10);
        let mut new = create_test_image(4, 4, 10);

        // Change some pixels in 'new'
        new.indices[0] = 20;
        new.indices[5] = 30;
        new.indices[15] = 40;

        let (any_diff, diff) = diff_paletted(&base, &new);

        assert!(any_diff, "Expected differences");
        assert_eq!(diff.indices[0], 20, "Different pixel should be from 'new'");
        assert_eq!(diff.indices[5], 30, "Different pixel should be from 'new'");
        assert_eq!(diff.indices[15], 40, "Different pixel should be from 'new'");
        
        // Check unchanged pixels are marked with DIFF_NO_CHANGE
        for i in 0..16 {
            if i != 0 && i != 5 && i != 15 {
                assert_eq!(
                    diff.indices[i], 
                    diff_no_change(),
                    "Unchanged pixel at index {} should be DIFF_NO_CHANGE", i
                );
            }
        }
    }

    #[test]
    fn test_diff_paletted_large_image() {
        let base = create_test_image(1000, 1000, 42);
        let mut new = base.clone();
        new.indices[500000] = 99; // Change one pixel in the middle

        let (any_diff, diff) = diff_paletted(&base, &new);

        assert!(any_diff);
        assert_eq!(diff.indices[500000], 99);
        for i in 0..(1000 * 1000) {
            if i != 500000 {
                assert_eq!(
                    diff.indices[i], 
                    diff_no_change(),
                    "Unchanged pixel at index {} should be DIFF_NO_CHANGE", i
                );
            }
        }
    }

    // ============= Tests for apply_diff_paletted =============

    #[test]
    fn test_apply_diff_paletted_no_changes() {
        let base = create_test_image(10, 10, 42);
        let diff = create_test_image(10, 10, diff_no_change());

        let result = apply_diff_paletted(&base, &diff);

        assert_eq!(result.width, 10);
        assert_eq!(result.height, 10);
        // Result should match base since diff indicates no changes
        assert_eq!(result.indices, base.indices);
    }

    #[test]
    fn test_apply_diff_paletted_all_changes() {
        let base = create_test_image(10, 10, 10);
        let diff = create_test_image(10, 10, 50);

        let result = apply_diff_paletted(&base, &diff);

        assert_eq!(result.width, 10);
        assert_eq!(result.height, 10);
        // Result should match diff since all are marked as changes
        assert!(result.indices.iter().all(|&x| x == 50));
    }

    #[test]
    fn test_apply_diff_paletted_partial() {
        let base = create_test_image(4, 4, 10);
        let mut diff = create_test_image(4, 4, diff_no_change());

        // Set some pixels to indicate changes
        diff.indices[0] = 20;
        diff.indices[5] = 30;
        diff.indices[15] = 40;

        let result = apply_diff_paletted(&base, &diff);

        assert_eq!(result.indices[0], 20);
        assert_eq!(result.indices[5], 30);
        assert_eq!(result.indices[15], 40);
        
        // Unchanged pixels should use base values
        for i in 0..16 {
            if i != 0 && i != 5 && i != 15 {
                assert_eq!(
                    result.indices[i], 
                    10,
                    "Unchanged pixel at index {} should be from base", i
                );
            }
        }
    }

    // ============= Roundtrip Tests =============

    #[test]
    fn test_roundtrip_identical_images() {
        let base = create_test_image(10, 10, 42);
        let new = create_test_image(10, 10, 42);

        let (_, diff) = diff_paletted(&base, &new);
        let reconstructed = apply_diff_paletted(&base, &diff);

        assert_eq!(reconstructed.width, new.width);
        assert_eq!(reconstructed.height, new.height);
        assert_eq!(reconstructed.indices, new.indices);
    }

    #[test]
    fn test_roundtrip_completely_different() {
        let base = create_test_image(10, 10, 10);
        let new = create_test_image(10, 10, 200);

        let (_, diff) = diff_paletted(&base, &new);
        let reconstructed = apply_diff_paletted(&base, &diff);

        assert_eq!(reconstructed.indices, new.indices);
    }

    #[test]
    fn test_roundtrip_partial_changes() {
        let base = create_test_image(100, 100, 50);
        let mut new = base.clone();

        // Change 20 random pixels
        for i in (0..2000).step_by(100) {
            new.indices[i] = ((i % 256) as u8).wrapping_add(1);
        }

        let (_, diff) = diff_paletted(&base, &new);
        let reconstructed = apply_diff_paletted(&base, &diff);

        assert_eq!(reconstructed.indices, new.indices);
    }

    #[test]
    fn test_roundtrip_large_image() {
        let base = create_test_image(1000, 1000, 42);
        let mut new = base.clone();
        
        // Modify several pixels at various locations
        new.indices[0] = 10;
        new.indices[500000] = 100;
        new.indices[999999] = 200;

        let (_, diff) = diff_paletted(&base, &new);
        let reconstructed = apply_diff_paletted(&base, &diff);

        assert_eq!(reconstructed.indices, new.indices);
    }
}