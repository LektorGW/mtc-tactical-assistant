use image::RgbaImage;
use serde::{Deserialize, Serialize};

mod auto_hud;
mod batch;
mod marker_selection;
mod marker_setup;
mod markers;
mod mtc_led;
mod mtc_loadout;
mod mtc_map;
mod mtc_panel;
mod ocr_layout;
mod preprocess;
mod recording;
mod tracking;
mod view;

pub use auto_hud::*;
pub use batch::*;
pub use marker_selection::*;
pub use marker_setup::*;
pub use markers::*;
pub use mtc_led::*;
pub use mtc_loadout::*;
pub use mtc_map::*;
pub use mtc_panel::*;
pub use ocr_layout::*;
pub use preprocess::*;
pub use recording::*;
pub use tracking::*;
pub use view::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Roi {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Roi {
    pub fn validate(&self) -> Result<(), String> {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .any(|v| !v.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > 1.000001
            || self.y + self.height > 1.000001
        {
            return Err("ROI musi mieścić się w obrazie (wartości 0–1)".into());
        }
        Ok(())
    }
    /// Pixel rectangle `(x, y, width, height)` of this ROI in an image of the given size.
    /// The left/top edge is floored and the size rounded, then clamped to the image.
    /// Every consumer (OCR crop, marker detection, click calibration) shares this rule,
    /// so normalized centroids and reference-map pixels use the same origin.
    pub fn pixel_rect(&self, width: u32, height: u32) -> Result<(u32, u32, u32, u32), String> {
        self.validate()?;
        if width == 0 || height == 0 {
            return Err("Pusty obraz".into());
        }
        let x = ((self.x * f64::from(width)).floor() as u32).min(width);
        let y = ((self.y * f64::from(height)).floor() as u32).min(height);
        let w = ((self.width * f64::from(width)).round() as u32)
            .max(1)
            .min(width.saturating_sub(x));
        let h = ((self.height * f64::from(height)).round() as u32)
            .max(1)
            .min(height.saturating_sub(y));
        if w == 0 || h == 0 {
            return Err("Pusty ROI".into());
        }
        Ok((x, y, w, h))
    }
    pub fn crop(&self, image: &RgbaImage) -> Result<RgbaImage, String> {
        let (x, y, w, h) = self.pixel_rect(image.width(), image.height())?;
        Ok(image::imageops::crop_imm(image, x, y, w, h).to_image())
    }
}

/// Preview thumbnail fitted into `max_width` x `max_height` with the same rounding as
/// `DynamicImage::thumbnail`, without first cloning the full source frame.
pub fn preview_thumbnail(image: &RgbaImage, max_width: u32, max_height: u32) -> RgbaImage {
    if image.width() == 0 || image.height() == 0 || max_width == 0 || max_height == 0 {
        return RgbaImage::new(0, 0);
    }
    let ratio = (f64::from(max_width) / f64::from(image.width()))
        .min(f64::from(max_height) / f64::from(image.height()));
    let width = ((f64::from(image.width()) * ratio).round() as u32).max(1);
    let height = ((f64::from(image.height()) * ratio).round() as u32).max(1);
    if (width, height) == image.dimensions() {
        return image.clone();
    }
    image::imageops::thumbnail(image, width, height)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub kind: String,
    pub value: f64,
    pub unit: String,
    pub text: String,
    pub confidence: f64,
    pub timestamp_ms: u64,
    pub source: String,
}

fn timer_regex() -> &'static regex::Regex {
    static TIMER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    TIMER
        .get_or_init(|| regex::Regex::new(r"([0-9]{1,2}):([0-5][0-9])").expect("Valid timer regex"))
}

fn number_regex() -> &'static regex::Regex {
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    NUMBER.get_or_init(|| {
        regex::Regex::new(r"[-+]?[0-9]+(?:[.,][0-9]+)?").expect("Valid number regex")
    })
}

const CONFUSABLE_DIGITS: [char; 5] = ['O', 'o', 'l', 'I', '|'];

/// Conservative OCR post-correction for numeric HUD fields only: a token that
/// already contains at least two digits may have exactly one `O`/`o` read
/// instead of `0` or `l`/`I`/`|` instead of `1`. Words are never changed.
fn correct_numeric_token(token: &str) -> std::borrow::Cow<'_, str> {
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let confusable = token
        .chars()
        .filter(|c| CONFUSABLE_DIGITS.contains(c))
        .count();
    let allowed = token.chars().all(|c| {
        c.is_ascii_digit()
            || CONFUSABLE_DIGITS.contains(&c)
            || matches!(c, ':' | '.' | ',' | '°' | '+' | '-')
    });
    if digits >= 2 && confusable == 1 && allowed {
        std::borrow::Cow::Owned(
            token
                .chars()
                .map(|c| match c {
                    'O' | 'o' => '0',
                    'l' | 'I' | '|' => '1',
                    other => other,
                })
                .collect(),
        )
    } else {
        std::borrow::Cow::Borrowed(token)
    }
}

