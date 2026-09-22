use tray_icon::Icon;

// 5x7 bit patterns for digits 0-9
// Each byte represents a row of 5 bits (bits 4..0)
const DIGITS_5X7: [[u8; 7]; 10] = [
    [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110], // 0
    [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110], // 1
    [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111], // 2
    [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110], // 3
    [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010], // 4
    [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110], // 5
    [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110], // 6
    [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000], // 7
    [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110], // 8
    [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100], // 9
];

/// Generates a 32x32 RGBA system tray icon displaying a 2-digit number (0-99).
/// Shows numbers with high visibility and subtle badge backing.
pub fn generate_stat_icon(value: u32) -> Result<Icon, String> {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let clamped = value.min(99);
    let d1 = (clamped / 10) as usize;
    let d2 = (clamped % 10) as usize;

    // Determine foreground text color based on load
    let (fg_r, fg_g, fg_b) = if clamped >= 85 {
        (239, 68, 68) // Crimson Red #EF4444
    } else if clamped >= 70 {
        (245, 158, 11) // Amber #F59E0B
    } else {
        (56, 189, 248) // Sky Blue #38BDF8
    };

    // Draw background rounded pill
    for y in 0..height {
        for x in 0..width {
            let cx = x as f32 - 15.5;
            let cy = y as f32 - 15.5;
            let dist = (cx * cx + cy * cy).sqrt();

            let idx = ((y * width + x) * 4) as usize;

            if dist <= 14.5 {
                // Dark background pill #18181B so digits pop on any taskbar theme
                rgba[idx] = 24;
                rgba[idx + 1] = 24;
                rgba[idx + 2] = 27;
                rgba[idx + 3] = 240;

                // Subtle edge highlight
                if dist >= 13.0 {
                    rgba[idx] = 39;
                    rgba[idx + 1] = 39;
                    rgba[idx + 2] = 42;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }

    // Draw two 5x7 digits scaled 2x (10x14 pixels each)
    // Left digit: x offset = 5, y offset = 9
    // Right digit: x offset = 17, y offset = 9
    draw_digit(&mut rgba, width, 5, 9, d1, fg_r, fg_g, fg_b);
    draw_digit(&mut rgba, width, 17, 9, d2, fg_r, fg_g, fg_b);

    Icon::from_rgba(rgba, width, height).map_err(|e| format!("Failed to create icon: {:?}", e))
}

fn draw_digit(
    rgba: &mut [u8],
    canvas_w: u32,
    offset_x: u32,
    offset_y: u32,
    digit: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    let pattern = DIGITS_5X7[digit % 10];

    for (row_idx, &row_bits) in pattern.iter().enumerate() {
        for col_idx in 0..5 {
            let is_set = (row_bits & (1 << (4 - col_idx))) != 0;
            if is_set {
                // Scale 2x2
                for dy in 0..2 {
                    for dx in 0..2 {
                        let px = offset_x + (col_idx * 2) + dx;
                        let py = offset_y + (row_idx as u32 * 2) + dy;

                        let idx = ((py * canvas_w + px) * 4) as usize;
                        if idx + 3 < rgba.len() {
                            rgba[idx] = r;
                            rgba[idx + 1] = g;
                            rgba[idx + 2] = b;
                            rgba[idx + 3] = 255;
                        }
                    }
                }
            }
        }
    }
}
