use crate::{parse_numeric, OcrKind, Roi};
use image::{imageops, GrayImage, Luma, RgbaImage};

const MAX_OCR_INPUT_PIXELS: u64 = 16_000_000;

/// How one OCR input was prepared. Only local pixels are transformed; nothing is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OcrPreprocessing {
    /// Plain Catmull-Rom enlargement of the ROI.
    Upscaled,
    /// Light, neutral HUD text: per-pixel minimum RGB channel, robust contrast
    /// stretch, inversion to dark text on a light background and a margin.
    LightText,
}

fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_OCR_INPUT_PIXELS {
        return Err("Powiększony ROI przekracza limit 16 milionów pikseli".into());
    }
    Ok(())
}

/// Plain enlargement used as the first OCR attempt (unchanged legacy behaviour).
pub fn upscale_for_ocr(crop: &RgbaImage, upscale: u32) -> Result<RgbaImage, String> {
    let width = crop.width().saturating_mul(upscale);
    let height = crop.height().saturating_mul(upscale);
    check_size(width, height)?;
    Ok(imageops::resize(
        crop,
        width,
        height,
        imageops::FilterType::CatmullRom,
    ))
}

/// Normalizes bright, desaturated HUD text (white digits over a blue sky or a
/// dark pill). The minimum RGB channel keeps white text bright while saturated
/// backgrounds become dark; 2nd/98th percentile stretching removes JPEG/scene
/// haze; inversion and a margin give Windows OCR dark glyphs on a light page.
pub fn light_text_for_ocr(crop: &RgbaImage, upscale: u32) -> Result<RgbaImage, String> {
    let width = crop.width().saturating_mul(upscale);
    let height = crop.height().saturating_mul(upscale);
    check_size(width, height)?;
    let whiteness = GrayImage::from_fn(crop.width(), crop.height(), |x, y| {
        let p = crop.get_pixel(x, y);
        Luma([p[0].min(p[1]).min(p[2])])
    });
    let mut histogram = [0u64; 256];
    for pixel in whiteness.pixels() {
        histogram[usize::from(pixel[0])] += 1;
    }
    let total = u64::from(crop.width()) * u64::from(crop.height());
    let percentile = |fraction: f64| -> u8 {
        let target = ((total as f64) * fraction).floor() as u64;
        let mut seen = 0;
        for (value, count) in histogram.iter().enumerate() {
            seen += count;
            if seen > target {
                return value as u8;
            }
        }
        255
    };
    let low = percentile(0.02);
    let high = percentile(0.98);
    let span = u32::from(high.saturating_sub(low)).max(1);
    let normalized = GrayImage::from_fn(crop.width(), crop.height(), |x, y| {
        let value = whiteness.get_pixel(x, y)[0].clamp(low, high);
        let stretched = u32::from(value - low) * 255 / span;
        Luma([255 - stretched.min(255) as u8])
    });
    let enlarged = imageops::resize(&normalized, width, height, imageops::FilterType::CatmullRom);
    // Pad with the median border value so glyphs touching the ROI edge are not
    // clipped by the recognizer and the margin matches the dominant background.
    let mut border: Vec<u8> = (0..enlarged.width())
        .flat_map(|x| {
            [
                enlarged.get_pixel(x, 0)[0],
                enlarged.get_pixel(x, enlarged.height() - 1)[0],
            ]
        })
        .chain((0..enlarged.height()).flat_map(|y| {
            [
                enlarged.get_pixel(0, y)[0],
                enlarged.get_pixel(enlarged.width() - 1, y)[0],
            ]
        }))
        .collect();
    border.sort_unstable();
    let background = border[border.len() / 2];
    let margin = (height / 4).clamp(8, 64);
    check_size(width + 2 * margin, height + 2 * margin)?;
    let mut padded = RgbaImage::from_pixel(
        width + 2 * margin,
        height + 2 * margin,
        image::Rgba([background, background, background, 255]),
    );
    for (x, y, pixel) in enlarged.enumerate_pixels() {
        let v = pixel[0];
        padded.put_pixel(x + margin, y + margin, image::Rgba([v, v, v, 255]));
    }
    Ok(padded)
}

fn normalized_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether OCR text is a usable read for this kind: exactly one valid number or
/// MM:SS value for numeric fields, any non-empty text for explicit labels.
pub fn ocr_text_accepted(kind: OcrKind, raw: &str) -> bool {
    let text = normalized_text(raw);
    match kind {
        OcrKind::Timer | OcrKind::Heading | OcrKind::Altitude | OcrKind::Speed => {
            parse_numeric(kind.as_str(), &text, 0).is_some()
        }
        _ => !text.is_empty(),
    }
}

