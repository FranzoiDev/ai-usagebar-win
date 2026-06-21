//! Tray icon bitmap generation. We draw the icon in code (no asset files) so
//! the binary is self-contained. The fill color encodes the worst-case
//! severity, giving an at-a-glance signal in the notification area.

use tray_icon::Icon;

use crate::usage::Severity;

const SIZE: u32 = 32;

fn rgb(severity: Severity) -> (u8, u8, u8) {
    match severity {
        Severity::Low => (0x4c, 0xaf, 0x50),      // green
        Severity::Mid => (0xff, 0xc1, 0x07),      // amber
        Severity::High => (0xff, 0x98, 0x00),     // orange
        Severity::Critical => (0xf4, 0x43, 0x36), // red
    }
}

/// Build a 32x32 rounded-square icon tinted by severity.
pub fn icon_for(severity: Severity) -> Option<Icon> {
    let (r, g, b) = rgb(severity);
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let n = SIZE as i32;
    let radius = 4i32; // corner rounding
    for y in 0..n {
        for x in 0..n {
            let transparent = in_corner(x, y, n, radius);
            if transparent {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            // 2px darker border for definition against any taskbar color.
            let border = x < 2 || y < 2 || x >= n - 2 || y >= n - 2;
            if border {
                rgba.extend_from_slice(&[r / 2, g / 2, b / 2, 255]);
            } else {
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).ok()
}

/// True for pixels outside the rounded corners (made transparent).
fn in_corner(x: i32, y: i32, n: i32, radius: i32) -> bool {
    let corners = [
        (radius, radius),
        (n - 1 - radius, radius),
        (radius, n - 1 - radius),
        (n - 1 - radius, n - 1 - radius),
    ];
    for (cx, cy) in corners {
        let near_x = (x < radius && cx == radius) || (x > n - 1 - radius && cx == n - 1 - radius);
        let near_y = (y < radius && cy == radius) || (y > n - 1 - radius && cy == n - 1 - radius);
        if near_x && near_y {
            let dx = (x - cx) as f32;
            let dy = (y - cy) as f32;
            if dx * dx + dy * dy > (radius as f32) * (radius as f32) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_builds_for_each_severity() {
        for s in [
            Severity::Low,
            Severity::Mid,
            Severity::High,
            Severity::Critical,
        ] {
            assert!(icon_for(s).is_some(), "icon failed for {s:?}");
        }
    }
}
