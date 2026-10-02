//! Reading League's ability bar from the recording (after the game, layer 2 of ult tracking).
//!
//! Geometry: the bottom HUD is anchored at the bottom centre of the game image and scales
//! uniformly with the HUD scale setting and the resolution. Measured on the owner's recordings
//! (HUD scale 0, 1080 lines): Q W E R are 38 px squares, 44 px apart, R's left edge 20 px left of
//! the centre, their top 87 px above the bottom. Other HUD scales are one unknown factor `g`,
//! found by searching for the four icons (gold/grey frames separated by dark gaps); the match
//! quality is the confidence. Letterbox bars are found and excluded first. A layout without
//! that four-icon pattern (other modes, other HUDs) simply isn't found: verification is skipped.
//!
//! R icon state: on cooldown the icon is covered by League's blue overlay with white digits
//! (a radial sweep uncovers it as the cooldown runs out); not learned = dark grey; ready = the
//! icon itself. A cast = the blue overlay jumping up (also catches a recast right at the end of
//! a cooldown, where the icon was mostly uncovered).

use cv_core::game::{Region, Rgb};

/// Reference geometry at 1080 lines, HUD scale factor 1 (the owner's HUD scale 0).
pub const REF_H: f64 = 1080.0;
pub const ICON: f64 = 38.0;
pub const PITCH: f64 = 44.0;
pub const R_LEFT: f64 = -20.0;
pub const TOP: f64 = 87.0;
/// HUD scale factors searched (bigger HUD scale settings make it larger).
pub const G_MIN: f64 = 0.8;
pub const G_MAX: f64 = 2.6;

/// The game image inside the video (without letterbox bars).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Content {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Content {
    pub fn full(w: u32, h: u32) -> Content {
        Content { left: 0, top: 0, right: w, bottom: h }
    }
    pub fn height(&self) -> f64 {
        (self.bottom - self.top) as f64
    }
    pub fn cx(&self) -> f64 {
        (self.left + self.right) as f64 / 2.0
    }
}

/// Where the ability bar is: HUD scale factor and a small pixel offset.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct HudFit {
    pub content: Content,
    pub g: f64,
    pub dx: f64,
    pub dy: f64,
    /// Contrast between the icon frames and the gaps (median over the sampled frames).
    pub score: f64,
    /// 0..1.
    pub confidence: f64,
}

