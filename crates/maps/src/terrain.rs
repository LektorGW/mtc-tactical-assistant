use crate::Point;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeightMapMetadata {
    pub schema_version: u32,
    pub map_id: String,
    pub map_width: u32,
    pub map_height: u32,
    pub min_height: f64,
    pub max_height: f64,
    pub unit: String,
    pub source: String,
    pub confirmed: bool,
    pub coordinate_alignment_confirmed: bool,
}

impl HeightMapMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.map_id.trim().is_empty()
            || self.map_id.len() > 256
            || self.map_width < 2
            || self.map_height < 2
            || self.map_width > 16384
            || self.map_height > 16384
            || !self.min_height.is_finite()
            || !self.max_height.is_finite()
            || self.min_height >= self.max_height
            || !(self.max_height - self.min_height).is_finite()
            || !["game_units", "game_meters"].contains(&self.unit.as_str())
            || self.source.trim().is_empty()
            || self.source.len() > 2048
        {
            return Err("Niepoprawne metadane, zakres wysokości lub jednostki heightmap".into());
        }
        if !self.confirmed {
            return Err("Potwierdź pochodzenie i zakres wysokości heightmap".into());
        }
        if !self.coordinate_alignment_confirmed {
            return Err(
                "Potwierdź zgodność heightmap z pełną mapą, jej kadrem i orientacją".into(),
            );
        }
        Ok(())
    }
}

/// Gray8 rasters keep their 8-bit code (2 bytes per sample instead of 16), so a
/// library of 21 prepared 1024² heightmaps stays near 42 MB instead of 336 MB.
#[derive(Clone)]
enum Samples {
    /// 0..=255 linear code; `MISSING_CODE` marks a transparent sample.
    Gray8(Vec<u16>),
    /// Absolute heights from a numeric grid; `None` marks missing data.
    Values(Vec<Option<f64>>),
}
const MISSING_CODE: u16 = u16::MAX;

/// User-supplied terrain raster, held only in RAM. Its axis-aligned extent is the same map image.
#[derive(Clone)]
pub struct HeightMap {
    metadata: HeightMapMetadata,
    width: u32,
    height: u32,
    samples: Samples,
}

impl HeightMap {
    /// Linear grayscale8 encoding: 0=minHeight, 255=maxHeight; transparent pixels are missing.
    /// Color imagery and partially transparent pixels are rejected instead of treated as terrain.
    pub fn from_rgba(image: &RgbaImage, metadata: HeightMapMetadata) -> Result<Self, String> {
        metadata.validate()?;
        validate_raster_size(image.width(), image.height())?;
        let mut codes = Vec::with_capacity(image.width() as usize * image.height() as usize);
        for pixel in image.pixels() {
            match pixel.0 {
                [_, _, _, 0] => codes.push(MISSING_CODE),
                [r, g, b, 255] if r == g && g == b => codes.push(r as u16),
                _ => {
                    return Err(
                        "Heightmap musi być liniowym obrazem szarym z alfą 0 lub 255".into(),
                    )
                }
            }
        }
        if codes.iter().all(|c| *c == MISSING_CODE) {
            return Err("Heightmap nie zawiera znanych wysokości".into());
        }
        Ok(Self {
            metadata,
            width: image.width(),
            height: image.height(),
            samples: Samples::Gray8(codes),
        })
    }

