use crate::Roi;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrWordBox {
    pub text: String,
    pub roi: Roi,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLineBox {
    pub text: String,
    pub roi: Roi,
    pub words: Vec<OcrWordBox>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLayout {
    pub schema_version: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub recognized_width: u32,
    pub recognized_height: u32,
    pub text: String,
    pub text_angle_deg: Option<f64>,
    pub lines: Vec<OcrLineBox>,
    pub source: String,
    pub elapsed_ms: f64,
}

impl OcrLayout {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.source_width == 0
            || self.source_height == 0
            || self.recognized_width == 0
            || self.recognized_height == 0
            || u64::from(self.source_width) * u64::from(self.source_height) > 64_000_000
            || self.text.len() > 65_536
            || self.lines.len() > 1024
            || self
                .text_angle_deg
                .is_some_and(|v| !v.is_finite() || v.abs() > 180.0)
            || !self.elapsed_ms.is_finite()
            || self.elapsed_ms < 0.0
        {
            return Err("Niepoprawny układ OCR, geometria lub limit danych".into());
        }
        let source_aspect = f64::from(self.source_width) / f64::from(self.source_height);
        let recognized_aspect =
            f64::from(self.recognized_width) / f64::from(self.recognized_height);
        if (recognized_aspect / source_aspect - 1.0).abs() > 0.02 {
            return Err("OCR zmienił proporcje obrazu źródłowego".into());
        }
        let mut count = 0;
        for line in &self.lines {
            line.roi.validate()?;
            if line.words.is_empty() || line.text.len() > 16_384 {
                return Err("Brak geometrii słów lub zbyt długi wiersz OCR".into());
            }
            for word in &line.words {
                word.roi.validate()?;
                if word.roi.x + 0.000001 < line.roi.x
                    || word.roi.y + 0.000001 < line.roi.y
                    || word.roi.x + word.roi.width > line.roi.x + line.roi.width + 0.000001
                    || word.roi.y + word.roi.height > line.roi.y + line.roi.height + 0.000001
                {
                    return Err("BoundingRect słowa wychodzi poza jego wiersz OCR".into());
                }
                if word.text.trim().is_empty() || word.text.len() > 4096 {
                    return Err("Niepoprawne słowo OCR".into());
                }
                count += 1;
            }
        }
        if count > 4096 {
            return Err("Układ OCR przekracza limit 4096 słów".into());
        }
        Ok(())
    }
}

pub(crate) fn union_rois(rois: impl IntoIterator<Item = Roi>) -> Option<Roi> {
    let mut iter = rois.into_iter();
    let first = iter.next()?;
    let (mut left, mut top, mut right, mut bottom) = (
        first.x,
        first.y,
        first.x + first.width,
        first.y + first.height,
    );
    for roi in iter {
        left = left.min(roi.x);
        top = top.min(roi.y);
        right = right.max(roi.x + roi.width);
        bottom = bottom.max(roi.y + roi.height);
    }
    Some(Roi {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Map Windows' deskewed BoundingRect back to the original captured frame.
/// TextAngle rotates OCR boxes clockwise around image center, as documented by Microsoft.
pub fn normalize_ocr_box(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    recognized_width: u32,
    recognized_height: u32,
    text_angle_deg: f64,
) -> Result<Roi, String> {
    if recognized_width == 0
        || recognized_height == 0
        || [x, y, width, height, text_angle_deg]
            .iter()
            .any(|v| !v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || text_angle_deg.abs() > 180.0
    {
        return Err("Niepoprawna geometria BoundingRect OCR".into());
    }
    let frame_width = f64::from(recognized_width);
    let frame_height = f64::from(recognized_height);
    let angle = text_angle_deg.to_radians();
    let (sin, cos) = angle.sin_cos();
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for (px, py) in [
        (x, y),
        (x + width, y),
        (x, y + height),
        (x + width, y + height),
    ] {
        let dx = px - frame_width / 2.0;
        let dy = py - frame_height / 2.0;
        let rx = frame_width / 2.0 + dx * cos - dy * sin;
        let ry = frame_height / 2.0 + dx * sin + dy * cos;
        left = left.min(rx);
        top = top.min(ry);
        right = right.max(rx);
        bottom = bottom.max(ry);
    }
    left = left.clamp(0.0, frame_width);
    top = top.clamp(0.0, frame_height);
    right = right.clamp(0.0, frame_width);
    bottom = bottom.clamp(0.0, frame_height);
    let roi = Roi {
        x: left / frame_width,
        y: top / frame_height,
        width: (right - left) / frame_width,
        height: (bottom - top) / frame_height,
    };
    roi.validate()?;
    Ok(roi)
}

#[cfg(windows)]
pub fn ocr_layout(image: &RgbaImage) -> Result<OcrLayout, String> {
    use std::time::Instant;
    use windows::{
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
    };
    let started = Instant::now();
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Pusty obraz lub przekroczony limit 64 milionów pikseli".into());
    }
    let max = OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
    let resized;
    let recognized = if image.width() > max || image.height() > max {
        let scale = f64::from(max) / f64::from(image.width().max(image.height()));
        resized = image::imageops::resize(
            image,
            (f64::from(image.width()) * scale).round().max(1.0) as u32,
            (f64::from(image.height()) * scale).round().max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        );
        &resized
    } else {
        image
    };
    let writer = DataWriter::new().map_err(|e| e.to_string())?;
    let mut pixels = recognized.as_raw().clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    writer.WriteBytes(&pixels).map_err(|e| e.to_string())?;
    let buffer = writer.DetachBuffer().map_err(|e| e.to_string())?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        recognized.width() as i32,
        recognized.height() as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|e| e.to_string())?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("OCR niedostępny. Zainstaluj język OCR Windows: {e}"))?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let text_angle_deg = result.TextAngle().ok().and_then(|angle| angle.Value().ok());
    let mut lines = Vec::new();
    for line in result.Lines().map_err(|e| e.to_string())? {
        let mut words = Vec::new();
        for word in line.Words().map_err(|e| e.to_string())? {
            let rect = word.BoundingRect().map_err(|e| e.to_string())?;
            let Ok(roi) = normalize_ocr_box(
                f64::from(rect.X),
                f64::from(rect.Y),
                f64::from(rect.Width),
                f64::from(rect.Height),
                recognized.width(),
                recognized.height(),
                text_angle_deg.unwrap_or(0.0),
            ) else {
                continue;
            };
            words.push(OcrWordBox {
                text: word.Text().map_err(|e| e.to_string())?.to_string(),
                roi,
            });
        }
        if let Some(roi) = union_rois(words.iter().map(|word| word.roi)) {
            lines.push(OcrLineBox {
                text: line.Text().map_err(|e| e.to_string())?.to_string(),
                roi,
                words,
            });
        }
    }
    let layout = OcrLayout {
        schema_version: 1,
        source_width: image.width(),
        source_height: image.height(),
        recognized_width: recognized.width(),
        recognized_height: recognized.height(),
        text: result.Text().map_err(|e| e.to_string())?.to_string(),
        text_angle_deg,
        lines,
        source: "OCR Windows: BoundingRect słów przeliczony do oryginalnej klatki".into(),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    };
    layout.validate()?;
    Ok(layout)
}

#[cfg(not(windows))]
pub fn ocr_layout(_image: &RgbaImage) -> Result<OcrLayout, String> {
    Err("Systemowy OCR jest dostępny wyłącznie w Windows".into())
}