/// Reads one ROI with deterministic fallbacks. The plain enlargement is tried
/// first; only when it gives no accepted value are the light-text variant at the
/// same and at one higher scale tried. The first accepted text wins. When no
/// attempt is accepted, the first non-empty text is returned so the caller
/// reports `ambiguous`/`empty` exactly as before; no value is ever invented.
pub fn read_roi_text<F>(
    image: &RgbaImage,
    roi: Roi,
    upscale: u32,
    kind: OcrKind,
    mut recognize: F,
) -> Result<(String, OcrPreprocessing), String>
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    if !(1..=4).contains(&upscale) {
        return Err("Powiększenie OCR musi wynosić od 1 do 4".into());
    }
    let crop = roi.crop(image)?;
    let attempts = [
        (OcrPreprocessing::Upscaled, upscale),
        (OcrPreprocessing::LightText, upscale),
        (OcrPreprocessing::LightText, (upscale + 1).min(5)),
    ];
    let mut fallback: Option<(String, OcrPreprocessing)> = None;
    let mut first_error = None;
    for (mode, scale) in attempts {
        let input = match mode {
            OcrPreprocessing::Upscaled => upscale_for_ocr(&crop, scale),
            OcrPreprocessing::LightText => light_text_for_ocr(&crop, scale),
        };
        let input = match input {
            Ok(input) => input,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        match recognize(&input) {
            Ok(text) if ocr_text_accepted(kind, &text) => return Ok((text, mode)),
            Ok(text) => {
                if fallback.is_none() && !text.trim().is_empty() {
                    fallback = Some((text, mode));
                }
            }
            // The system OCR is unavailable: further attempts would fail the same way.
            Err(error) => return Err(error),
        }
    }
    match (fallback, first_error) {
        (Some(result), _) => Ok(result),
        (None, Some(error)) => Err(error),
        (None, None) => Ok((String::new(), OcrPreprocessing::Upscaled)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bright_digits_on_blue() -> RgbaImage {
        RgbaImage::from_fn(40, 12, |x, y| {
            if (8..32).contains(&x) && (3..9).contains(&y) && x % 4 != 0 {
                image::Rgba([235, 240, 245, 255])
            } else {
                image::Rgba([90, 150, 230, 255])
            }
        })
    }

    #[test]
    fn light_text_turns_white_glyphs_on_blue_into_dark_glyphs_on_light_margin() {
        let image = light_text_for_ocr(&bright_digits_on_blue(), 2).unwrap();
        let margin = (24 / 4u32).clamp(8, 64);
        assert_eq!(image.dimensions(), (80 + 2 * margin, 24 + 2 * margin));
        let glyph = image.get_pixel(margin + 2 * 10, margin + 2 * 5)[0];
        let sky = image.get_pixel(margin + 2, margin + 2)[0];
        assert!(glyph < 40, "glyph {glyph}");
        assert!(sky > 215, "background {sky}");
        assert_eq!(image.get_pixel(0, 0)[0], sky);
    }

    #[test]
    fn light_text_handles_uniform_and_one_pixel_crops_without_division_by_zero() {
        let flat = RgbaImage::from_pixel(1, 1, image::Rgba([10, 10, 10, 255]));
        let image = light_text_for_ocr(&flat, 4).unwrap();
        assert_eq!(image.dimensions(), (4 + 16, 4 + 16));
        assert!(light_text_for_ocr(&RgbaImage::new(0, 3), 2).is_err());
        assert!(light_text_for_ocr(&RgbaImage::new(5000, 5000), 4).is_err());
    }

    fn roi() -> Roi {
        Roi {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    #[test]
    fn falls_back_to_light_text_only_when_plain_read_is_not_a_valid_value() {
        let mut calls = Vec::new();
        let (text, mode) = read_roi_text(
            &bright_digits_on_blue(),
            roi(),
            2,
            OcrKind::Heading,
            |input| {
                calls.push(input.dimensions());
                Ok(if calls.len() == 1 { "13B" } else { "138" }.into())
            },
        )
        .unwrap();
        assert_eq!((text.as_str(), mode), ("138", OcrPreprocessing::LightText));
        assert_eq!(calls.len(), 2);
        let mut count = 0;
        let (text, mode) =
            read_roi_text(&bright_digits_on_blue(), roi(), 2, OcrKind::Timer, |_| {
                count += 1;
                Ok("27:42".into())
            })
            .unwrap();
        assert_eq!(
            (text.as_str(), mode, count),
            ("27:42", OcrPreprocessing::Upscaled, 1)
        );
    }

    #[test]
    fn keeps_first_unaccepted_text_and_propagates_ocr_errors() {
        let (text, _) = read_roi_text(&bright_digits_on_blue(), roi(), 2, OcrKind::Heading, |_| {
            Ok("090 138 180".into())
        })
        .unwrap();
        assert_eq!(text, "090 138 180");
        let mut count = 0;
        let error = read_roi_text(&bright_digits_on_blue(), roi(), 2, OcrKind::Speed, |_| {
            count += 1;
            Err("OCR niedostępny".into())
        })
        .unwrap_err();
        assert_eq!((error.as_str(), count), ("OCR niedostępny", 1));
        let (text, _) = read_roi_text(
            &bright_digits_on_blue(),
            roi(),
            2,
            OcrKind::VehicleName,
            |_| Ok(String::new()),
        )
        .unwrap();
        assert!(text.is_empty());
        assert!(
            read_roi_text(&bright_digits_on_blue(), roi(), 0, OcrKind::Timer, |_| Ok(
                String::new()
            ))
            .is_err()
        );
    }
}