    /// Deterministic normalized scalar input, useful for imported numeric height grids.
    pub fn from_normalized(
        metadata: HeightMapMetadata,
        width: u32,
        height: u32,
        samples: &[Option<f64>],
    ) -> Result<Self, String> {
        metadata.validate()?;
        validate_raster_size(width, height)?;
        if samples.len() != width as usize * height as usize
            || samples
                .iter()
                .flatten()
                .any(|s| !s.is_finite() || *s < 0.0 || *s > 1.0)
            || samples.iter().all(Option::is_none)
        {
            return Err("Niepoprawne lub brakujące próbki heightmap".into());
        }
        let heights = samples
            .iter()
            .map(|value| {
                value.map(|v| metadata.min_height + (metadata.max_height - metadata.min_height) * v)
            })
            .collect();
        Ok(Self {
            metadata,
            width,
            height,
            samples: Samples::Values(heights),
        })
    }
    fn sample(&self, index: usize) -> Option<f64> {
        match &self.samples {
            Samples::Gray8(codes) => match codes[index] {
                MISSING_CODE => None,
                code => Some(
                    self.metadata.min_height
                        + (self.metadata.max_height - self.metadata.min_height) * code as f64
                            / 255.0,
                ),
            },
            Samples::Values(values) => values[index],
        }
    }
    /// Approximate sample storage, for diagnostics of the in-memory library.
    pub fn memory_bytes(&self) -> usize {
        match &self.samples {
            Samples::Gray8(codes) => codes.len() * std::mem::size_of::<u16>(),
            Samples::Values(values) => values.len() * std::mem::size_of::<Option<f64>>(),
        }
    }
    pub fn metadata(&self) -> &HeightMapMetadata {
        &self.metadata
    }
    pub fn raster_dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn sample_height(&self, point: Point, map_id: &str, unit: &str) -> Result<f64, String> {
        self.metadata.validate()?;
        if map_id != self.metadata.map_id {
            return Err("Heightmap dotyczy innej mapy".into());
        }
        if unit != self.metadata.unit {
            return Err("Jednostka wysokości nie zgadza się z jednostką obliczeń".into());
        }
        if !point.valid()
            || point.x < 0.0
            || point.y < 0.0
            || point.x > self.metadata.map_width as f64 - 1.0
            || point.y > self.metadata.map_height as f64 - 1.0
        {
            return Err("Punkt poza zakresem heightmap; ekstrapolacja jest wyłączona".into());
        }
        let x = point.x / (self.metadata.map_width - 1) as f64 * (self.width - 1) as f64;
        let y = point.y / (self.metadata.map_height - 1) as f64 * (self.height - 1) as f64;
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let dx = x - x0 as f64;
        let dy = y - y0 as f64;
        let mut height = 0.0;
        for (px, py, weight) in [
            (x0, y0, (1.0 - dx) * (1.0 - dy)),
            (x1, y0, dx * (1.0 - dy)),
            (x0, y1, (1.0 - dx) * dy),
            (x1, y1, dx * dy),
        ] {
            if weight > 1e-12 {
                let sample = self
                    .sample(py as usize * self.width as usize + px as usize)
                    .ok_or("Brak wysokości w obszarze interpolacji")?;
                height += sample * weight;
            }
        }
        if !height.is_finite() {
            return Err("Interpolacja wysokości przekracza zakres liczb".into());
        }
        Ok(height)
    }
    pub fn height_delta(
        &self,
        own: Point,
        target: Point,
        map_id: &str,
        unit: &str,
    ) -> Result<f64, String> {
        let own = self.sample_height(own, map_id, unit)?;
        let target = self.sample_height(target, map_id, unit)?;
        let delta = target - own;
        if !delta.is_finite() {
            return Err("Różnica wysokości przekracza zakres liczb".into());
        }
        Ok(delta)
    }
}

