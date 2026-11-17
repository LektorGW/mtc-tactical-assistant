//! Recognizer for the yellow dot-matrix digits of the MTC weapon panel
//! ("23.53°", "-0.89°"). Windows OCR misreads this font (lost decimal points,
//! `9` as `g`), so values are decoded by template matching under a strict
//! grammar. Templates come from labelled live captures (see
//! `examples/train_led.rs`); nothing outside the grammar is ever produced.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// Normalized band height (pixels of a 1080p client).
pub const LED_HEIGHT: usize = 11;
/// Canvas width a glyph is centred in for comparison.
pub const LED_CANVAS: usize = 10;
const GAP_COST: f32 = 0.05;
const LED_GLOW: i32 = 45;
const LED_RANGE: f32 = 70.0;
const WIDTH_WEIGHT: f32 = 0.05;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedGlyph {
    pub ch: char,
    pub min_width: usize,
    pub max_width: usize,
    pub mean_width: f32,
    /// Row-major LED_HEIGHT x LED_CANVAS intensities 0..=255.
    pub canvas: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LedModel {
    pub glyphs: Vec<LedGlyph>,
    /// Upper bound of the mean per-character cost accepted as a reading.
    pub max_mean_cost: f32,
}

/// Yellow text intensity 0..1: `min(R,G) - B` for pixels whose red and green
/// are within ~22% of each other. Orange icons, grass and grey metal give ~0.
pub fn yellowness(pixel: &image::Rgba<u8>) -> f32 {
    let (r, g, b) = (
        i32::from(pixel[0]),
        i32::from(pixel[1]),
        i32::from(pixel[2]),
    );
    if g * 100 < r * 78 || r * 100 < g * 78 {
        return 0.0;
    }
    // The LED glow between strokes reaches ~50; strokes exceed ~110.
    ((r.min(g) - b - LED_GLOW) as f32 / LED_RANGE).clamp(0.0, 1.0)
}

/// One text line of the display, resampled to LED_HEIGHT rows.
#[derive(Clone, Debug, PartialEq)]
pub struct LedBand {
    pub width: usize,
    /// Row-major LED_HEIGHT x width values 0..1.
    pub values: Vec<f32>,
}

impl LedBand {
    fn at(&self, x: usize, y: usize) -> f32 {
        self.values[y * self.width + x]
    }
    fn column_ink(&self, x: usize) -> f32 {
        (0..LED_HEIGHT).map(|y| self.at(x, y).powi(2)).sum()
    }
}

/// Extracts a band centred on rows `[row0, row1)` and columns `[col0, col1)` of
/// `image`, scaled so the band is LED_HEIGHT pixels tall at `scale` = height/1080.
pub fn led_band(
    image: &RgbaImage,
    col0: u32,
    col1: u32,
    row0: u32,
    row1: u32,
    scale: f32,
) -> Option<LedBand> {
    if col1 <= col0 || row1 <= row0 || col1 > image.width() || !(0.3..=4.0).contains(&scale) {
        return None;
    }
    let centre = (row0 + row1) as f32 / 2.0;
    let source_height = LED_HEIGHT as f32 * scale;
    let width = (((col1 - col0) as f32) / scale).round().max(1.0) as usize;
    let mut values = vec![0.0; LED_HEIGHT * width];
    for y in 0..LED_HEIGHT {
        for x in 0..width {
            // Area-free bilinear sampling; at scale 1 this is an exact pixel copy.
            let sx = col0 as f32 + (x as f32 + 0.5) * scale - 0.5;
            let sy = centre - source_height / 2.0 + (y as f32 + 0.5) * scale - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let mut sum = 0.0;
            for (dx, dy, w) in [
                (0, 0, (1.0 - fx) * (1.0 - fy)),
                (1, 0, fx * (1.0 - fy)),
                (0, 1, (1.0 - fx) * fy),
                (1, 1, fx * fy),
            ] {
                let px = x0 as i64 + dx;
                let py = y0 as i64 + dy;
                if w > 0.0
                    && px >= i64::from(col0)
                    && px < i64::from(col1)
                    && py >= 0
                    && py < i64::from(image.height())
                {
                    sum += w * yellowness(image.get_pixel(px as u32, py as u32));
                }
            }
            values[y * width + x] = sum;
        }
    }
    Some(LedBand { width, values })
}

/// Glyph columns `[a, b)` left-aligned at canvas column 1. The decoder picks the
/// exact start column, so no centroid re-centring (which blurs 6/8/0) is needed.
fn canvas(band: &LedBand, a: usize, b: usize) -> [f32; LED_HEIGHT * LED_CANVAS] {
    let mut out = [0.0; LED_HEIGHT * LED_CANVAS];
    for x in a..b {
        let target = x - a + 1;
        if target >= LED_CANVAS {
            break;
        }
        for y in 0..LED_HEIGHT {
            out[y * LED_CANVAS + target] = band.at(x, y);
        }
    }
    out
}

fn glyph_cost(band: &LedBand, a: usize, b: usize, glyph: &LedGlyph) -> f32 {
    let c = canvas(band, a, b);
    let ssd: f32 = c
        .iter()
        .zip(&glyph.canvas)
        .map(|(v, t)| (v - f32::from(*t) / 255.0).powi(2))
        .sum();
    ssd + WIDTH_WEIGHT * ((b - a) as f32 - glyph.mean_width).powi(2)
}

/// Grammar of the elevation/azimuth display: `-? d{1,3} . d d °`.
fn transitions(state: usize) -> &'static [(&'static str, usize)] {
    const DIGITS: &str = "0123456789";
    match state {
        0 => &[("-", 1), (DIGITS, 2)],
        1 => &[(DIGITS, 2)],
        2 => &[(DIGITS, 3), (".", 5)],
        3 => &[(DIGITS, 4), (".", 5)],
        4 => &[(".", 5)],
        5 => &[(DIGITS, 6)],
        6 => &[(DIGITS, 7)],
        7 => &[("°", 8)],
        _ => &[],
    }
}
const FINAL_STATE: usize = 8;
const STATES: usize = 9;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedReading {
    pub text: String,
    pub value: f64,
    /// Mean template cost per character (lower is better).
    pub mean_cost: f32,
}