fn correct_numeric_text(text: &str) -> String {
    text.split_whitespace()
        .map(correct_numeric_token)
        .collect::<Vec<_>>()
        .join(" ")
}

const NUMERIC_UNITS: [&str; 10] = [
    "m", "ft", "km", "km/h", "kmh", "kph", "mph", "kt", "kn", "m/s",
];

/// A number glued to letters (`13B`, `A12`) is a misread or another label, not a
/// HUD value. Headings allow only a degree sign; speed/altitude allow a unit.
fn plausible_number_context(kind: &str, text: &str, start: usize, end: usize) -> bool {
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphabetic)
    {
        return false;
    }
    let suffix: String = text[end..]
        .chars()
        .take_while(|c| c.is_alphabetic() || *c == '/')
        .collect();
    suffix.is_empty()
        || (kind != "heading" && NUMERIC_UNITS.contains(&suffix.to_lowercase().as_str()))
}

pub fn parse_numeric(kind: &str, text: &str, timestamp_ms: u64) -> Option<Observation> {
    let corrected = correct_numeric_text(text);
    let (value, unit) = if kind == "timer" {
        let matches: Vec<_> = timer_regex().captures_iter(&corrected).collect();
        if matches.len() != 1 {
            return None;
        }
        let c = &matches[0];
        let m = c.get(0)?;
        // A digit or another `:` next to the match means a longer clock such as
        // H:MM:SS or a misread value, not one MM:SS match timer.
        if corrected[..m.start()]
            .chars()
            .next_back()
            .is_some_and(|v| v.is_ascii_digit() || v == ':')
            || corrected[m.end()..]
                .chars()
                .next()
                .is_some_and(|v| v.is_ascii_digit() || v == ':')
        {
            return None;
        }
        (
            c[1].parse::<f64>().ok()? * 60.0 + c[2].parse::<f64>().ok()?,
            "s",
        )
    } else if ["heading", "altitude", "speed"].contains(&kind) {
        let values: Vec<_> = number_regex().find_iter(&corrected).collect();
        if values.len() != 1
            || !plausible_number_context(kind, &corrected, values[0].start(), values[0].end())
        {
            return None;
        }
        let value = values[0].as_str().replace(',', ".").parse::<f64>().ok()?;
        if kind == "heading" && !(0.0..360.0).contains(&value) {
            return None;
        }
        (
            value,
            if kind == "heading" {
                "°"
            } else {
                "HUD (jednostka niepotwierdzona)"
            },
        )
    } else {
        return None;
    };
    Some(Observation {
        kind: kind.into(),
        value,
        unit: unit.into(),
        text: text.into(),
        confidence: 0.6,
        timestamp_ms,
        source: "OCR Windows; pewność heurystyczna, wymaga stabilności".into(),
    })
}

pub fn is_fresh(observation: &Observation, now: u64, max_age: u64) -> bool {
    observation.timestamp_ms <= now && now - observation.timestamp_ms <= max_age
}

/// Windows OCR provides no statistical confidence; stability is a separate heuristic.
pub fn stabilize(current: &mut Observation, previous: Option<&Observation>) {
    if let Some(p) = previous {
        if p.kind == current.kind
            && p.timestamp_ms < current.timestamp_ms
            && is_fresh(p, current.timestamp_ms, 2000)
        {
            let delta = (p.value - current.value).abs();
            let stable = if current.kind == "timer" {
                delta <= 2.0
            } else if current.kind == "heading" {
                delta.min(360.0 - delta) <= 1.0
            } else {
                delta < 0.01
            };
            if stable {
                current.confidence = 0.9;
            }
        }
    }
}

