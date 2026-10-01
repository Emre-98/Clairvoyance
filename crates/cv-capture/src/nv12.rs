//! NV12 (what hardware video decoders output) → RGB for a small region, the same way on every
//! platform so the ability-bar thresholds measured on one decoder hold on the other.
//!
//! Recordings carry no colour tags; like ffmpeg's default for untagged video this uses BT.601
//! limited range (the ult detector's thresholds were measured that way).

use cv_core::game::{Region, Rgb};

/// The box to copy out of a decoded NV12 frame for `r`: grown to even coordinates (chroma is
/// shared by 2×2 pixels), clamped to the frame.
pub fn aligned_box(r: Region, frame_w: u32, frame_h: u32) -> Region {
    let x0 = r.x & !1;
    let y0 = r.y & !1;
    let x1 = ((r.x + r.w + 1) & !1).min(frame_w & !1);
    let y1 = ((r.y + r.h + 1) & !1).min(frame_h & !1);
    Region { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
}

#[inline]
fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Converts `r` (frame coordinates) from an NV12 box that starts at `bx` (frame coordinates,
/// from [`aligned_box`]). `y_plane`/`uv_plane` start at the box's first row.
pub fn to_rgb(y_plane: &[u8], y_pitch: usize, uv_plane: &[u8], uv_pitch: usize, bx: Region, r: Region) -> Rgb {
    let mut data = Vec::with_capacity((r.w * r.h * 3) as usize);
    for row in 0..r.h {
        let ly = (r.y + row - bx.y) as usize;
        for col in 0..r.w {
            let lx = (r.x + col - bx.x) as usize;
            let yv = y_plane.get(ly * y_pitch + lx).copied().unwrap_or(16) as i32;
            let ci = (ly / 2) * uv_pitch + (lx / 2) * 2;
            let u = uv_plane.get(ci).copied().unwrap_or(128) as i32 - 128;
            let v = uv_plane.get(ci + 1).copied().unwrap_or(128) as i32 - 128;
            let c = (yv - 16) * 298;
            data.push(clamp((c + 409 * v + 128) >> 8));
            data.push(clamp((c - 100 * u - 208 * v + 128) >> 8));
            data.push(clamp((c + 516 * u + 128) >> 8));
        }
    }
    Rgb { w: r.w, h: r.h, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_and_colours() {
        let b = aligned_box(Region { x: 939, y: 991, w: 43, h: 43 }, 1920, 1080);
        assert_eq!(b, Region { x: 938, y: 990, w: 44, h: 44 });
        assert_eq!(aligned_box(Region { x: 1900, y: 1070, w: 30, h: 30 }, 1920, 1080), Region { x: 1900, y: 1070, w: 20, h: 10 });
        // 2×2 box: white, black, and a blue chroma.
        let y = [235, 16, 41, 41];
        let uv = [240, 110];
        let rgb = to_rgb(&y, 2, &uv, 2, Region { x: 0, y: 0, w: 2, h: 2 }, Region { x: 0, y: 0, w: 2, h: 2 });
        assert!(rgb.px(0, 0)[0] > 200 && rgb.px(0, 0)[2] == 255);
        assert!(rgb.px(1, 0)[1] < 10);
        let p = rgb.px(0, 1);
        assert!(p[2] > p[0] + 35 && p[2] > p[1], "{p:?}");
    }
}
