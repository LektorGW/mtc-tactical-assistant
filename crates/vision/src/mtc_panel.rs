//! Reader for the grey MTC weapon-station panel on the right side of the HUD:
//! weapon and ammunition names, the RANGE field and the ELEVATION/AZIMUTH display.
//! The panel is located geometrically (dark LED display boxes framed by metal),
//! so its position may move between weapons; sizes scale with frame height.

use crate::mtc_led::{
    builtin_led_model, decode_led_value, led_band, yellowness, LedBand, LedModel,
};
use crate::{ocr, preprocess::upscale_for_ocr, Roi};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Pixel rectangle `[x0, x1) x [y0, y1)` in the client frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelBox {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl PixelBox {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
    fn centre_x(&self) -> f32 {
        (self.x0 + self.x1) as f32 / 2.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponPanelLayout {
    /// Display with the weapon and ammunition names (always present).
    pub name_box: PixelBox,
    /// "0000" range field, when visible.
    pub range_box: Option<PixelBox>,
    /// ELEVATION/AZIMUTH display; absent on weapons without it (e.g. SPG-9).
    pub angle_box: Option<PixelBox>,
    /// Frame height / 1080.
    pub scale: f32,
}

fn dark(pixel: &image::Rgba<u8>) -> bool {
    pixel[0].max(pixel[1]).max(pixel[2]) <= 48
}

fn display_pixel(pixel: &image::Rgba<u8>) -> bool {
    dark(pixel) || yellowness(pixel) > 0.0
}

/// Fraction of display pixels along each of the four 1-px-inset edges.
fn edge_support(image: &RgbaImage, b: &PixelBox) -> f32 {
    let horizontal = |y: u32| {
        (b.x0..b.x1)
            .filter(|x| display_pixel(image.get_pixel(*x, y)))
            .count() as f32
            / b.width() as f32
    };
    let vertical = |x: u32| {
        (b.y0..b.y1)
            .filter(|y| display_pixel(image.get_pixel(x, *y)))
            .count() as f32
            / b.height() as f32
    };
    [
        horizontal(b.y0 + 1),
        horizontal(b.y1 - 2),
        vertical(b.x0 + 1),
        vertical(b.x1 - 2),
    ]
    .into_iter()
    .fold(1.0, f32::min)
}

/// Rectangular dark display boxes in the right 32% of the frame.
pub fn find_display_boxes(image: &RgbaImage) -> Vec<PixelBox> {
    let (w, h) = (image.width(), image.height());
    if w < 200 || h < 120 {
        return Vec::new();
    }
    let scale = h as f32 / 1080.0;
    let x_start = (w as f32 * 0.68) as u32;
    let region_w = (w - x_start) as usize;
    let mut seen = vec![false; region_w * h as usize];
    let mut boxes = Vec::new();
    let mut queue = VecDeque::new();
    for y in 0..h {
        for x in x_start..w {
            let index = (y as usize) * region_w + (x - x_start) as usize;
            if seen[index] {
                continue;
            }
            seen[index] = true;
            if !display_pixel(image.get_pixel(x, y)) {
                continue;
            }
            queue.clear();
            queue.push_back((x, y));
            let (mut x0, mut x1, mut y0, mut y1, mut count) = (x, x, y, y, 0u32);
            while let Some((px, py)) = queue.pop_front() {
                count += 1;
                x0 = x0.min(px);
                x1 = x1.max(px);
                y0 = y0.min(py);
                y1 = y1.max(py);
                let neighbours = [
                    (px.wrapping_sub(1), py),
                    (px + 1, py),
                    (px, py.wrapping_sub(1)),
                    (px, py + 1),
                ];
                for (nx, ny) in neighbours {
                    if nx < x_start || nx >= w || ny >= h {
                        continue;
                    }
                    let ni = (ny as usize) * region_w + (nx - x_start) as usize;
                    if !seen[ni] {
                        seen[ni] = true;
                        if display_pixel(image.get_pixel(nx, ny)) {
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            let candidate = PixelBox {
                x0,
                y0,
                x1: x1 + 1,
                y1: y1 + 1,
            };
            let (bw, bh) = (
                candidate.width() as f32 / scale,
                candidate.height() as f32 / scale,
            );
            if !(60.0..=220.0).contains(&bw) || !(14.0..=190.0).contains(&bh) {
                continue;
            }
            let fill = count as f32 / (candidate.width() * candidate.height()) as f32;
            if fill >= 0.5 && edge_support(image, &candidate) >= 0.85 {
                boxes.push(candidate);
            }
        }
    }
    boxes
}

/// Picks the panel: a name box, optionally a range field below it and an
/// angle display below that, all horizontally aligned.
pub fn locate_weapon_panel(image: &RgbaImage) -> Option<WeaponPanelLayout> {
    let scale = image.height() as f32 / 1080.0;
    let boxes = find_display_boxes(image);
    let size = |b: &PixelBox| (b.width() as f32 / scale, b.height() as f32 / scale);
    let names: Vec<&PixelBox> = boxes
        .iter()
        .filter(|b| {
            let (bw, bh) = size(b);
            (110.0..=200.0).contains(&bw) && (90.0..=180.0).contains(&bh)
        })
        .collect();
    let name_box = **names.iter().max_by_key(|b| b.x1)?;
    let aligned = |b: &&PixelBox| (b.centre_x() - name_box.centre_x()).abs() <= 12.0 * scale;
    let range_box = boxes
        .iter()
        .filter(aligned)
        .filter(|b| {
            let (bw, bh) = size(b);
            b.y0 > name_box.y1 && (50.0..=110.0).contains(&bw) && (14.0..=36.0).contains(&bh)
        })
        .min_by_key(|b| b.y0)
        .copied();
    let angle_box = boxes
        .iter()
        .filter(aligned)
        .filter(|b| {
            let (bw, bh) = size(b);
            let below = range_box.map_or(name_box.y1, |r| r.y1);
            b.y0 > below && (110.0..=200.0).contains(&bw) && (50.0..=95.0).contains(&bh)
        })
        .min_by_key(|b| b.y0)
        .copied();
    Some(WeaponPanelLayout {
        name_box,
        range_box,
        angle_box,
        scale,
    })
}

/// Rows `[y0, y1)` of yellow text lines inside a display box.
pub fn yellow_text_rows(image: &RgbaImage, b: &PixelBox, scale: f32) -> Vec<(u32, u32)> {
    let mut rows = Vec::new();
    let mut start = None;
    for y in b.y0..b.y1 {
        let lit = (b.x0..b.x1)
            .filter(|x| yellowness(image.get_pixel(*x, y)) > 0.0)
            .count();
        match (lit > 0, start) {
            (true, None) => start = Some(y),
            (false, Some(s)) => {
                if (y - s) as f32 >= 5.0 * scale {
                    rows.push((s, y));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if (b.y1 - s) as f32 >= 5.0 * scale {
            rows.push((s, b.y1));
        }
    }
    rows
}

/// Value bands (elevation, azimuth) of the angle display, for decoding or training.
pub fn angle_value_bands(
    image: &RgbaImage,
    layout: &WeaponPanelLayout,
) -> Option<(LedBand, LedBand)> {
    let b = layout.angle_box?;
    let rows = yellow_text_rows(image, &b, layout.scale);
    if rows.len() != 4 {
        return None;
    }
    let band = |(r0, r1): (u32, u32)| led_band(image, b.x0 + 2, b.x1 - 2, r0, r1, layout.scale);
    Some((band(rows[1])?, band(rows[3])?))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelValue {
    pub value: f64,
    pub text: String,
    /// Heuristic: 0.6 for one frame, 0.9 after two agreeing fresh frames.
    pub confidence: f64,
    pub template_cost: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelText {
    pub text: String,
    /// Catalog spelling when the OCR reading matched it (possibly after
    /// correcting digit/letter confusions such as `S-SKI` -> `S-5K1`).
    pub catalog_match: Option<String>,
    pub confidence: f64,
    pub attempts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WeaponPanelStatus {
    /// Names and both angles read.
    Ready,
    /// Panel found; some fields missing (e.g. no angle display on this weapon).
    Partial,
    /// No panel visible.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponPanelReading {
    pub schema_version: u32,
    pub status: WeaponPanelStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub layout: Option<WeaponPanelLayout>,
    pub weapon: Option<PanelText>,
    pub ammo: Option<PanelText>,
    /// "0000" in the RANGE field means no measurement and is reported as None.
    pub range_meters: Option<f64>,
    pub range_text: Option<String>,
    pub elevation_deg: Option<PanelValue>,
    pub azimuth_deg: Option<PanelValue>,
    pub warnings: Vec<String>,
}

/// Panel labels seen in live MTC captures (2026-10-15) that are not in the
/// community profile catalog. They only help spelling; no parameters are implied.
pub const OBSERVED_PANEL_NAMES: [&str; 5] =
    ["Rocketetet", "HE Rockets", "Unloaded", "73 mm 2A28", "PG-9"];

/// Canonical form tolerant to digit/letter confusions of this font.
fn confusable_key(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            '5' => 'S',
            '1' | 'L' | '|' => 'I',
            '0' | 'D' | 'Q' => 'O',
            '8' => 'B',
            '2' => 'Z',
            '6' | '9' => 'G',
            'C' => 'G',
            other => other,
        })
        .collect()
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads one name line with several scales, then prefers a catalog name that
/// matches (exactly or after confusion folding) in at least two attempts.
fn read_name_line<F>(
    image: &RgbaImage,
    b: &PixelBox,
    rows: (u32, u32),
    known: &[String],
    recognize: &mut F,
) -> Option<PanelText>
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let pad = 3;
    let y0 = rows.0.saturating_sub(pad).max(b.y0);
    let y1 = (rows.1 + pad).min(b.y1);
    let crop = image::imageops::crop_imm(image, b.x0 + 2, y0, b.width() - 4, y1 - y0).to_image();
    // Yellow text becomes dark glyphs on a white page; everything else is white.
    let mask = RgbaImage::from_fn(crop.width(), crop.height(), |x, y| {
        let v = 255 - (yellowness(crop.get_pixel(x, y)) * 255.0 * 1.6).min(255.0) as u8;
        image::Rgba([v, v, v, 255])
    });
    let mut attempts = Vec::new();
    for scale in [3u32, 4, 2] {
        for source in [&mask, &crop] {
            if let Ok(input) = upscale_for_ocr(source, scale) {
                if let Ok(text) = recognize(&input) {
                    let text = normalized(&text);
                    if !text.is_empty() {
                        attempts.push(text);
                    }
                }
            }
        }
    }
    if attempts.is_empty() {
        return None;
    }
    let exact = |name: &String| {
        attempts
            .iter()
            .filter(|a| a.eq_ignore_ascii_case(name))
            .count()
    };
    let folded = |name: &String| {
        let key = confusable_key(name);
        attempts.iter().filter(|a| confusable_key(a) == key).count()
    };
    let mut scored: Vec<(&String, usize, usize)> = known
        .iter()
        .map(|name| (name, exact(name), folded(name)))
        .filter(|(_, e, f)| *e >= 1 || *f >= 2)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(b.0)));
    let unique_best = match scored.as_slice() {
        [first, second, ..] if (first.1, first.2) == (second.1, second.2) => None,
        [first, ..] => Some(first),
        [] => None,
    };
    if let Some((name, exact_hits, _)) = unique_best {
        return Some(PanelText {
            text: (*name).clone(),
            catalog_match: Some((*name).clone()),
            confidence: if *exact_hits >= 2 { 0.75 } else { 0.65 },
            attempts,
        });
    }
    // No catalog name: the most frequent reading, only if repeated.
    let mut best: Option<(&String, usize)> = None;
    for a in &attempts {
        let count = attempts.iter().filter(|b| *b == a).count();
        if best.is_none_or(|b| count > b.1) {
            best = Some((a, count));
        }
    }
    let (text, count) = best?;
    Some(PanelText {
        text: text.clone(),
        catalog_match: None,
        confidence: if count >= 2 { 0.6 } else { 0.4 },
        attempts: attempts.clone(),
    })
}

fn decode_value(band: &LedBand, model: &LedModel) -> Option<PanelValue> {
    decode_led_value(band, model).map(|reading| PanelValue {
        value: reading.value,
        text: reading.text,
        confidence: 0.6,
        template_cost: reading.mean_cost,
    })
}

/// System OCR for names, built-in templates for angles.
pub fn read_weapon_panel(
    image: &RgbaImage,
    timestamp_ms: u64,
    known_names: &[String],
) -> WeaponPanelReading {
    read_weapon_panel_with(image, timestamp_ms, known_names, builtin_led_model(), ocr)
}

pub fn read_weapon_panel_with<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    known_names: &[String],
    model: &LedModel,
    mut recognize: F,
) -> WeaponPanelReading
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let mut reading = WeaponPanelReading {
        schema_version: 1,
        status: WeaponPanelStatus::Blocked,
        timestamp_ms,
        frame_width: image.width(),
        frame_height: image.height(),
        layout: None,
        weapon: None,
        ammo: None,
        range_meters: None,
        range_text: None,
        elevation_deg: None,
        azimuth_deg: None,
        warnings: Vec::new(),
    };
    let Some(layout) = locate_weapon_panel(image) else {
        reading
            .warnings
            .push("Nie znaleziono panelu stanowiska broni po prawej stronie HUD".into());
        return reading;
    };
    reading.status = WeaponPanelStatus::Partial;
    let rows = yellow_text_rows(image, &layout.name_box, layout.scale);
    // Names occupy the first two lines at the top of the display.
    if let Some(first) = rows.first() {
        reading.weapon =
            read_name_line(image, &layout.name_box, *first, known_names, &mut recognize);
    }
    if let Some(second) = rows.get(1) {
        reading.ammo = read_name_line(
            image,
            &layout.name_box,
            *second,
            known_names,
            &mut recognize,
        );
    }
    if let Some(range) = layout.range_box {
        let rows = yellow_text_rows(image, &range, layout.scale);
        if let Some(row) = rows.first() {
            let crop_roi = Roi {
                x: f64::from(range.x0) / f64::from(image.width()),
                y: f64::from(row.0.saturating_sub(2)) / f64::from(image.height()),
                width: f64::from(range.width()) / f64::from(image.width()),
                height: f64::from(row.1 - row.0 + 4) / f64::from(image.height()),
            };
            let text = crop_roi
                .crop(image)
                .ok()
                .and_then(|c| upscale_for_ocr(&c, 3).ok())
                .and_then(|c| recognize(&c).ok())
                .map(|t| t.chars().filter(|c| !c.is_whitespace()).collect::<String>())
                .unwrap_or_default();
            let digits: String = text
                .chars()
                .map(|c| match c {
                    'O' | 'o' | 'D' | 'Q' => '0',
                    other => other,
                })
                .collect();
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                let value: f64 = digits.parse().unwrap_or(0.0);
                // The game shows "0000" when no range is measured.
                reading.range_meters = (value > 0.0).then_some(value);
                reading.range_text = Some(digits);
            } else if !text.is_empty() {
                reading.range_text = Some(text);
                reading
                    .warnings
                    .push("Pole RANGE nieczytelne; brak pomiaru odległości".into());
            }
        }
    }
    match layout.angle_box {
        None => reading
            .warnings
            .push("To stanowisko nie pokazuje ELEVATION/AZIMUTH".into()),
        Some(_) => match angle_value_bands(image, &layout) {
            Some((elevation, azimuth)) => {
                reading.elevation_deg =
                    decode_value(&elevation, model).filter(|v| (-90.0..=90.0).contains(&v.value));
                reading.azimuth_deg =
                    decode_value(&azimuth, model).filter(|v| (0.0..360.0).contains(&v.value));
                if reading.elevation_deg.is_none() || reading.azimuth_deg.is_none() {
                    reading
                        .warnings
                        .push("Wartość ELEVATION/AZIMUTH nie pasuje do wzorca wyświetlacza".into());
                }
            }
            None => reading
                .warnings
                .push("Wyświetlacz kątów nie ma czterech linii tekstu".into()),
        },
    }
    reading.layout = Some(layout);
    if reading.weapon.is_some() && reading.elevation_deg.is_some() && reading.azimuth_deg.is_some()
    {
        reading.status = WeaponPanelStatus::Ready;
    }
    reading
}

/// Two fresh readings (≤ 2 s apart) of the same weapon/ammo with identical
/// displayed angles raise the heuristic confidence to 0.9.
pub fn stabilize_weapon_panel(
    current: &mut WeaponPanelReading,
    previous: Option<&WeaponPanelReading>,
) {
    let Some(previous) = previous else { return };
    if previous.timestamp_ms >= current.timestamp_ms
        || current.timestamp_ms - previous.timestamp_ms > 2000
        || previous.frame_width != current.frame_width
        || previous.frame_height != current.frame_height
    {
        return;
    }
    let same_text = |a: &Option<PanelText>, b: &Option<PanelText>| {
        a.as_ref().map(|t| &t.text) == b.as_ref().map(|t| &t.text) && a.is_some()
    };
    if same_text(&current.weapon, &previous.weapon) {
        if let Some(w) = current.weapon.as_mut() {
            w.confidence = w.confidence.max(0.85);
        }
    }
    if same_text(&current.ammo, &previous.ammo) {
        if let Some(a) = current.ammo.as_mut() {
            a.confidence = a.confidence.max(0.85);
        }
    }
    let same_value = |a: &Option<PanelValue>, b: &Option<PanelValue>| match (a, b) {
        (Some(a), Some(b)) => (a.value - b.value).abs() < 0.005,
        _ => false,
    };
    if same_value(&current.elevation_deg, &previous.elevation_deg) {
        if let Some(v) = current.elevation_deg.as_mut() {
            v.confidence = 0.9;
        }
    }
    if same_value(&current.azimuth_deg, &previous.azimuth_deg) {
        if let Some(v) = current.azimuth_deg.as_mut() {
            v.confidence = 0.9;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_font_confusions_for_catalog_names_only() {
        assert_eq!(confusable_key("S-SKI"), confusable_key("S-5K1"));
        assert_eq!(confusable_key("PC-9"), confusable_key("PG-9"));
        assert_eq!(confusable_key("PG-g"), confusable_key("PG-9"));
        assert_ne!(confusable_key("UB-32"), confusable_key("UB-16"));
    }

    #[test]
    fn empty_or_small_frames_have_no_panel() {
        assert!(locate_weapon_panel(&RgbaImage::new(100, 100)).is_none());
        let reading = read_weapon_panel_with(
            &RgbaImage::from_pixel(640, 360, image::Rgba([120, 120, 120, 255])),
            1,
            &[],
            builtin_led_model(),
            |_| panic!("no OCR without a panel"),
        );
        assert_eq!(reading.status, WeaponPanelStatus::Blocked);
    }

    fn value(v: f64) -> Option<PanelValue> {
        Some(PanelValue {
            value: v,
            text: format!("{v:.2}°"),
            confidence: 0.6,
            template_cost: 1.0,
        })
    }

    #[test]
    fn stabilizes_only_identical_fresh_angles() {
        let mut first = read_weapon_panel_with(
            &RgbaImage::new(100, 100),
            1000,
            &[],
            builtin_led_model(),
            |_| Ok(String::new()),
        );
        first.elevation_deg = value(23.53);
        first.azimuth_deg = value(268.53);
        let mut second = first.clone();
        second.timestamp_ms = 1500;
        second.azimuth_deg = value(268.54);
        stabilize_weapon_panel(&mut second, Some(&first));
        assert_eq!(second.elevation_deg.as_ref().unwrap().confidence, 0.9);
        assert_eq!(second.azimuth_deg.as_ref().unwrap().confidence, 0.6);
        let mut stale = first.clone();
        stale.timestamp_ms = 4000;
        stabilize_weapon_panel(&mut stale, Some(&first));
        assert_eq!(stale.elevation_deg.unwrap().confidence, 0.6);
    }
}