/// Placement of one decoded character, for training and diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct LedPlacement {
    pub ch: char,
    pub start: usize,
    pub end: usize,
}

fn decode_path(
    band: &LedBand,
    model: &LedModel,
    allowed: &dyn Fn(usize) -> Vec<(char, usize)>,
    states: usize,
    final_state: usize,
) -> Option<(Vec<LedPlacement>, f32)> {
    let w = band.width;
    let inf = f32::INFINITY;
    let mut dp = vec![inf; (w + 1) * states];
    let mut back: Vec<Option<(usize, usize, Option<char>)>> = vec![None; (w + 1) * states];
    let index = |x: usize, s: usize| x * states + s;
    dp[index(0, 0)] = 0.0;
    let gaps: Vec<f32> = (0..w).map(|x| band.column_ink(x) + GAP_COST).collect();
    for x in 0..=w {
        for s in 0..states {
            let value = dp[index(x, s)];
            if !value.is_finite() {
                continue;
            }
            if x < w && value + gaps[x] < dp[index(x + 1, s)] {
                dp[index(x + 1, s)] = value + gaps[x];
                back[index(x + 1, s)] = Some((x, s, None));
            }
            for (ch, next) in allowed(s) {
                for glyph in model.glyphs.iter().filter(|g| g.ch == ch) {
                    for width in glyph.min_width..=glyph.max_width {
                        if x + width > w {
                            break;
                        }
                        let cost = value + glyph_cost(band, x, x + width, glyph);
                        if cost < dp[index(x + width, next)] {
                            dp[index(x + width, next)] = cost;
                            back[index(x + width, next)] = Some((x, s, Some(ch)));
                        }
                    }
                }
            }
        }
    }
    let total = dp[index(w, final_state)];
    if !total.is_finite() {
        return None;
    }
    let (mut x, mut s) = (w, final_state);
    let mut placements = Vec::new();
    while let Some((px, ps, ch)) = back[index(x, s)] {
        if let Some(ch) = ch {
            placements.push(LedPlacement {
                ch,
                start: px,
                end: x,
            });
        }
        x = px;
        s = ps;
    }
    placements.reverse();
    Some((placements, total))
}

/// Decodes one value line. `None` when the line does not fit the grammar
/// well enough; a misfit is never rounded to a plausible number.
pub fn decode_led_value(band: &LedBand, model: &LedModel) -> Option<LedReading> {
    let allowed = |s: usize| {
        transitions(s)
            .iter()
            .flat_map(|(chars, next)| chars.chars().map(move |c| (c, *next)))
            .collect::<Vec<_>>()
    };
    let (placements, total) = decode_path(band, model, &allowed, STATES, FINAL_STATE)?;
    let text: String = placements.iter().map(|p| p.ch).collect();
    let mean_cost = total / placements.len().max(1) as f32;
    if mean_cost > model.max_mean_cost {
        return None;
    }
    let value: f64 = text.trim_end_matches('°').parse().ok()?;
    Some(LedReading {
        text,
        value: if value == 0.0 { 0.0 } else { value },
        mean_cost,
    })
}