pub fn ocr(image: &RgbaImage) -> Result<String, String> {
    ocr_layout(image).map(|layout| layout.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_match_timer_without_confusing_compass_ticks() {
        let o = parse_numeric("timer", "27:42", 1000).unwrap();
        assert_eq!(o.value, 1662.0);
        assert!(parse_numeric("heading", "090 138 180", 1000).is_none());
    }
    #[test]
    fn rejects_out_of_range_heading_and_invalid_timer() {
        assert!(parse_numeric("heading", "360", 0).is_none());
        assert!(parse_numeric("timer", "27:72", 0).is_none());
        assert!(parse_numeric("timer", "27:42 29:33", 0).is_none());
        assert!(parse_numeric("timer", "127:42", 0).is_none());
    }
    #[test]
    fn rejects_stale_and_future_observations() {
        let o = parse_numeric("heading", "138", 1000).unwrap();
        assert!(is_fresh(&o, 2000, 2000));
        assert!(!is_fresh(&o, 4000, 2000));
        assert!(!is_fresh(&o, 0, 2000));
    }
    #[test]
    fn increases_heuristic_confidence_only_with_matching_fresh_read() {
        let p = parse_numeric("heading", "138", 1000).unwrap();
        let mut c = parse_numeric("heading", "138", 1500).unwrap();
        stabilize(&mut c, Some(&p));
        assert_eq!(c.confidence, 0.9);
        let mut c = parse_numeric("heading", "180", 1500).unwrap();
        stabilize(&mut c, Some(&p));
        assert_eq!(c.confidence, 0.6);
    }
    #[test]
    fn rejects_numbers_glued_to_letters_but_keeps_units_and_degree_sign() {
        assert!(parse_numeric("heading", "13B", 0).is_none());
        assert!(parse_numeric("heading", "N138", 0).is_none());
        assert_eq!(parse_numeric("heading", "138°", 0).unwrap().value, 138.0);
        assert_eq!(
            parse_numeric("heading", "HDG: 138", 0).unwrap().value,
            138.0
        );
        assert!(parse_numeric("heading", "138m", 0).is_none());
        assert_eq!(parse_numeric("speed", "45km/h", 0).unwrap().value, 45.0);
        assert_eq!(
            parse_numeric("altitude", "Alt 120 m", 0).unwrap().value,
            120.0
        );
        assert!(parse_numeric("speed", "45 kmx", 0).is_some());
        assert!(parse_numeric("speed", "45abc", 0).is_none());
    }
    #[test]
    fn rejects_timer_embedded_in_a_longer_clock() {
        assert!(parse_numeric("timer", "1:05:30", 0).is_none());
        assert!(parse_numeric("timer", "05:30:", 0).is_none());
        assert_eq!(
            parse_numeric("timer", "Time 05:30", 0).unwrap().value,
            330.0
        );
    }
    #[test]
    fn corrects_single_letter_confusions_only_inside_numeric_tokens() {
        assert_eq!(parse_numeric("heading", "l38", 0).unwrap().value, 138.0);
        assert_eq!(parse_numeric("heading", "O90°", 0).unwrap().value, 90.0);
        assert_eq!(parse_numeric("timer", "2O:15", 0).unwrap().value, 1215.0);
        // Words, or tokens with more than one guess, are never rewritten.
        assert!(parse_numeric("heading", "lol", 0).is_none());
        assert!(parse_numeric("heading", "1lO8", 0).is_none());
        assert_eq!(parse_numeric("heading", "Iso 138", 0).unwrap().value, 138.0);
        assert_eq!(parse_numeric("heading", "l38", 0).unwrap().text, "l38");
    }
    #[test]
    fn roi_pixel_rect_never_panics_on_edges_or_empty_images() {
        let edge = Roi {
            x: 1.0,
            y: 0.0,
            width: 0.000001,
            height: 0.5,
        };
        assert!(edge.pixel_rect(10, 10).is_err());
        assert!(edge.crop(&RgbaImage::new(10, 10)).is_err());
        let full = Roi {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        assert!(full.pixel_rect(0, 10).is_err());
        assert_eq!(full.pixel_rect(7, 3).unwrap(), (0, 0, 7, 3));
        let tiny = Roi {
            x: 0.95,
            y: 0.95,
            width: 0.01,
            height: 0.01,
        };
        assert_eq!(tiny.pixel_rect(10, 10).unwrap(), (9, 9, 1, 1));
    }
    #[test]
    fn preview_thumbnail_matches_dynamic_image_dimensions_without_cloning_source() {
        for (w, h) in [(1920, 1080), (982, 653), (2560, 1600), (3, 900)] {
            let image = RgbaImage::new(w, h);
            let expected = image::DynamicImage::ImageRgba8(image.clone()).thumbnail(1440, 900);
            assert_eq!(
                preview_thumbnail(&image, 1440, 900).dimensions(),
                (expected.width(), expected.height())
            );
        }
        assert_eq!(
            preview_thumbnail(&RgbaImage::new(0, 5), 1440, 900).width(),
            0
        );
    }
    #[test]
    fn rejects_roi_outside_frame() {
        assert!(Roi {
            x: 0.9,
            y: 0.0,
            width: 0.2,
            height: 0.1
        }
        .validate()
        .is_err());
    }
}
