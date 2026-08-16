/// Signature: `INDEX => (R, G, B)`.
///
/// Generates:
/// - `PALETTE`: index -> RGBA. Unlisted indices default to magenta (debug), except
///   `TRANSPARENT`, `DEBUG_COLOR` and `DIFF_NO_CHANGE` which are set specially.
/// - `index_from_rgba`: constant-time reverse lookup via `match`.
macro_rules! palette {
    ( $( $idx:expr => ($r:expr, $g:expr, $b:expr) ),* $(,)? ) => {
        pub const PALETTE: [[u8; 4]; PALETTE_SIZE] = {
            let mut a = [[255u8, 0, 255, 255]; PALETTE_SIZE];
            a[TRANSPARENT as usize] = [0, 0, 0, 0];
            a[DEBUG_COLOR as usize] = [255, 0, 255, 255];
            a[DIFF_NO_CHANGE as usize] = [255, 1, 255, 255];
            $(
                a[$idx as usize] = [$r as u8, $g as u8, $b as u8, 255];
            )*
            a
        };

        /// Reverse lookup: RGBA -> index. Unknown RGBA -> `DEBUG_COLOR` (255).
        pub const fn index_from_rgba(rgba: [u8; 4]) -> u8 {
            match rgba {
                [0, 0, 0, 0] => TRANSPARENT,
                [255, 1, 255, 255] => DIFF_NO_CHANGE,
                $(
                    [$r, $g, $b, 255] => $idx as u8,
                )*
                _ => DEBUG_COLOR,
            }
        }
    };
}

pub const PALETTE_SIZE: usize = 256;
pub const TRANSPARENT: u8 = 0u8;
pub const BLACK: u8 = 1u8;
pub const WHITE: u8 = 5u8;
pub const DEBUG_COLOR: u8 = 255u8;
pub const DIFF_NO_CHANGE: u8 = 254u8;

palette! {
    BLACK => (0, 0, 0), // Black
    2 => (60, 60, 60), // Dark Gray
    3 => (120, 120, 120), // Gray
    4 => (210, 210, 210), // Light Gray
    WHITE => (255, 255, 255), // White
    6 => (96, 0, 24), // Deep Red
    7 => (237, 28, 36), // Red
    8 => (255, 127, 39), // Orange
    9 => (246, 170, 9), // Gold
    10 => (249, 221, 59), // Yellow
    11 => (255, 250, 188), // Light Yellow
    12 => (14, 185, 104), // Dark Green
    13 => (19, 230, 123), // Green
    14 => (135, 255, 94), // Light Green
    15 => (12, 129, 110), // Dark Teal
    16 => (16, 174, 166), // Teal
    17 => (19, 225, 190), // Light Teal
    18 => (40, 80, 158), // Dark Blue
    19 => (64, 147, 228), // Blue
    20 => (96, 247, 242), // Cyan
    21 => (107, 80, 246), // Indigo
    22 => (153, 177, 251), // Light Indigo
    23 => (120, 12, 153), // Dark Purple
    24 => (170, 56, 185), // Purple
    25 => (224, 159, 249), // Light Purple
    26 => (203, 0, 122), // Dark Pink
    27 => (236, 31, 128), // Pink
    28 => (243, 141, 169), // Light Pink
    29 => (104, 70, 52), // Dark Brown
    30 => (149, 104, 42), // Brown
    31 => (248, 178, 119), // Beige
    32 => (170, 170, 170), // Medium Gray
    33 => (165, 14, 30), // Dark Red
    34 => (250, 128, 114), // Light Red
    35 => (228, 92, 26), // Dark Orange
    36 => (214, 181, 148), // Light Tan
    37 => (156, 132, 49), // Dark Goldenrod
    38 => (197, 173, 49), // Goldenrod
    39 => (232, 212, 95), // Light Goldenrod
    40 => (74, 107, 58), // Dark Olive
    41 => (90, 148, 74), // Olive
    42 => (132, 197, 115), // Light Olive
    43 => (15, 121, 159), // Dark Cyan
    44 => (187, 250, 242), // Light Cyan
    45 => (125, 199, 255), // Light Blue
    46 => (77, 49, 184), // Dark Indigo
    47 => (74, 66, 132), // Dark Slate Blue
    48 => (122, 113, 196), // Slate Blue
    49 => (181, 174, 241), // Light Slate Blue
    50 => (219, 164, 99), // Light Brown
    51 => (209, 128, 81), // Dark Beige
    52 => (255, 197, 165), // Light Beige
    53 => (155, 82, 73), // Dark Peach
    54 => (209, 128, 120), // Peach
    55 => (250, 182, 164), // Light Peach
    56 => (123, 99, 82), // Dark Tan
    57 => (156, 132, 107), // Tan
    58 => (51, 57, 65), // Dark Slate
    59 => (109, 117, 141), // Slate
    60 => (179, 185, 209), // Light Slate
    61 => (109, 100, 63), // Dark Stone
    62 => (148, 140, 107), // Stone
    63 => (205, 197, 158), // Light Stone
}

