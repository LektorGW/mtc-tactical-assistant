use crate::{view::match_view_signature_on_crop, MapViewSignature, Roi};
use image::RgbaImage;
use mtc_maps::Point;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Roles describe the user's assignment, never an inferred team or allegiance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerRole {
    Own,
    Target,
    Reference,
    Unassigned,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSignature {
    pub id: String,
    pub role: MarkerRole,
    pub rgb: [u8; 3],
    /// Maximum absolute difference per RGB channel, 0–100.
    pub tolerance: u8,
    /// Area measured in pixels of the reference map, after resolution scaling.
    pub min_component_pixels: u32,
    pub max_component_pixels: u32,
    pub min_aspect_ratio: f64,
    pub max_aspect_ratio: f64,
    pub min_fill_ratio: f64,
}

impl MarkerSignature {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.id.len() > 80 {
            return Err("Sygnatura markera wymaga krótkiego identyfikatora".into());
        }
        if self.tolerance > 100
            || self.min_component_pixels == 0
            || self.max_component_pixels < self.min_component_pixels
            || self.max_component_pixels > 1_000_000
            || !self.min_aspect_ratio.is_finite()
            || !self.max_aspect_ratio.is_finite()
            || self.min_aspect_ratio <= 0.0
            || self.max_aspect_ratio < self.min_aspect_ratio
            || !self.min_fill_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.min_fill_ratio)
        {
            return Err("Niepoprawne progi koloru, powierzchni lub kształtu markera".into());
        }
        Ok(())
    }

    fn matches(&self, pixel: &image::Rgba<u8>) -> bool {
        pixel[3] != 0
            && self
                .rgb
                .iter()
                .enumerate()
                .all(|(channel, color)| pixel[channel].abs_diff(*color) <= self.tolerance)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerConfig {
    #[serde(default)]
    pub enabled: bool,
    pub map_roi: Roi,
    pub map_width: u32,
    pub map_height: u32,
    /// The user confirms that ROI contains the complete calibrated reference map.
    #[serde(default)]
    pub coordinate_calibration_confirmed: bool,
    /// Descriptor of the complete visible map view explicitly chosen by the user.
    #[serde(default)]
    pub view_signature: Option<MapViewSignature>,
    pub aspect_tolerance: f64,
    pub signatures: Vec<MarkerSignature>,
}

impl MarkerConfig {
    pub fn validate(&self) -> Result<(), String> {
        self.map_roi.validate()?;
        if self.map_width == 0
            || self.map_height == 0
            || u64::from(self.map_width) * u64::from(self.map_height) > 64_000_000
            || !self.aspect_tolerance.is_finite()
            || !(0.0..=0.2).contains(&self.aspect_tolerance)
            || self.signatures.len() > 8
        {
            return Err("Niepoprawne wymiary lub proporcje konfiguracji mapy".into());
        }
        let mut ids = std::collections::HashSet::new();
        if let Some(signature) = &self.view_signature {
            signature.validate()?;
        }
        for signature in &self.signatures {
            signature.validate()?;
            if !ids.insert(&signature.id) {
                return Err("Identyfikatory sygnatur markerów muszą być unikalne".into());
            }
        }
        for (index, a) in self.signatures.iter().enumerate() {
            for b in self.signatures.iter().skip(index + 1) {
                let color_overlap = a.rgb.iter().zip(&b.rgb).all(|(ca, cb)| {
                    u16::from(ca.abs_diff(*cb)) <= u16::from(a.tolerance) + u16::from(b.tolerance)
                });
                let area_overlap = a.min_component_pixels <= b.max_component_pixels
                    && b.min_component_pixels <= a.max_component_pixels;
                let shape_overlap = a.min_aspect_ratio <= b.max_aspect_ratio
                    && b.min_aspect_ratio <= a.max_aspect_ratio;
                if color_overlap && area_overlap && shape_overlap {
                    return Err(format!(
                        "Sygnatury {} i {} są nierozróżnialne; zmień progi lub wskaż marker ręcznie",
                        a.id, b.id
                    ));
                }
            }
        }
        if self.enabled && self.signatures.is_empty() {
            return Err("Wskaż co najmniej jedną sygnaturę widocznego markera".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerObservation {
    pub signature_id: String,
    pub role: MarkerRole,
    pub map_position: Point,
    pub frame_position_normalized: Point,
    /// Bounding box relative to the full captured frame, in normalized coordinates.
    pub bounding_box: Roi,
    pub pixel_count: u32,
    pub confidence: f64,
    pub timestamp_ms: u64,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerAnalysis {
    pub markers: Vec<MarkerObservation>,
    pub warning: Option<String>,
}

/// Detect only visible pixels matching explicit user signatures. Ambiguity remains
/// in the returned list; this function never chooses a target among candidates.
pub fn detect_markers(
    image: &RgbaImage,
    config: &MarkerConfig,
    timestamp_ms: u64,
    now_ms: u64,
    max_age_ms: u64,
) -> Result<MarkerAnalysis, String> {
    config.validate()?;
    let blocked = |message: &str| MarkerAnalysis {
        markers: Vec::new(),
        warning: Some(message.into()),
    };
    if !config.enabled {
        return Ok(blocked("Automatyczny odczyt markerów jest wyłączony"));
    }
    if !config.coordinate_calibration_confirmed {
        return Ok(blocked(
            "Potwierdź, że ROI obejmuje całą skalibrowaną mapę referencyjną",
        ));
    }
    if timestamp_ms > now_ms || now_ms - timestamp_ms > max_age_ms {
        return Ok(blocked(
            "Klatka mapy jest przeterminowana lub ma przyszły znacznik czasu",
        ));
    }
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Obraz jest pusty lub przekracza limit 64 milionów pikseli".into());
    }
    let map = config.map_roi.crop(image)?;
    let observed_aspect = f64::from(map.width()) / f64::from(map.height());
    let expected_aspect = f64::from(config.map_width) / f64::from(config.map_height);
    if (observed_aspect / expected_aspect - 1.0).abs() > config.aspect_tolerance {
        return Ok(blocked(
            "Proporcje ROI nie pasują do mapy referencyjnej; wymagana ponowna kalibracja",
        ));
    }
    let Some(signature) = &config.view_signature else {
        return Ok(blocked(
            "Zapamiętaj wybrany widok pełnej mapy; brak deskryptora blokuje automatyczne pozycje",
        ));
    };
    // Reuse the single map crop; the descriptor match previously cropped it again.
    let view_match = match_view_signature_on_crop(&map, signature)?;
    if !view_match.matched {
        return Ok(blocked(view_match.warning.as_deref().unwrap_or(
            "Bieżący obraz nie pasuje do zapamiętanego widoku mapy",
        )));
    }
    let (crop_x, crop_y, _, _) = config.map_roi.pixel_rect(image.width(), image.height())?;
    let (crop_x, crop_y) = (f64::from(crop_x), f64::from(crop_y));
    let reference_area_scale = f64::from(config.map_width) * f64::from(config.map_height)
        / (f64::from(map.width()) * f64::from(map.height()));
    let mut markers = Vec::new();
    // One visited buffer and BFS queue are reused for every signature of the frame.
    let mut visited = vec![false; (map.width() as usize) * (map.height() as usize)];
    let mut queue = VecDeque::new();
    for signature in &config.signatures {
        // Each pixel is visited at most once per signature. Eight-connected
        // components preserve small diagonal and hollow HUD icons.
        visited.fill(false);
        for y in 0..map.height() {
            for x in 0..map.width() {
                let start = (y * map.width() + x) as usize;
                if visited[start] {
                    continue;
                }
                visited[start] = true;
                if !signature.matches(map.get_pixel(x, y)) {
                    continue;
                }
                queue.clear();
                queue.push_back((x, y));
                let (mut sum_x, mut sum_y, mut count) = (0.0, 0.0, 0u32);
                let (mut min_x, mut max_x, mut min_y, mut max_y) = (x, x, y, y);
                while let Some((px, py)) = queue.pop_front() {
                    sum_x += f64::from(px) + 0.5;
                    sum_y += f64::from(py) + 0.5;
                    count += 1;
                    min_x = min_x.min(px);
                    max_x = max_x.max(px);
                    min_y = min_y.min(py);
                    max_y = max_y.max(py);
                    for ny in py.saturating_sub(1)..=(py + 1).min(map.height() - 1) {
                        for nx in px.saturating_sub(1)..=(px + 1).min(map.width() - 1) {
                            let index = (ny * map.width() + nx) as usize;
                            if !visited[index] {
                                visited[index] = true;
                                if signature.matches(map.get_pixel(nx, ny)) {
                                    queue.push_back((nx, ny));
                                }
                            }
                        }
                    }
                }
                let reference_area = f64::from(count) * reference_area_scale;
                let width = max_x - min_x + 1;
                let height = max_y - min_y + 1;
                let aspect = f64::from(width) / f64::from(height);
                let fill = f64::from(count) / (f64::from(width) * f64::from(height));
                if reference_area < f64::from(signature.min_component_pixels)
                    || reference_area > f64::from(signature.max_component_pixels)
                    || aspect < signature.min_aspect_ratio
                    || aspect > signature.max_aspect_ratio
                    || fill < signature.min_fill_ratio
                {
                    continue;
                }
                let centroid_x = sum_x / f64::from(count);
                let centroid_y = sum_y / f64::from(count);
                // A cropped edge icon may be only partially visible; lower the
                // score so it cannot satisfy the default tracker threshold.
                let clipped = min_x == 0
                    || min_y == 0
                    || max_x + 1 == map.width()
                    || max_y + 1 == map.height();
                let confidence = if clipped {
                    0.35
                } else {
                    (0.55 + 0.25 * fill).min(0.8)
                };
                markers.push(MarkerObservation {
                    signature_id: signature.id.clone(),
                    role: signature.role,
                    map_position: Point {
                        x: centroid_x / f64::from(map.width()) * f64::from(config.map_width),
                        y: centroid_y / f64::from(map.height()) * f64::from(config.map_height),
                    },
                    frame_position_normalized: Point {
                        x: (crop_x + centroid_x) / f64::from(image.width()),
                        y: (crop_y + centroid_y) / f64::from(image.height()),
                    },
                    bounding_box: Roi {
                        x: (crop_x + f64::from(min_x)) / f64::from(image.width()),
                        y: (crop_y + f64::from(min_y)) / f64::from(image.height()),
                        width: f64::from(width) / f64::from(image.width()),
                        height: f64::from(height) / f64::from(image.height()),
                    },
                    pixel_count: count,
                    confidence,
                    timestamp_ms,
                    source: "CV: jawna sygnatura użytkownika; pewność heurystyczna".into(),
                });
                if markers.len() > 512 {
                    return Ok(blocked(
                        "Zbyt wiele kandydatów; zawęź ROI lub progi koloru i kształtu",
                    ));
                }
            }
        }
    }
    let ambiguous = [MarkerRole::Own, MarkerRole::Target]
        .iter()
        .any(|role| markers.iter().filter(|marker| marker.role == *role).count() > 1);
    let warning = if ambiguous {
        Some(
            "Wiele widocznych kandydatów dla pozycji lub celu; wskaż właściwy marker ręcznie"
                .into(),
        )
    } else if markers.is_empty() {
        Some("Brak widocznych markerów spełniających ustawione progi".into())
    } else {
        None
    };
    Ok(MarkerAnalysis { markers, warning })
}