impl HudFit {
    fn s(&self) -> f64 {
        self.g * self.content.height() / REF_H
    }
    /// Box of ability `i` (0 = Q .. 3 = R) in video pixels: (x, y, size).
    pub fn icon(&self, i: usize) -> (f64, f64, f64) {
        icon_box(&self.content, self.g, self.dx, self.dy, i)
    }
    /// The R icon as a crop region (whole pixels, small margin).
    pub fn r_region(&self, frame_w: u32, frame_h: u32) -> Region {
        let (x, y, s) = self.icon(3);
        let m = 2.0;
        let x0 = (x - m).floor().max(0.0) as u32;
        let y0 = (y - m).floor().max(0.0) as u32;
        let x1 = ((x + s + m).ceil() as u32).min(frame_w);
        let y1 = ((y + s + m).ceil() as u32).min(frame_h);
        Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
    /// The bottom band that holds the four icons (for calibration crops).
    pub fn scale(&self) -> f64 {
        self.s()
    }
}

fn icon_box(c: &Content, g: f64, dx: f64, dy: f64, i: usize) -> (f64, f64, f64) {
    let s = g * c.height() / REF_H;
    let x = c.cx() + (R_LEFT - PITCH * (3 - i) as f64) * s + dx;
    let y = c.bottom as f64 - TOP * s + dy;
    (x, y, ICON * s)
}

/// Region of the video to decode for calibration: bottom centre, wide enough for any HUD scale.
pub fn calibration_region(w: u32, h: u32) -> Region {
    let s = h as f64 / REF_H * G_MAX;
    let half = ((PITCH * 4.0 + 40.0) * s) as u32;
    let top = (TOP * s + 20.0) as u32;
    let x = (w / 2).saturating_sub(half);
    let y = h.saturating_sub(top);
    Region { x, y, w: (half * 2).min(w - x), h: h - y }
}

#[inline]
fn lum(p: [u8; 3]) -> f64 {
    0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
}

/// Letterbox: rows/columns that are black in every sampled full frame.
pub fn find_content(frames: &[Rgb]) -> Option<Content> {
    let f0 = frames.first()?;
    let (w, h) = (f0.w, f0.h);
    let row_black = |y: u32| frames.iter().all(|f| (0..w).step_by(7).all(|x| lum(f.px(x, y)) < 14.0));
    let col_black = |x: u32| frames.iter().all(|f| (0..h).step_by(7).all(|y| lum(f.px(x, y)) < 14.0));
    let mut top = 0;
    while top < h / 3 && row_black(top) {
        top += 1;
    }
    let mut bottom = h;
    while bottom > h * 2 / 3 && row_black(bottom - 1) {
        bottom -= 1;
    }
    let mut left = 0;
    while left < w / 3 && col_black(left) {
        left += 1;
    }
    let mut right = w;
    while right > w * 2 / 3 && col_black(right - 1) {
        right -= 1;
    }
    Some(Content { left, top, right, bottom })
}

/// Bilinear luminance at continuous video coordinates (pixel `i` covers [i, i+1)).
fn lum_at(f: &Rgb, origin: (u32, u32), x: f64, y: f64) -> Option<f64> {
    let u = x - origin.0 as f64 - 0.5;
    let v = y - origin.1 as f64 - 0.5;
    if u < 0.0 || v < 0.0 {
        return None;
    }
    let (x0, y0) = (u.floor() as u32, v.floor() as u32);
    if x0 + 1 >= f.w || y0 + 1 >= f.h {
        return None;
    }
    let (fx, fy) = (u - x0 as f64, v - y0 as f64);
    let l = |x: u32, y: u32| lum(f.px(x, y));
    Some(l(x0, y0) * (1.0 - fx) * (1.0 - fy) + l(x0 + 1, y0) * fx * (1.0 - fy) + l(x0, y0 + 1) * (1.0 - fx) * fy + l(x0 + 1, y0 + 1) * fx * fy)
}

/// Contrast of the four-icon pattern in one frame (a crop taken at `origin`): the icons' 2 px
/// frames (bright gold when ready, dimmer grey otherwise) against the dark gaps between them.
/// The gaps are the HUD's background: very dark and the same everywhere (lum ~18 measured).
fn frame_score(f: &Rgb, origin: (u32, u32), c: &Content, g: f64, dx: f64, dy: f64) -> f64 {
    let s = g * c.height() / REF_H;
    let get = |x: f64, y: f64| lum_at(f, origin, x, y);
    let mut rings = [0.0; 4];
    let mut gaps: Vec<f64> = Vec::new();
    for (i, ring_out) in rings.iter_mut().enumerate() {
        let (x, y, size) = icon_box(c, g, dx, dy, i);
        // Each edge: the brightest of three lines 0.5..1.5 px inside (tolerates half-pixel
        // rounding of the 2 px frame).
        let edge = |pos: &dyn Fn(f64, f64) -> (f64, f64)| -> Option<f64> {
            let mut best: Option<f64> = None;
            for inset in [0.5 * s, 1.0 * s, 1.5 * s] {
                let mut sum = 0.0;
                let mut n = 0.0;
                for k in 2..12 {
                    let (px, py) = pos(size * k as f64 / 14.0, inset);
                    if let Some(v) = get(px, py) {
                        sum += v;
                        n += 1.0;
                    }
                }
                if n >= 5.0 {
                    let m = sum / n;
                    best = Some(best.map_or(m, |b: f64| b.max(m)));
                }
            }
            best
        };
        let edges = [
            edge(&|t, d| (x + t, y + d)),
            edge(&|t, d| (x + t, y + size - d)),
            edge(&|t, d| (x + d, y + t)),
            edge(&|t, d| (x + size - d, y + t)),
        ];
        if edges.iter().any(|e| e.is_none()) {
            return -100.0;
        }
        *ring_out = edges.iter().map(|e| e.unwrap()).sum::<f64>() / 4.0;
        // Gap centres: between icons (6 px wide at scale 1), and the 4 px gaps left of Q and
        // right of R.
        let mut cols: Vec<f64> = Vec::new();
        if i < 3 {
            cols.push(x + size + (PITCH - ICON) * s / 2.0);
        }
        if i == 3 {
            cols.push(x + size + 2.5 * s);
        }
        if i == 0 {
            cols.push(x - 3.0 * s);
        }
        for gx in cols {
            let mut gs = 0.0;
            let mut gn = 0.0;
            for k in 2..11 {
                if let Some(v) = get(gx, y + size * k as f64 / 12.0) {
                    gs += v;
                    gn += 1.0;
                }
            }
            if gn > 0.0 {
                gaps.push(gs / gn);
            }
        }
    }
    if gaps.is_empty() {
        return -100.0;
    }
    let gap = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let spread = (gaps.iter().map(|v| (v - gap).powi(2)).sum::<f64>() / gaps.len() as f64).sqrt();
    let contrast = rings.iter().map(|r| (r - gap).min(40.0)).sum::<f64>() / 4.0;
    contrast - (gap - 30.0).max(0.0) - 0.5 * spread
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Finds the ability bar in sampled frames (crops of [`calibration_region`] at `origin`).
/// `None` = no four-icon ability bar (another HUD, or not visible enough).
pub fn locate(frames: &[Rgb], origin: (u32, u32), content: Content) -> Option<HudFit> {
    if frames.is_empty() {
        return None;
    }
    let score_at = |g: f64, dx: f64, dy: f64| median(frames.iter().map(|f| frame_score(f, origin, &content, g, dx, dy)).collect());
    // Coarse: g only. Then g ± small, with pixel offsets.
    let mut coarse: Vec<(f64, f64)> = Vec::new();
    let mut g = G_MIN;
    while g <= G_MAX + 1e-9 {
        let best = [-1.0, 0.0, 1.0].iter().map(|dy| score_at(g, 0.0, *dy)).fold(f64::MIN, f64::max);
        coarse.push((g, best));
        g += 0.02;
    }
    let &(g0, _) = coarse.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())?;
    let mut best = (f64::MIN, g0, 0.0, 0.0);
    let offsets = [-1.0, -0.5, 0.0, 0.5, 1.0];
    let mut g = g0 - 0.03;
    while g <= g0 + 0.03 + 1e-9 {
        for dx in offsets {
            for dy in offsets {
                let sc = score_at(g, dx, dy);
                if sc > best.0 {
                    best = (sc, g, dx, dy);
                }
            }
        }
        g += 0.005;
    }
    let (score, g, dx, dy) = best;
    if std::env::var("CV_HUD_DEBUG").is_ok() {
        let rival = coarse.iter().filter(|(cg, _)| (cg - g).abs() > 0.15).map(|c| c.1).fold(f64::MIN, f64::max);
        let near = coarse.iter().filter(|(cg, _)| (cg - g).abs() > 0.03 && (cg - g).abs() <= 0.15).map(|c| c.1).fold(f64::MIN, f64::max);
        eprintln!("locate: g {g:.3} dx {dx} dy {dy} score {score:.1} rival(far) {rival:.1} near {near:.1}");
    }
    // A distinct optimum: nothing comparable at a clearly different scale.
    let rival = coarse.iter().filter(|(cg, _)| (cg - g).abs() > 0.15).map(|c| c.1).fold(f64::MIN, f64::max);
    let distinct = ((score - rival.max(4.0)) / 10.0).clamp(0.0, 1.0);
    let strength = ((score - 8.0) / 12.0).clamp(0.0, 1.0);
    let confidence = strength * (0.5 + 0.5 * distinct);
    if score < 10.0 {
        return None;
    }
    Some(HudFit { content, g, dx, dy, score, confidence })
}

/// What the R icon shows.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct RLook {
    /// Share of the icon under League's blue cooldown overlay (0..1).
    pub blue: f64,
    /// Share of very dark pixels (not learned).
    pub dark: f64,
    /// White digits in the centre (cooldown countdown).
    pub digits: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RState {
    Ready,
    Cooldown,
    NotLearned,
}

impl RLook {
    pub fn state(&self) -> RState {
        if (self.blue >= 0.5 && self.digits >= 0.06) || (self.blue >= 0.15 && self.digits >= 0.085) {
            // Full overlay, or the end of a cooldown (the sweep has uncovered most of the icon).
            RState::Cooldown
        } else if self.dark >= 0.28 && self.blue < 0.05 && self.digits < 0.02 {
            RState::NotLearned
        } else {
            RState::Ready
        }
    }
}

/// Measures the R icon in `crop` (any size, taken at `origin`); `fit` tells where the icon is.
pub fn look(crop: &Rgb, origin: (u32, u32), fit: &HudFit) -> RLook {
    let (x, y, s) = fit.icon(3);
    // Sample the icon on a 38×38 grid (its native size at the reference scale).
    let n = 38;
    let at = |i: usize, j: usize| -> [u8; 3] {
        let px = (x + (i as f64 + 0.5) * s / n as f64).floor() as i64 - origin.0 as i64;
        let py = (y + (j as f64 + 0.5) * s / n as f64).floor() as i64 - origin.1 as i64;
        let px = px.clamp(0, crop.w as i64 - 1) as u32;
        let py = py.clamp(0, crop.h as i64 - 1) as u32;
        crop.px(px, py)
    };
    let (mut blue, mut dark, mut cnt) = (0.0f64, 0.0f64, 0.0f64);
    for j in 4..34 {
        for i in 4..34 {
            let p = at(i, j);
            let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            if mn > 190 {
                continue; // white text
            }
            cnt += 1.0;
            if b > r + 35 && b > g + 12 && mx < 235 {
                blue += 1.0;
            }
            if mx < 45 {
                dark += 1.0;
            }
        }
    }
    let (mut white, mut wn) = (0.0f64, 0.0f64);
    for j in 12..32 {
        for i in 8..30 {
            let p = at(i, j);
            wn += 1.0;
            if p.iter().min().copied().unwrap_or(0) > 180 {
                white += 1.0;
            }
        }
    }
    RLook { blue: blue / cnt.max(1.0), dark: dark / cnt.max(1.0), digits: white / wn }
}

/// A small picture of the R icon (4×4 cells of mean colour over its inside), to tell its normal
/// "ready" look from another picture (a recast / command icon, e.g. Tibbers' head for Annie).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconSig(pub [f32; 48]);

/// Two icon pictures differ by more than this (mean absolute difference of the cells, 0..1):
/// video noise and the ready glow stay well under it, another picture is well over it.
pub const ALT_DIST: f32 = 0.085;

pub fn signature(crop: &Rgb, origin: (u32, u32), fit: &HudFit) -> IconSig {
    let (x, y, s) = fit.icon(3);
    let n = 38.0;
    let mut sum = [0f32; 48];
    let mut cnt = [0f32; 16];
    for j in 5..33 {
        for i in 5..33 {
            let px = ((x + (i as f64 + 0.5) * s / n).floor() as i64 - origin.0 as i64).clamp(0, crop.w as i64 - 1) as u32;
            let py = ((y + (j as f64 + 0.5) * s / n).floor() as i64 - origin.1 as i64).clamp(0, crop.h as i64 - 1) as u32;
            let p = crop.px(px, py);
            let c = ((j - 5) / 7) * 4 + (i - 5) / 7;
            for k in 0..3 {
                sum[c * 3 + k] += p[k] as f32 / 255.0;
            }
            cnt[c] += 1.0;
        }
    }
    for c in 0..16 {
        for k in 0..3 {
            sum[c * 3 + k] /= cnt[c].max(1.0);
        }
    }
    IconSig(sum)
}

pub fn sig_dist(a: &IconSig, b: &IconSig) -> f32 {
    a.0.iter().zip(b.0.iter()).map(|(x, y)| (x - y).abs()).sum::<f32>() / 48.0
}

/// The icon's own ready look in this game: the ready-state picture most others are close to
/// (champions spend more time with their normal icon than in a recast state).
pub fn ready_reference(sigs: &[IconSig]) -> Option<IconSig> {
    let step = (sigs.len() / 200).max(1);
    let pool: Vec<&IconSig> = sigs.iter().step_by(step).collect();
    pool.iter()
        .map(|a| (pool.iter().filter(|b| sig_dist(a, b) < ALT_DIST * 0.6).count(), *a))
        .max_by_key(|(n, _)| *n)
        .map(|(_, s)| *s)
}

/// A cast between two samples: the cooldown overlay appears, or comes back over an icon that
/// was mostly uncovered (recast right after the cooldown ended).
/// Two signs, either is enough: the blue overlay jumps up (most icons), or the cooldown number
/// appears where there was none (icons that are blue themselves, e.g. Ashe's).
pub fn is_cast(prev: &RLook, cur: &RLook) -> bool {
    let on_cooldown = cur.blue >= 0.5 && cur.digits >= 0.06;
    on_cooldown && (cur.blue - prev.blue >= 0.25 || (prev.digits < 0.03 && cur.digits - prev.digits >= 0.06))
}

/// The whole icon darkened: abilities disabled (stunned, silenced, dead, out of mana...) or not
/// learned. Says nothing about the cooldown, so it's skipped when looking for casts (otherwise
/// the cooldown coming back after a stun would look like a new cast).
pub fn dimmed(l: &RLook) -> bool {
    l.dark >= 0.45
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ashe's ult icon is blue itself: the cast shows as the cooldown number appearing.
    #[test]
    fn blue_icon_cast() {
        let at = (934, 987);
        let fit = HudFit { content: Content::full(1920, 1080), g: 1.0, dx: 0.0, dy: 0.0, score: 40.0, confidence: 1.0 };
        let ready = look(&fixture("r-ashe-ready.ppm"), at, &fit);
        let cd = look(&fixture("r-ashe-cooldown.ppm"), at, &fit);
        assert!(ready.blue > 0.6, "{ready:?}");
        assert_eq!(ready.state(), RState::Ready, "{ready:?}");
        assert_eq!(cd.state(), RState::Cooldown, "{cd:?}");
        assert!(is_cast(&ready, &cd));
        assert!(!is_cast(&cd, &cd));
    }

    #[test]
    fn dimmed_icons() {
        let at = (934, 987);
        let fit = HudFit { content: Content::full(1920, 1080), g: 1.0, dx: 0.0, dy: 0.0, score: 40.0, confidence: 1.0 };
        for n in ["r-yunara-dimmed.ppm", "r-caitlyn-dimmed.ppm", "r-twitch-dead.ppm", "r-caitlyn-notlearned.ppm"] {
            let l = look(&fixture(n), at, &fit);
            assert!(dimmed(&l), "{n}: {l:?}");
        }
        for n in ["r-yunara-ready.ppm", "r-yunara-cooldown.ppm", "r-caitlyn-ready.ppm", "r-caitlyn-cooldown.ppm", "r-twitch-ready.ppm", "r-twitch-cooldown.ppm"] {
            let l = look(&fixture(n), at, &fit);
            assert!(!dimmed(&l), "{n}: {l:?}");
        }
    }

    fn fixture(name: &str) -> Rgb {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hud").join(name);
        Rgb::from_ppm(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
    }

    /// Crops cut from the owner's recordings (3840×2160 game, 1080p video, HUD scale 0):
    /// bands of the bottom centre (360×100 at x 780, y 980) and the R icon (50×50 at 934, 987).
    const BAND: (u32, u32) = (780, 980);
    const RCROP: (u32, u32) = (934, 987);

    fn bands(prefix: &str) -> Vec<Rgb> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hud");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with(prefix) && n.ends_with(".ppm")).collect();
        names.sort();
        names.iter().map(|n| fixture(n)).collect()
    }

    fn located(prefix: &str) -> HudFit {
        locate(&bands(prefix), BAND, Content::full(1920, 1080)).expect("ability bar found")
    }

    #[test]
    fn finds_the_ability_bar_in_real_recordings() {
        for champ in ["band-caitlyn", "band-twitch", "band-yunara"] {
            let fit = located(champ);
            let (x, y, s) = fit.icon(3);
            assert!((x - 940.0).abs() <= 1.5 && (y - 993.0).abs() <= 1.5 && (s - 38.0).abs() <= 1.0, "{champ}: {fit:?}");
            assert!(fit.confidence > 0.5, "{champ}: {fit:?}");
        }
    }

    #[test]
    fn no_ability_bar_no_fit() {
        // A crop of the game world where the bar would be: no four-icon pattern.
        let fit = locate(&bands("band-none"), BAND, Content::full(1920, 1080));
        assert!(fit.is_none(), "{fit:?}");
    }

    #[test]
    fn scaled_hud_and_letterbox() {
        // A 1.5× HUD inside a 1920×1080 video with 60 px bars on top and bottom: scale the band.
        let reg = Region { x: BAND.0, y: BAND.1, w: 360, h: 100 };
        let src = bands("band-caitlyn");
        let content = Content { left: 0, top: 60, right: 1920, bottom: 1020 };
        let g = 1.5;
        let s = g * content.height() / REF_H;
        let out_reg = calibration_region(1920, 1080);
        let frames: Vec<Rgb> = src
            .iter()
            .map(|f| {
                let mut data = Vec::with_capacity((out_reg.w * out_reg.h * 3) as usize);
                for oy in 0..out_reg.h {
                    for ox in 0..out_reg.w {
                        // video px -> reference px (inverse of the HUD transform)
                        let vx = (out_reg.x + ox) as f64;
                        let vy = (out_reg.y + oy) as f64;
                        let rx = 960.0 + (vx - content.cx()) / s;
                        let ry = 1080.0 - (content.bottom as f64 - vy) / s;
                        let (lx, ly) = (rx as i64 - reg.x as i64, ry as i64 - reg.y as i64);
                        let p = if vy >= content.bottom as f64 { [0, 0, 0] } else if lx < 0 || ly < 0 || lx >= f.w as i64 || ly >= f.h as i64 { [70, 80, 60] } else { f.px(lx as u32, ly as u32) };
                        data.extend_from_slice(&p);
                    }
                }
                Rgb { w: out_reg.w, h: out_reg.h, data }
            })
            .collect();
        let fit = locate(&frames, (out_reg.x, out_reg.y), content).expect("found at 1.5×");
        assert!((fit.g - 1.5).abs() < 0.03, "{fit:?}");
        let (x, _, sz) = fit.icon(3);
        assert!((x - (960.0 - 20.0 * s)).abs() < 2.0 && (sz - 38.0 * s).abs() < 1.5, "{fit:?}");
    }

    #[test]
    fn r_states_from_real_frames() {
        let fit = located("band-caitlyn");
        for (name, want) in [
            ("r-caitlyn-ready.ppm", RState::Ready),
            ("r-caitlyn-cooldown.ppm", RState::Cooldown),
            ("r-caitlyn-notlearned.ppm", RState::NotLearned),
            ("r-yunara-ready.ppm", RState::Ready),
            ("r-yunara-cooldown.ppm", RState::Cooldown),
            ("r-twitch-ready.ppm", RState::Ready),
            ("r-twitch-cooldown.ppm", RState::Cooldown),
            ("r-twitch-late.ppm", RState::Cooldown),
        ] {
            let l = look(&fixture(name), RCROP, &fit);
            assert_eq!(l.state(), want, "{name}: {l:?}");
        }
        let ready = look(&fixture("r-yunara-ready.ppm"), RCROP, &fit);
        let cd = look(&fixture("r-yunara-cooldown.ppm"), RCROP, &fit);
        let late = look(&fixture("r-twitch-late.ppm"), RCROP, &fit);
        assert!(!is_cast(&cd, &late), "a cooldown refund isn't a cast");
        assert!(is_cast(&late, &look(&fixture("r-twitch-cooldown.ppm"), RCROP, &fit)), "recast at the end of a cooldown");
        assert!(is_cast(&ready, &cd), "{ready:?} -> {cd:?}");
        assert!(!is_cast(&cd, &cd));
    }

    /// Recast / command icons: the R icon showing another picture than its ready look. Real crops
    /// come from the owner's Practice Tool test; until then: the ready crops of this folder with
    /// the inside of another champion's icon pasted in (`*-alt-synthetic.ppm`), plus noise.
    #[test]
    fn recast_icon_picture() {
        let fit = located("band-caitlyn");
        let sig = |n: &str| signature(&fixture(n), RCROP, &fit);
        let ready = ["r-caitlyn-ready.ppm", "r-yunara-ready.ppm", "r-twitch-ready.ppm", "r-ashe-ready.ppm"];
        for a in ready {
            assert_eq!(sig_dist(&sig(a), &sig(a)), 0.0);
            for b in ready {
                if a != b {
                    let d = sig_dist(&sig(a), &sig(b));
                    assert!(d > ALT_DIST, "{a} vs {b}: {d:.3}: another icon must count as another picture");
                }
            }
        }
        for (base, alt) in [("r-caitlyn-ready.ppm", "r-caitlyn-alt-synthetic.ppm"), ("r-yunara-ready.ppm", "r-yunara-alt-synthetic.ppm")] {
            let (r, a) = (sig(base), sig(alt));
            let d = sig_dist(&r, &a);
            assert!(d > ALT_DIST, "{alt}: {d:.3}");
            // The alt crops are still "ready" (no cooldown overlay, not dimmed): only the picture
            // tells the recast state.
            let l = look(&fixture(alt), RCROP, &fit);
            assert_eq!(l.state(), RState::Ready, "{alt}: {l:?}");
            assert!(!dimmed(&l));
        }
        // Video noise and a brighter frame (the ready glow) stay the same picture.
        let mut noisy = fixture("r-caitlyn-ready.ppm");
        let mut seed = 7u32;
        for v in noisy.data.iter_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let n = ((seed >> 16) % 13) as i32 - 6;
            *v = (*v as i32 + n + 8).clamp(0, 255) as u8;
        }
        let d = sig_dist(&signature(&noisy, RCROP, &fit), &sig("r-caitlyn-ready.ppm"));
        assert!(d < ALT_DIST * 0.5, "noise: {d:.3}");
        // The game's own ready look wins the reference even with recast samples around.
        let mut sigs = vec![sig("r-caitlyn-ready.ppm"); 30];
        sigs.extend(vec![sig("r-caitlyn-alt-synthetic.ppm"); 12]);
        sigs.push(signature(&noisy, RCROP, &fit));
        let r = ready_reference(&sigs).unwrap();
        assert!(sig_dist(&r, &sig("r-caitlyn-ready.ppm")) < ALT_DIST * 0.5);
    }

    #[test]
    fn content_with_bars() {
        let mut data = vec![0u8; 100 * 80 * 3];
        for y in 10..70 {
            for x in 5..95 {
                data[(y * 100 + x) * 3] = 120;
            }
        }
        let c = find_content(&[Rgb { w: 100, h: 80, data }]).unwrap();
        assert_eq!(c, Content { left: 5, top: 10, right: 95, bottom: 70 });
    }
}
#[cfg(test)]
mod debug {
    use super::*;
    #[test]
    #[ignore]
    fn landscape() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hud");
        for n in ["band-yunara-300-4.ppm", "band-yunara-826-2.ppm", "band-yunara-826-9.ppm", "band-caitlyn-544-1.ppm"] {
            let f = Rgb::from_ppm(&std::fs::read(dir.join(n)).unwrap()).unwrap();
            let c = Content::full(1920, 1080);
            let mut row = String::new();
            for g in [0.99, 1.0, 1.01] {
                for (dx, dy) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.5, 0.5), (-0.5, -0.5), (1.0, 1.0)] {
                    row += &format!("g{g} d({dx},{dy})={:.1} ", frame_score(&f, (780, 980), &c, g, dx, dy));
                }
            }
            println!("{n}: {row}");
        }
    }
}
