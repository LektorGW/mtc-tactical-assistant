use crate::{MarkerRole, MarkerSignature, Roi};
use image::RgbaImage;
use mtc_maps::Point;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerProposalConfig {
    pub max_age_ms: u64,
    pub max_analysis_pixels: u32,
    pub max_components: u32,
    pub max_proposals: usize,
    pub min_component_pixels: u32,
    pub max_component_pixels: u32,
    pub color_tolerance: u8,
    pub min_alpha: u8,
}

impl Default for MarkerProposalConfig {
    fn default() -> Self {
        Self {
            max_age_ms: 2000,
            max_analysis_pixels: 4_000_000,
            max_components: 1024,
            max_proposals: 64,
            min_component_pixels: 3,
            max_component_pixels: 4096,
            color_tolerance: 36,
            min_alpha: 128,
        }
    }
}

impl MarkerProposalConfig {
    fn validate(&self) -> Result<(), String> {
        if self.max_age_ms == 0
            || self.max_age_ms > 2000
            || self.max_analysis_pixels == 0
            || self.max_analysis_pixels > 4_000_000
            || self.max_components == 0
            || self.max_components > 4096
            || !(1..=64).contains(&self.max_proposals)
            || self.min_component_pixels == 0
            || self.max_component_pixels < self.min_component_pixels
            || self.max_component_pixels > 4096
            || !(1..=64).contains(&self.color_tolerance)
            || self.min_alpha < 64
        {
            return Err("Niepoprawne limity propozycji widocznych markerów".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSignatureProposal {
    /// Component-local ID for this frame, never a persistent object identity.
    pub id: String,
    /// Area bounds use observed crop pixels. Consumers scale to reference pixels.
    pub signature: MarkerSignature,
    pub frame_position_normalized: Point,
    pub bounding_box: Roi,
    pub pixel_count: u32,
    pub aspect_ratio: f64,
    pub fill_ratio: f64,
    pub clipped: bool,
    pub color_spread: u8,
    pub confidence: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerProposalStatus {
    Proposed,
    Blocked,
    Stale,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerProposalReport {
    pub schema_version: u32,
    pub status: MarkerProposalStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub map_roi: Roi,
    pub observed_map_width: u32,
    pub observed_map_height: u32,
    pub proposals: Vec<MarkerSignatureProposal>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerCalibrationStatus {
    Calibrated,
    Ambiguous,
    Blocked,
    Stale,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerCalibration {
    pub schema_version: u32,
    pub status: MarkerCalibrationStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub map_roi: Roi,
    pub observed_map_width: u32,
    pub observed_map_height: u32,
    /// User click normalized to the full source image, not the map crop.
    pub click: Point,
    pub proposal: Option<MarkerSignatureProposal>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy)]
struct PixelBounds {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

struct Component {
    proposal: MarkerSignatureProposal,
    pixels: Vec<(u32, u32)>,
}

fn is_enclosed_background(image: &RgbaImage, inner: &Component, outer: &Component) -> bool {
    let a = inner.proposal.bounding_box;
    let b = outer.proposal.bounding_box;
    if outer.proposal.fill_ratio >= 0.8
        || a.x <= b.x
        || a.y <= b.y
        || a.x + a.width >= b.x + b.width
        || a.y + a.height >= b.y + b.height
    {
        return false;
    }
    let left = (b.x * f64::from(image.width())).round() as u32;
    let top = (b.y * f64::from(image.height())).round() as u32;
    let right = ((b.x + b.width) * f64::from(image.width())).round() as u32;
    let bottom = ((b.y + b.height) * f64::from(image.height())).round() as u32;
    let center_x = (left + right) / 2;
    let center_y = (top + bottom) / 2;
    let around = [
        (left.saturating_sub(1), center_y),
        (right, center_y),
        (center_x, top.saturating_sub(1)),
        (center_x, bottom),
    ];
    let color = inner.proposal.signature.rgb;
    around
        .iter()
        .filter(|(x, y)| {
            if *x >= image.width() || *y >= image.height() {
                return false;
            }
            let pixel = image.get_pixel(*x, *y);
            pixel[3] >= 128 && (0..3).all(|channel| pixel[channel].abs_diff(color[channel]) <= 36)
        })
        .count()
        >= 2
}

fn bounds(image: &RgbaImage, roi: Roi) -> Result<PixelBounds, String> {
    roi.validate()?;
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Obraz jest pusty lub przekracza limit 64 milionów pikseli".into());
    }
    let (x, y, width, height) = roi
        .pixel_rect(image.width(), image.height())
        .map_err(|_| "ROI nie obejmuje żadnych widocznych pikseli".to_string())?;
    Ok(PixelBounds {
        x,
        y,
        width,
        height,
    })
}

fn proposal_pixel(p: &image::Rgba<u8>, config: &MarkerProposalConfig) -> bool {
    let high = *p.0[..3].iter().max().unwrap();
    let low = *p.0[..3].iter().min().unwrap();
    // Suggest compact chromatic or strongly contrasting components, not every
    // shaded terrain pixel. This is a visual heuristic, not an icon/team model.
    p[3] >= config.min_alpha && ((high >= 70 && high - low >= 32) || low >= 205 || high <= 25)
}

fn components(
    image: &RgbaImage,
    area: PixelBounds,
    config: &MarkerProposalConfig,
) -> Result<Vec<Component>, String> {
    if u64::from(area.width) * u64::from(area.height) > u64::from(config.max_analysis_pixels) {
        return Err(
            "ROI propozycji przekracza limit pikseli; wskaż mniejszy widoczny obszar".into(),
        );
    }
    let mut visited = vec![false; area.width as usize * area.height as usize];
    let mut output = Vec::new();
    let mut component_count = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let index = (y * area.width + x) as usize;
            if visited[index] {
                continue;
            }
            visited[index] = true;
            let anchor = *image.get_pixel(area.x + x, area.y + y);
            if !proposal_pixel(&anchor, config) {
                continue;
            }
            component_count += 1;
            if component_count > config.max_components {
                return Err("Zbyt wiele kolorowych komponentów; propozycje wstrzymane bez wybierania podzbioru".into());
            }
            let mut queue = VecDeque::from([(x, y)]);
            let mut pixels = Vec::new();
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (x, x, y, y);
            let (mut sum_x, mut sum_y, mut count) = (0.0, 0.0, 0u32);
            while let Some((px, py)) = queue.pop_front() {
                count += 1;
                sum_x += f64::from(px) + 0.5;
                sum_y += f64::from(py) + 0.5;
                min_x = min_x.min(px);
                max_x = max_x.max(px);
                min_y = min_y.min(py);
                max_y = max_y.max(py);
                if count <= config.max_component_pixels {
                    pixels.push((area.x + px, area.y + py));
                } else {
                    pixels.clear();
                }
                for ny in py.saturating_sub(1)..=(py + 1).min(area.height - 1) {
                    for nx in px.saturating_sub(1)..=(px + 1).min(area.width - 1) {
                        let i = (ny * area.width + nx) as usize;
                        if visited[i] {
                            continue;
                        }
                        let pixel = image.get_pixel(area.x + nx, area.y + ny);
                        if !proposal_pixel(pixel, config) {
                            visited[i] = true;
                            continue;
                        }
                        if (0..3).all(|channel| {
                            pixel[channel].abs_diff(anchor[channel]) <= config.color_tolerance
                        }) {
                            visited[i] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            let width = max_x - min_x + 1;
            let height = max_y - min_y + 1;
            let aspect = f64::from(width) / f64::from(height);
            let fill = f64::from(count) / (f64::from(width) * f64::from(height));
            if count < config.min_component_pixels
                || count > config.max_component_pixels
                || width > 96
                || height > 96
                || !(0.2..=5.0).contains(&aspect)
                || fill < 0.08
            {
                continue;
            }
            let mut rgb = [0u8; 3];
            for (channel, value) in rgb.iter_mut().enumerate() {
                let mut values: Vec<_> = pixels
                    .iter()
                    .map(|(x, y)| image.get_pixel(*x, *y)[channel])
                    .collect();
                values.sort_unstable();
                *value = values[values.len() / 2];
            }
            let mut deviations: Vec<_> = pixels
                .iter()
                .map(|(x, y)| {
                    (0..3)
                        .map(|channel| image.get_pixel(*x, *y)[channel].abs_diff(rgb[channel]))
                        .max()
                        .unwrap()
                })
                .collect();
            deviations.sort_unstable();
            let spread = deviations[(deviations.len() - 1) * 95 / 100];
            let clipped =
                min_x == 0 || min_y == 0 || max_x + 1 == area.width || max_y + 1 == area.height;
            let signature = MarkerSignature {
                id: format!("visible-{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]),
                role: MarkerRole::Unassigned,
                rgb,
                tolerance: spread.saturating_add(12).clamp(12, 64),
                min_component_pixels: ((f64::from(count) * 0.4).floor() as u32).max(1),
                max_component_pixels: ((f64::from(count) * 2.5).ceil() as u32).min(1_000_000),
                min_aspect_ratio: (aspect * 0.5).max(0.1),
                max_aspect_ratio: aspect * 2.0,
                min_fill_ratio: (fill * 0.5).max(0.04),
            };
            output.push(Component {
                pixels,
                proposal: MarkerSignatureProposal {
                    id: format!(
                        "component-{}-{}-{}-{}",
                        area.x + min_x,
                        area.y + min_y,
                        width,
                        height
                    ),
                    signature,
                    frame_position_normalized: Point {
                        x: (f64::from(area.x) + sum_x / f64::from(count))
                            / f64::from(image.width()),
                        y: (f64::from(area.y) + sum_y / f64::from(count))
                            / f64::from(image.height()),
                    },
                    bounding_box: Roi {
                        x: f64::from(area.x + min_x) / f64::from(image.width()),
                        y: f64::from(area.y + min_y) / f64::from(image.height()),
                        width: f64::from(width) / f64::from(image.width()),
                        height: f64::from(height) / f64::from(image.height()),
                    },
                    pixel_count: count,
                    aspect_ratio: aspect,
                    fill_ratio: fill,
                    clipped,
                    color_spread: spread,
                    confidence: if clipped {
                        0.3
                    } else {
                        (0.55 + 0.2 * fill).min(0.75)
                    },
                },
            });
            if output.len() > config.max_proposals {
                return Err(
                    "Zbyt wiele możliwych ikon; propozycje wstrzymane bez arbitralnego wyboru"
                        .into(),
                );
            }
        }
    }
    Ok(output)
}

pub fn propose_marker_signatures(
    image: &RgbaImage,
    map_roi: Roi,
    timestamp_ms: u64,
    now_ms: u64,
) -> Result<MarkerProposalReport, String> {
    propose_marker_signatures_with_config(
        image,
        map_roi,
        timestamp_ms,
        now_ms,
        &MarkerProposalConfig::default(),
    )
}

pub fn propose_marker_signatures_with_config(
    image: &RgbaImage,
    map_roi: Roi,
    timestamp_ms: u64,
    now_ms: u64,
    config: &MarkerProposalConfig,
) -> Result<MarkerProposalReport, String> {
    let started = Instant::now();
    config.validate()?;
    let area = bounds(image, map_roi)?;
    let mut report = MarkerProposalReport {
        schema_version: 1,
        status: MarkerProposalStatus::Blocked,
        timestamp_ms,
        frame_width: image.width(),
        frame_height: image.height(),
        map_roi,
        observed_map_width: area.width,
        observed_map_height: area.height,
        proposals: Vec::new(),
        warnings: Vec::new(),
    };
    if timestamp_ms > now_ms || now_ms - timestamp_ms > config.max_age_ms {
        report.status = MarkerProposalStatus::Stale;
        report
            .warnings
            .push("Klatka propozycji jest przeterminowana lub ma przyszły czas".into());
        return Ok(report);
    }
    match components(image, area, config) {
        Err(reason) => report.warnings.push(reason),
        Ok(found) if found.is_empty() => report.warnings.push("Brak kompaktowych komponentów o wyraźnym kolorze lub kontraście; ustaw wzorzec ręcznie".into()),
        Ok(found) => {
            report.status = MarkerProposalStatus::Proposed;
            report.proposals = found.into_iter().map(|c| c.proposal).collect();
            report.warnings.push("Propozycje pikselowe wymagają ręcznego wskazania ikony i roli; kolor nie potwierdza drużyny ani celu".into());
        }
    }
    let completed_now =
        now_ms.saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
    if completed_now - timestamp_ms > config.max_age_ms {
        report.status = MarkerProposalStatus::Stale;
        report.proposals.clear();
        report
            .warnings
            .push("Klatka straciła świeżość podczas tworzenia propozycji".into());
    }
    Ok(report)
}

/// Calibrate only the compact visible component explicitly clicked by the user.
/// A hollow center is valid; ties, clipped components and missing colors refuse
/// a signature. Search is local (at most 97x97 px) even on a noisy map.
pub fn calibrate_marker_signature(
    image: &RgbaImage,
    map_roi: Roi,
    click: Point,
    timestamp_ms: u64,
    now_ms: u64,
) -> Result<MarkerCalibration, String> {
    let started = Instant::now();
    let area = bounds(image, map_roi)?;
    if !click.valid() || click.x < 0.0 || click.y < 0.0 || click.x >= 1.0 || click.y >= 1.0 {
        return Err("Kliknięcie wymaga normalizowanych współrzędnych pełnego obrazu".into());
    }
    let mut result = MarkerCalibration {
        schema_version: 1,
        status: MarkerCalibrationStatus::Blocked,
        timestamp_ms,
        frame_width: image.width(),
        frame_height: image.height(),
        map_roi,
        observed_map_width: area.width,
        observed_map_height: area.height,
        click,
        proposal: None,
        warnings: Vec::new(),
    };
    if timestamp_ms > now_ms || now_ms - timestamp_ms > 2000 {
        result.status = MarkerCalibrationStatus::Stale;
        result
            .warnings
            .push("Kliknięcie odnosi się do nieaktualnej klatki; pobierz nowy podgląd".into());
        return Ok(result);
    }
    let px = click.x * f64::from(image.width());
    let py = click.y * f64::from(image.height());
    if px < f64::from(area.x)
        || py < f64::from(area.y)
        || px >= f64::from(area.x + area.width)
        || py >= f64::from(area.y + area.height)
    {
        result
            .warnings
            .push("Kliknięcie leży poza widocznym ROI mapy".into());
        return Ok(result);
    }
    let cx = px.floor() as u32;
    let cy = py.floor() as u32;
    let x = cx.saturating_sub(48).max(area.x);
    let y = cy.saturating_sub(48).max(area.y);
    let local = PixelBounds {
        x,
        y,
        width: (cx.saturating_add(49).min(area.x + area.width)) - x,
        height: (cy.saturating_add(49).min(area.y + area.height)) - y,
    };
    let found = match components(image, local, &MarkerProposalConfig::default()) {
        Ok(found) => found,
        Err(reason) => {
            result.warnings.push(reason);
            return Ok(result);
        }
    };
    let enclosed: Vec<_> = found
        .iter()
        .enumerate()
        .map(|(index, inner)| {
            found
                .iter()
                .enumerate()
                .any(|(other, outer)| other != index && is_enclosed_background(image, inner, outer))
        })
        .collect();
    let mut nearby: Vec<_> = found
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !enclosed[*index])
        .map(|(_, component)| component)
        .filter_map(|component| {
            let box_roi = component.proposal.bounding_box;
            let inside = click.x >= box_roi.x
                && click.y >= box_roi.y
                && click.x < box_roi.x + box_roi.width
                && click.y < box_roi.y + box_roi.height;
            let distance = component
                .pixels
                .iter()
                .map(|(x, y)| (f64::from(*x) + 0.5 - px).hypot(f64::from(*y) + 0.5 - py))
                .fold(f64::INFINITY, f64::min);
            // Empty centers of outlines retain their surrounding visible component.
            let limit = if inside {
                (box_roi.width * f64::from(image.width()))
                    .min(box_roi.height * f64::from(image.height()))
                    * 0.5
                    + 1.0
            } else {
                3.0
            };
            (distance <= limit).then_some((distance, component.proposal))
        })
        .collect();
    nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
    if nearby.is_empty() {
        result.warnings.push(
            "Brak jednoznacznego widocznego komponentu przy kliknięciu; ustaw kolor ręcznie".into(),
        );
    } else if nearby.len() > 1 && nearby[1].0 - nearby[0].0 <= 0.75 {
        result.status = MarkerCalibrationStatus::Ambiguous;
        result
            .warnings
            .push("Kliknięcie pasuje do kilku komponentów; wskaż środek konkretnej ikony".into());
    } else if nearby[0].1.clipped {
        result
            .warnings
            .push("Komponent jest przycięty; pokaż całą ikonę przed kalibracją".into());
    } else {
        result.status = MarkerCalibrationStatus::Calibrated;
        result.proposal = Some(nearby.remove(0).1);
        result.warnings.push("Rola pozostaje nieprzypisana; progi kształtu i koloru są propozycją z widocznych pikseli".into());
    }
    let completed_now =
        now_ms.saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
    if completed_now - timestamp_ms > 2000 {
        result.status = MarkerCalibrationStatus::Stale;
        result.proposal = None;
        result
            .warnings
            .push("Klatka straciła świeżość podczas kalibracji koloru".into());
    }
    Ok(result)
}
