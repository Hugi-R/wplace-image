use crate::{PalettedImage};

impl PalettedImage {
    pub fn downscale_mode_weighted(&self, weights: &[u32; 256], block_size: usize) -> PalettedImage {
        if block_size == 2 {
            // faster
            downscale_mode_weighted_2x2(self, weights)
        } else {
            downscale_mode_weighted(self, weights, block_size)
        }
    }
}

fn downscale_mode_weighted(
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

fn downscale_mode_weighted_2x2(
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