fn validate_raster_size(width: u32, height: u32) -> Result<(), String> {
    if width < 2 || height < 2 || width as u64 * height as u64 > 4_194_304 {
        return Err("Nieobsługiwany rozmiar heightmap (2×2 do 4M próbek)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata() -> HeightMapMetadata {
        HeightMapMetadata {
            schema_version: 1,
            map_id: "fixture".into(),
            map_width: 101,
            map_height: 101,
            min_height: 100.0,
            max_height: 200.0,
            unit: "game_units".into(),
            source: "Syntetyczny teren testowy".into(),
            confirmed: true,
            coordinate_alignment_confirmed: true,
        }
    }
    #[test]
    fn samples_bilinearly_in_reference_map_coordinates() {
        let terrain = HeightMap::from_normalized(
            metadata(),
            2,
            2,
            &[Some(0.0), Some(0.2), Some(0.8), Some(1.0)],
        )
        .unwrap();
        assert!(
            (terrain
                .sample_height(Point { x: 50.0, y: 50.0 }, "fixture", "game_units")
                .unwrap()
                - 150.0)
                .abs()
                < 1e-8
        );
        assert_eq!(
            terrain
                .sample_height(Point { x: 100.0, y: 100.0 }, "fixture", "game_units")
                .unwrap(),
            200.0
        );
        assert_eq!(
            terrain
                .height_delta(
                    Point { x: 0.0, y: 0.0 },
                    Point { x: 100.0, y: 100.0 },
                    "fixture",
                    "game_units"
                )
                .unwrap(),
            100.0
        );
    }
    #[test]
    fn rejects_missing_samples_wrong_map_units_and_extrapolation() {
        let terrain =
            HeightMap::from_normalized(metadata(), 2, 2, &[Some(0.0), None, Some(0.8), Some(1.0)])
                .unwrap();
        assert!(terrain
            .sample_height(Point { x: 50.0, y: 50.0 }, "fixture", "game_units")
            .is_err());
        assert_eq!(
            terrain
                .sample_height(Point { x: 0.0, y: 0.0 }, "fixture", "game_units")
                .unwrap(),
            100.0
        );
        for (p, id, unit) in [
            (Point { x: -0.01, y: 0.0 }, "fixture", "game_units"),
            (Point { x: 100.01, y: 0.0 }, "fixture", "game_units"),
            (Point { x: 0.0, y: 0.0 }, "wrong", "game_units"),
            (Point { x: 0.0, y: 0.0 }, "fixture", "game_meters"),
        ] {
            assert!(terrain.sample_height(p, id, unit).is_err());
        }
    }
    #[test]
    fn gates_confirmation_alignment_and_valid_range() {
        let mut m = metadata();
        m.confirmed = false;
        assert!(m.validate().is_err());
        m.confirmed = true;
        m.coordinate_alignment_confirmed = false;
        assert!(m.validate().is_err());
        m.coordinate_alignment_confirmed = true;
        m.max_height = m.min_height;
        assert!(m.validate().is_err());
        m.max_height = f64::INFINITY;
        assert!(m.validate().is_err());
        m.min_height = -f64::MAX;
        m.max_height = f64::MAX;
        assert!(m.validate().is_err());
    }
    #[test]
    fn rejects_colored_terrain_and_uses_grayscale_as_declared() {
        let mut image = RgbaImage::from_pixel(2, 2, image::Rgba([128, 128, 128, 255]));
        let terrain = HeightMap::from_rgba(&image, metadata()).unwrap();
        assert!(
            (terrain
                .sample_height(Point { x: 50.0, y: 50.0 }, "fixture", "game_units")
                .unwrap()
                - (100.0 + 100.0 * 128.0 / 255.0))
                .abs()
                < 1e-8
        );
        image.put_pixel(0, 0, image::Rgba([128, 0, 128, 255]));
        assert!(HeightMap::from_rgba(&image, metadata()).is_err());
    }
    #[test]
    fn gray8_storage_is_compact_and_keeps_transparent_samples_missing() {
        let mut image = RgbaImage::from_pixel(1024, 1024, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        let terrain = HeightMap::from_rgba(&image, metadata()).unwrap();
        assert_eq!(terrain.memory_bytes(), 1024 * 1024 * 2);
        assert!(terrain
            .sample_height(Point { x: 0.0, y: 0.0 }, "fixture", "game_units")
            .is_err());
        assert_eq!(
            terrain
                .sample_height(Point { x: 100.0, y: 100.0 }, "fixture", "game_units")
                .unwrap(),
            200.0
        );
        let copy = terrain.clone();
        assert_eq!(copy.raster_dimensions(), (1024, 1024));
    }
}