/// Forced alignment of a known label (training only).
pub fn align_led_label(band: &LedBand, model: &LedModel, label: &str) -> Option<Vec<LedPlacement>> {
    let chars: Vec<char> = label.chars().collect();
    let n = chars.len();
    let allowed = |s: usize| {
        if s < n {
            vec![(chars[s], s + 1)]
        } else {
            Vec::new()
        }
    };
    decode_path(band, model, &allowed, n + 1, n).map(|(p, _)| p)
}

/// Learns templates from labelled bands by alternating forced alignment and
/// averaging (a few EM rounds). `initial_widths` seeds the first layout.
pub fn train_led_model(
    samples: &[(LedBand, String)],
    initial_widths: &[(char, usize)],
) -> LedModel {
    let width_of = |c: char| {
        initial_widths
            .iter()
            .find(|(ch, _)| *ch == c)
            .map_or(7, |(_, w)| *w)
    };
    // Proportional initial layout over each sample's ink extent.
    let mut layouts: Vec<Vec<LedPlacement>> = samples
        .iter()
        .map(|(band, label)| {
            let inked: Vec<usize> = (0..band.width)
                .filter(|x| band.column_ink(*x) > 0.1)
                .collect();
            let (l, r) = (
                *inked.first().unwrap_or(&0),
                inked.last().map_or(band.width, |x| x + 1),
            );
            let total: usize = label.chars().map(width_of).sum();
            let scale = (r - l) as f32 / total.max(1) as f32;
            let mut x = l as f32;
            label
                .chars()
                .map(|c| {
                    let start = x.round() as usize;
                    x += width_of(c) as f32 * scale;
                    LedPlacement {
                        ch: c,
                        start,
                        end: (x.round() as usize).min(band.width).max(start + 1),
                    }
                })
                .collect()
        })
        .collect();
    let mut model = LedModel {
        glyphs: Vec::new(),
        max_mean_cost: f32::INFINITY,
    };
    for _ in 0..8 {
        let mut chars: Vec<char> = samples.iter().flat_map(|(_, l)| l.chars()).collect();
        chars.sort_unstable();
        chars.dedup();
        model.glyphs = chars
            .iter()
            .map(|&ch| {
                let mut sum = [0.0f32; LED_HEIGHT * LED_CANVAS];
                let mut widths = Vec::new();
                for ((band, _), layout) in samples.iter().zip(&layouts) {
                    for p in layout.iter().filter(|p| p.ch == ch) {
                        for (s, v) in sum.iter_mut().zip(canvas(band, p.start, p.end)) {
                            *s += v;
                        }
                        widths.push(p.end - p.start);
                    }
                }
                let n = widths.len().max(1) as f32;
                let mean = widths.iter().sum::<usize>() as f32 / n;
                LedGlyph {
                    ch,
                    min_width: widths
                        .iter()
                        .min()
                        .copied()
                        .unwrap_or(1)
                        .saturating_sub(1)
                        .max(1),
                    max_width: widths.iter().max().copied().unwrap_or(8) + 1,
                    mean_width: mean,
                    canvas: sum
                        .iter()
                        .map(|v| ((v / n) * 255.0).round().clamp(0.0, 255.0) as u8)
                        .collect(),
                }
            })
            .collect();
        layouts = samples
            .iter()
            .zip(&layouts)
            .map(|((band, label), old)| {
                align_led_label(band, &model, label).unwrap_or_else(|| old.clone())
            })
            .collect();
    }
    // Acceptance bound: generous margin over the worst training fit.
    let worst = samples
        .iter()
        .filter_map(|(band, label)| {
            let placements = align_led_label(band, &model, label)?;
            let cost: f32 = placements
                .iter()
                .map(|p| {
                    let glyph = model.glyphs.iter().find(|g| g.ch == p.ch).unwrap();
                    glyph_cost(band, p.start, p.end, glyph)
                })
                .sum();
            Some(cost / placements.len().max(1) as f32)
        })
        .fold(0.0f32, f32::max);
    model.max_mean_cost = worst * 2.0 + 0.5;
    model
}

/// Built-in model trained on the 2026-10-15 live captures (1080p panel).
pub fn builtin_led_model() -> &'static LedModel {
    static MODEL: std::sync::OnceLock<LedModel> = std::sync::OnceLock::new();
    MODEL.get_or_init(|| {
        serde_json::from_str(include_str!("mtc_led_model.json")).expect("Valid LED model")
    })
}