/// Get RGBA for palette index. index 0 is transparent.
pub const fn rgba_from_index(i: u8) -> [u8; 4] {
    PALETTE[i as usize]
}

/// Palette with the diff-marker color made transparent.
pub const PALETTE_NO_DIFF: [[u8; 4]; PALETTE_SIZE] = {
    let mut a = PALETTE;
    a[DIFF_NO_CHANGE as usize] = [0, 0, 0, 0];
    a
};

const fn png_palette(ignore_diff: bool) -> ([u8; PALETTE_SIZE * 3], [u8; PALETTE_SIZE]) {
    let pal = if ignore_diff { &PALETTE_NO_DIFF } else { &PALETTE };
    let mut palette_bytes = [0u8; PALETTE_SIZE * 3];
    let mut trns = [0u8; PALETTE_SIZE];

    let mut i = 0;
    while i < PALETTE_SIZE {
        let rgba = pal[i];
        palette_bytes[i * 3] = rgba[0];
        palette_bytes[i * 3 + 1] = rgba[1];
        palette_bytes[i * 3 + 2] = rgba[2];
        trns[i] = rgba[3];
        i += 1;
    }
    (palette_bytes, trns)
}

pub const PNG_PALETTE: ([u8; PALETTE_SIZE * 3], [u8; PALETTE_SIZE]) = png_palette(false);
pub const PNG_PALETTE_NO_DIFF: ([u8; PALETTE_SIZE * 3], [u8; PALETTE_SIZE]) = png_palette(true);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_defined_colors() {
        for i in 0..64 {
            assert_eq!(index_from_rgba(rgba_from_index(i)), i as u8);
        }
        assert_eq!(index_from_rgba(rgba_from_index(DIFF_NO_CHANGE)), DIFF_NO_CHANGE);
        assert_eq!(index_from_rgba(rgba_from_index(DEBUG_COLOR)), DEBUG_COLOR);
    }

    #[test]
    fn undefined_indices_fall_back_to_debug() {
        for i in 64..=253 {
            assert_eq!(rgba_from_index(i), [255, 0, 255, 255]);
            assert_eq!(index_from_rgba(rgba_from_index(i)), DEBUG_COLOR);
        }
    }

    #[test]
    fn transparent_and_black_distinct() {
        assert_eq!(index_from_rgba([0, 0, 0, 0]), TRANSPARENT);
        assert_eq!(index_from_rgba([0, 0, 0, 255]), BLACK);
        assert_eq!(index_from_rgba([255, 1, 255, 255]), DIFF_NO_CHANGE);
    }

    #[test]
    fn unknown_returns_debug() {
        assert_eq!(index_from_rgba([123, 45, 67, 255]), DEBUG_COLOR);
        assert_eq!(index_from_rgba([0, 0, 0, 128]), DEBUG_COLOR);
    }
}