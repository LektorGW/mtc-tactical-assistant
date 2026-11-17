//! Reader for the full MTC map overlay (key M): the 9 x 9 white grid, the
//! "Grid size: N meters" label and the resulting pixel <-> map <-> world mapping.
//! Only visible pixels are used. A map is identified solely by its labelled grid
//! cell size compared with the local catalog; ties stay ambiguous.

use crate::{ocr, preprocess::light_text_for_ocr, preprocess::upscale_for_ocr, Roi};
use image::RgbaImage;
use mtc_maps::{MapCalibration, Point};
use serde::{Deserialize, Serialize};

pub const MTC_GRID_CELLS: u32 = 9;
/// Column labels used by MTC: A–J without I.
pub const MTC_GRID_COLUMNS: [char; 9] = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J'];
const RIDGE_DISTANCE: usize = 2;
const RIDGE_CONTRAST: i32 = 10;
const MIN_LINE_FRACTION: f64 = 0.3;

/// Grid line rectangle in client pixels (line centres, not cell interiors).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapGrid {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub cell_width: f64,
    pub cell_height: f64,
    pub columns: u32,
    pub rows: u32,
    /// Mean fraction of pixels along the 20 grid lines that look like a thin bright line.
    pub line_strength: f64,
    pub frame_width: u32,
    pub frame_height: u32,
}

impl MapGrid {
    pub fn width(&self) -> f64 {
        self.right - self.left
    }
    pub fn height(&self) -> f64 {
        self.bottom - self.top
    }
    /// Client pixel -> map fraction (0..1 from the west/north edge). `None` outside the grid.
    pub fn fraction(&self, x: f64, y: f64) -> Option<Point> {
        let u = (x - self.left) / self.width();
        let v = (y - self.top) / self.height();
        (u.is_finite() && v.is_finite() && (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v))
            .then_some(Point { x: u, y: v })
    }
    /// Map fraction -> client pixel.
    pub fn pixel(&self, fraction: Point) -> Point {
        Point {
            x: self.left + fraction.x * self.width(),
            y: self.top + fraction.y * self.height(),
        }
    }
}

/// Sector label such as `C7`: columns A–J (no I) west to east, rows 1–9 north to south.
pub fn grid_sector(fraction: Point) -> Option<String> {
    if !(0.0..=1.0).contains(&fraction.x) || !(0.0..=1.0).contains(&fraction.y) {
        return None;
    }
    let column = ((fraction.x * 9.0).floor() as usize).min(8);
    let row = ((fraction.y * 9.0).floor() as usize).min(8) + 1;
    Some(format!("{}{row}", MTC_GRID_COLUMNS[column]))
}

fn whiteness(image: &RgbaImage) -> Vec<i32> {
    image
        .pixels()
        .map(|p| i32::from(p[0].min(p[1]).min(p[2])))
        .collect()
}

/// Fraction of rows (or columns) in the window that show a thin bright line.
fn ridge_profile(
    white: &[i32],
    width: usize,
    window: (usize, usize, usize, usize),
    vertical_lines: bool,
) -> Vec<f64> {
    let (x0, y0, x1, y1) = window;
    let d = RIDGE_DISTANCE;
    let at = |x: usize, y: usize| white[y * width + x];
    if vertical_lines {
        let mut profile = vec![0.0; width];
        let rows = (y1 - y0).max(1) as f64;
        for (x, value) in profile
            .iter_mut()
            .enumerate()
            .take(x1.min(width - d))
            .skip(x0.max(d))
        {
            let mut count = 0u32;
            for y in y0..y1 {
                let c = at(x, y);
                if c - at(x - d, y).max(at(x + d, y)) > RIDGE_CONTRAST {
                    count += 1;
                }
            }
            *value = f64::from(count) / rows;
        }
        profile
    } else {
        let height = white.len() / width;
        let mut profile = vec![0.0; height];
        let columns = (x1 - x0).max(1) as f64;
        for (y, value) in profile
            .iter_mut()
            .enumerate()
            .take(y1.min(height - d))
            .skip(y0.max(d))
        {
            let mut count = 0u32;
            for x in x0..x1 {
                let c = at(x, y);
                if c - at(x, y - d).max(at(x, y + d)) > RIDGE_CONTRAST {
                    count += 1;
                }
            }
            *value = f64::from(count) / columns;
        }
        profile
    }
}

fn line_value(profile: &[f64], position: f64) -> f64 {
    let i = position.round() as isize;
    (i - 1..=i + 1)
        .filter(|j| *j >= 0 && (*j as usize) < profile.len())
        .map(|j| profile[j as usize])
        .fold(0.0, f64::max)
}

/// Best lattice of 10 equally spaced lines: (offset, spacing, score).
fn best_lattice(profile: &[f64], min_spacing: f64, max_spacing: f64) -> Option<(f64, f64, f64)> {
    let lines = MTC_GRID_CELLS as f64;
    let length = profile.len() as f64;
    let score = |offset: f64, spacing: f64| {
        (0..=MTC_GRID_CELLS)
            .map(|k| line_value(profile, offset + f64::from(k) * spacing))
            .sum::<f64>()
    };
    let mut best: Option<(f64, f64, f64)> = None;
    let mut spacing = min_spacing;
    while spacing <= max_spacing {
        let span = spacing * lines;
        let mut offset = 1.0;
        while offset + span < length - 1.0 {
            let s = score(offset, spacing);
            if best.is_none_or(|b| s > b.2 + 1e-9) {
                best = Some((offset, spacing, s));
            }
            offset += 1.0;
        }
        spacing += 0.5;
    }
    // Refine spacing and offset around the coarse optimum.
    let (o, s, _) = best?;
    let mut refined = best;
    let mut spacing = s - 0.5;
    while spacing <= s + 0.5 {
        let mut offset = o - 2.0;
        while offset <= o + 2.0 {
            if offset >= 0.0 && offset + spacing * lines < length {
                let value = score(offset, spacing);
                if refined.is_none_or(|b| value > b.2 + 1e-9) {
                    refined = Some((offset, spacing, value));
                }
            }
            offset += 0.25;
        }
        spacing += 0.05;
    }
    refined
}

/// Sub-pixel line centres near the lattice, then a least-squares fit.
fn fit_lines(profile: &[f64], offset: f64, spacing: f64) -> (f64, f64, f64) {
    let mut points = Vec::new();
    let mut strength = 0.0;
    for k in 0..=MTC_GRID_CELLS {
        let expected = offset + f64::from(k) * spacing;
        let centre = expected.round() as isize;
        let window: Vec<(f64, f64)> = (centre - 2..=centre + 2)
            .filter(|j| *j >= 0 && (*j as usize) < profile.len())
            .map(|j| (j as f64, profile[j as usize]))
            .collect();
        let peak = window.iter().map(|w| w.1).fold(0.0, f64::max);
        strength += peak;
        if peak >= MIN_LINE_FRACTION {
            let sum: f64 = window
                .iter()
                .filter(|w| w.1 >= peak * 0.5)
                .map(|w| w.1)
                .sum();
            let position = window
                .iter()
                .filter(|w| w.1 >= peak * 0.5)
                .map(|w| w.0 * w.1)
                .sum::<f64>()
                / sum;
            points.push((f64::from(k), position));
        }
    }
    strength /= f64::from(MTC_GRID_CELLS + 1);
    if points.len() < 6 {
        return (offset, spacing, strength);
    }
    let n = points.len() as f64;
    let mean_k = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_x = points.iter().map(|p| p.1).sum::<f64>() / n;
    let slope = points
        .iter()
        .map(|p| (p.0 - mean_k) * (p.1 - mean_x))
        .sum::<f64>()
        / points.iter().map(|p| (p.0 - mean_k).powi(2)).sum::<f64>();
    (mean_x - slope * mean_k, slope, strength)
}

fn strong_lines(profile: &[f64], offset: f64, spacing: f64) -> usize {
    (0..=MTC_GRID_CELLS)
        .filter(|k| line_value(profile, offset + f64::from(*k) * spacing) >= MIN_LINE_FRACTION)
        .count()
}

/// Finds the 9 x 9 grid of thin bright lines. The result is `None` unless at
/// least 8 of the 10 lines in each direction are clearly visible and cell
/// interiors are not line-like (a uniform texture or HUD stripes are refused).
pub fn detect_map_grid(image: &RgbaImage) -> Option<MapGrid> {
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width < 120 || height < 120 || width * height > 40_000_000 {
        return None;
    }
    let white = whiteness(image);
    let short = width.min(height) as f64;
    let (min_spacing, max_spacing) = ((short / 90.0).max(6.0), short / 9.6);
    let full = (0, 0, width, height);
    let columns = ridge_profile(&white, width, full, true);
    let (x_offset, x_spacing, _) = best_lattice(&columns, min_spacing, max_spacing)?;
    let (left, right) = (x_offset, x_offset + 9.0 * x_spacing);
    // A few pixels of slack: the coarse lattice may sit 1–3 px inside the true
    // border lines, which must still be measured.
    let band = (
        (left - 4.0).floor().max(0.0) as usize,
        0,
        ((right + 5.0).ceil() as usize).min(width),
        height,
    );
    let rows = ridge_profile(&white, width, band, false);
    let (y_offset, y_spacing, _) = best_lattice(&rows, min_spacing, max_spacing)?;
    // Re-measure both directions inside the found rectangle only.
    let (top, bottom) = (y_offset, y_offset + 9.0 * y_spacing);
    let inside = (
        band.0,
        (top - 4.0).floor().max(0.0) as usize,
        band.2,
        ((bottom + 5.0).ceil() as usize).min(height),
    );
    let columns = ridge_profile(&white, width, inside, true);
    let rows = ridge_profile(&white, width, inside, false);
    let (x_offset, x_spacing, x_strength) = fit_lines(&columns, x_offset, x_spacing);
    let (y_offset, y_spacing, y_strength) = fit_lines(&rows, y_offset, y_spacing);
    if strong_lines(&columns, x_offset, x_spacing) < 8
        || strong_lines(&rows, y_offset, y_spacing) < 8
    {
        return None;
    }
    // An edge line hidden by an overlay (taskbar, window) lets a lattice shifted by
    // one cell fit as well as the true one. Both outer border lines must therefore
    // be visible; otherwise the grid is refused instead of guessed.
    for (profile, offset, spacing) in [
        (&columns, x_offset, x_spacing),
        (&rows, y_offset, y_spacing),
    ] {
        if line_value(profile, offset) < MIN_LINE_FRACTION
            || line_value(profile, offset + 9.0 * spacing) < MIN_LINE_FRACTION
        {
            return None;
        }
    }
    // Midpoints between lines must be clearly weaker than the lines themselves.
    let interior = |profile: &[f64], offset: f64, spacing: f64| {
        (0..MTC_GRID_CELLS)
            .map(|k| line_value(profile, offset + (f64::from(k) + 0.5) * spacing))
            .sum::<f64>()
            / f64::from(MTC_GRID_CELLS)
    };
    if interior(&columns, x_offset, x_spacing) > x_strength * 0.5
        || interior(&rows, y_offset, y_spacing) > y_strength * 0.5
    {
        return None;
    }
    Some(MapGrid {
        left: x_offset,
        top: y_offset,
        right: x_offset + 9.0 * x_spacing,
        bottom: y_offset + 9.0 * y_spacing,
        cell_width: x_spacing,
        cell_height: y_spacing,
        columns: MTC_GRID_CELLS,
        rows: MTC_GRID_CELLS,
        line_strength: (x_strength + y_strength) / 2.0,
        frame_width: image.width(),
        frame_height: image.height(),
    })
}

/// Parses the number in "Grid size: 324 meters" from imperfect OCR text.
pub fn parse_grid_size(text: &str) -> Option<f64> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)s\S?[i1l|]\S?z\S?e\S{0,2}\s*([0-9]{2,4}(?:[.,][0-9]+)?)\s+m")
            .expect("Valid grid size regex")
    });
    let captures: Vec<_> = pattern.captures_iter(text).collect();
    if captures.len() != 1 {
        return None;
    }
    captures[0][1]
        .replace(',', ".")
        .parse()
        .ok()
        .filter(|v: &f64| (20.0..=5000.0).contains(v))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSizeReading {
    pub meters: Option<f64>,
    /// Every OCR attempt, in order, for diagnostics.
    pub attempts: Vec<String>,
    /// Number of attempts that agree with `meters`.
    pub agreeing: u32,
}

/// OCRs the label below the grid's lower-left corner. A value needs at least
/// two agreeing attempts out of several preprocessings and scales.
pub fn read_grid_size_label<F>(
    image: &RgbaImage,
    grid: &MapGrid,
    mut recognize: F,
) -> GridSizeReading
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let (w, h) = (f64::from(image.width()), f64::from(image.height()));
    let x = (grid.left - grid.cell_width * 0.1).max(0.0);
    let y = (grid.bottom + grid.cell_height * 0.05).min(h - 1.0);
    let roi = Roi {
        x: x / w,
        y: y / h,
        width: ((grid.width() * 0.45).min(w - x) / w).max(1.0 / w),
        height: ((grid.cell_height * 0.4).min(h - y) / h).max(1.0 / h),
    };
    let mut attempts = Vec::new();
    let mut values: Vec<f64> = Vec::new();
    if let Ok(crop) = roi.crop(image) {
        for scale in [2u32, 3, 4] {
            for light in [false, true] {
                let input = if light {
                    light_text_for_ocr(&crop, scale)
                } else {
                    upscale_for_ocr(&crop, scale)
                };
                let Ok(input) = input else { continue };
                let Ok(text) = recognize(&input) else {
                    continue;
                };
                if let Some(value) = parse_grid_size(&text) {
                    values.push(value);
                }
                attempts.push(text);
            }
        }
    }
    let mut best: Option<(f64, u32)> = None;
    for value in &values {
        let count = values.iter().filter(|v| (*v - value).abs() < 0.01).count() as u32;
        if best.is_none_or(|b| count > b.1) {
            best = Some((*value, count));
        }
    }
    match best {
        Some((meters, agreeing)) if agreeing >= 2 => GridSizeReading {
            meters: Some(meters),
            attempts,
            agreeing,
        },
        _ => GridSizeReading {
            meters: None,
            attempts,
            agreeing: best.map_or(0, |b| b.1),
        },
    }
}

/// One catalog entry: id, display name and full side length in game meters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCatalogEntry {
    pub id: String,
    pub name: String,
    pub size_game_meters: f64,
    /// Optional second side for non-square maps; `None` means square.
    #[serde(default)]
    pub height_game_meters: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCandidate {
    pub id: String,
    pub name: String,
    pub size_game_meters: f64,
    pub height_game_meters: f64,
    pub cell_meters: f64,
    pub delta_meters: f64,
}

/// Catalog maps whose cell (side / 9) is within `tolerance` of the label.
/// Several maps can share a size; the caller must then keep a candidate list.
pub fn identify_map_by_cell_size(
    cell_meters: f64,
    catalog: &[MapCatalogEntry],
    tolerance: f64,
) -> Vec<MapCandidate> {
    let mut candidates: Vec<MapCandidate> = catalog
        .iter()
        .filter(|entry| entry.size_game_meters.is_finite() && entry.size_game_meters > 0.0)
        .map(|entry| {
            let cell = entry.size_game_meters / 9.0;
            MapCandidate {
                id: entry.id.clone(),
                name: entry.name.clone(),
                size_game_meters: entry.size_game_meters,
                height_game_meters: entry.height_game_meters.unwrap_or(entry.size_game_meters),
                cell_meters: cell,
                delta_meters: (cell - cell_meters).abs(),
            }
        })
        .filter(|candidate| candidate.delta_meters <= tolerance)
        .collect();
    candidates.sort_by(|a, b| {
        a.delta_meters
            .total_cmp(&b.delta_meters)
            .then(a.id.cmp(&b.id))
    });
    candidates
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MtcMapStatus {
    /// Grid found and exactly one catalog map matches the labelled cell size.
    Ready,
    /// Grid found, several maps share the labelled cell size.
    Ambiguous,
    /// Grid found, but no readable label or no matching catalog map.
    Unidentified,
    /// No full-map grid visible.
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MtcMapReading {
    pub schema_version: u32,
    pub status: MtcMapStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub grid: Option<MapGrid>,
    pub grid_size_meters: Option<f64>,
    pub grid_size_ocr: Vec<String>,
    pub map_id: Option<String>,
    pub candidates: Vec<MapCandidate>,
    /// Side lengths of the identified map in game meters (east-west, north-south).
    pub map_width_meters: Option<f64>,
    pub map_height_meters: Option<f64>,
    /// Client-pixel calibration: world x = meters east of the west edge,
    /// world y = meters north of the south edge. Only for square pixels/cells.
    pub calibration: Option<MapCalibration>,
    pub warnings: Vec<String>,
}

pub const MAP_CELL_TOLERANCE_METERS: f64 = 1.5;

pub fn read_mtc_full_map(
    image: &RgbaImage,
    timestamp_ms: u64,
    catalog: &[MapCatalogEntry],
) -> MtcMapReading {
    read_mtc_full_map_with_ocr(image, timestamp_ms, catalog, ocr)
}

pub fn read_mtc_full_map_with_ocr<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    catalog: &[MapCatalogEntry],
    recognize: F,
) -> MtcMapReading
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let mut reading = MtcMapReading {
        schema_version: 1,
        status: MtcMapStatus::Blocked,
        timestamp_ms,
        frame_width: image.width(),
        frame_height: image.height(),
        grid: None,
        grid_size_meters: None,
        grid_size_ocr: Vec::new(),
        map_id: None,
        candidates: Vec::new(),
        map_width_meters: None,
        map_height_meters: None,
        calibration: None,
        warnings: Vec::new(),
    };
    let Some(grid) = detect_map_grid(image) else {
        reading
            .warnings
            .push("Nie widać pełnej mapy z siatką 9 × 9 (klawisz M)".into());
        return reading;
    };
    reading.grid = Some(grid);
    reading.status = MtcMapStatus::Unidentified;
    let label = read_grid_size_label(image, &grid, recognize);
    reading.grid_size_ocr = label.attempts;
    let Some(meters) = label.meters else {
        reading.warnings.push(
            "Nie odczytano zgodnie napisu „Grid size: N meters”; mapa nie jest zgadywana".into(),
        );
        return reading;
    };
    reading.grid_size_meters = Some(meters);
    reading.candidates = identify_map_by_cell_size(meters, catalog, MAP_CELL_TOLERANCE_METERS);
    match reading.candidates.len() {
        0 => {
            reading.warnings.push(format!(
                "Żadna mapa katalogu nie ma pola {meters} m (±{MAP_CELL_TOLERANCE_METERS} m)"
            ));
            return reading;
        }
        1 => {}
        _ => {
            reading.status = MtcMapStatus::Ambiguous;
            reading
                .warnings
                .push("Kilka map katalogu ma ten sam rozmiar pola; wybierz mapę ręcznie".into());
            return reading;
        }
    }
    let map = reading.candidates[0].clone();
    reading.status = MtcMapStatus::Ready;
    reading.map_id = Some(map.id.clone());
    reading.map_width_meters = Some(map.size_game_meters);
    reading.map_height_meters = Some(map.height_game_meters);
    let x_scale = map.size_game_meters / grid.width();
    let y_scale = map.height_game_meters / grid.height();
    if (x_scale / y_scale - 1.0).abs() <= 0.02 {
        reading.calibration = Some(MapCalibration {
            units_per_pixel: (x_scale + y_scale) / 2.0,
            north_rotation_deg: 0.0,
            origin_pixel: Point {
                x: grid.left,
                y: grid.bottom,
            },
            origin_world: Point { x: 0.0, y: 0.0 },
            // About one pixel of line-fit error at each end.
            uncertainty_units: (x_scale + y_scale),
        });
    } else {
        reading.warnings.push(
            "Skale pikseli w osiach różnią się o ponad 2%; użyj przeliczenia ułamkowego zamiast kalibracji".into(),
        );
    }
    reading
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapGridPoint {
    pub fraction: Point,
    /// Meters east of the west edge and north of the south edge.
    pub world: Option<Point>,
    pub sector: String,
}

/// Client pixel -> fraction, sector and (with known map size) world meters.
pub fn map_grid_point(
    grid: &MapGrid,
    map_size: Option<(f64, f64)>,
    pixel: Point,
) -> Option<MapGridPoint> {
    let fraction = grid.fraction(pixel.x, pixel.y)?;
    Some(MapGridPoint {
        sector: grid_sector(fraction)?,
        world: map_size.map(|(w, h)| Point {
            x: fraction.x * w,
            y: (1.0 - fraction.y) * h,
        }),
        fraction,
    })
}

/// World meters (east of west edge, north of south edge) -> client pixel.
pub fn map_world_to_pixel(grid: &MapGrid, map_size: (f64, f64), world: Point) -> Option<Point> {
    let fraction = Point {
        x: world.x / map_size.0,
        y: 1.0 - world.y / map_size.1,
    };
    ((0.0..=1.0).contains(&fraction.x) && (0.0..=1.0).contains(&fraction.y))
        .then(|| grid.pixel(fraction))
}

// ---------------------------------------------------------------------------
// Own position: the white player arrow on the minimap or the full map.
// ---------------------------------------------------------------------------

/// Chevron of the player arrow pointing north, centred on its area centroid,
/// in 1080p minimap pixels (22 px long, 16 px wide, rear notch 6 px deep).
const ARROW_POLYGON: [(f64, f64); 4] = [(0.0, -12.67), (8.0, 9.33), (0.0, 3.33), (-8.0, 9.33)];
const ARROW_WINDOW: usize = 41;
/// Minimum IoU between a bright component and the rotated chevron.
pub const OWN_ARROW_MIN_SCORE: f64 = 0.72;
/// Arrow heading and compass heading must agree within this many degrees.
pub const OWN_ARROW_COMPASS_TOLERANCE_DEG: f64 = 20.0;

fn point_in_polygon(x: f64, y: f64, polygon: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Rasterized chevron rotated clockwise by `angle_deg` (screen coordinates).
fn arrow_template(angle_deg: f64, scale: f64) -> Vec<bool> {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let polygon: Vec<(f64, f64)> = ARROW_POLYGON
        .iter()
        .map(|(x, y)| ((x * cos - y * sin) * scale, (x * sin + y * cos) * scale))
        .collect();
    let half = (ARROW_WINDOW / 2) as f64;
    let mut out = vec![false; ARROW_WINDOW * ARROW_WINDOW];
    for (index, cell) in out.iter_mut().enumerate() {
        let (px, py) = (
            (index % ARROW_WINDOW) as f64 - half,
            (index / ARROW_WINDOW) as f64 - half,
        );
        let mut hits = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let x = px - 0.375 + f64::from(sx) * 0.25;
                let y = py - 0.375 + f64::from(sy) * 0.25;
                hits += usize::from(point_in_polygon(x, y, &polygon));
            }
        }
        *cell = hits >= 8;
    }
    out
}

fn arrow_templates(scale: f64) -> Vec<(f64, f64, Vec<bool>)> {
    let mut out = Vec::new();
    for step in 0..72 {
        let angle = f64::from(step) * 5.0;
        for factor in [0.9, 1.0, 1.1] {
            out.push((angle, factor, arrow_template(angle, scale * factor)));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnArrowCandidate {
    /// Area centroid in client pixels.
    pub pixel: Point,
    pub area: u32,
    /// IoU with the best rotated chevron (0–1).
    pub shape_score: f64,
    /// Arrow (body) heading, degrees clockwise from map north.
    pub heading_deg: f64,
}

/// Bright desaturated components inside the grid matched against a rotated chevron.
/// `ui_scale` is the HUD scale of the source client frame (client height / 1080);
/// crops of a frame keep the scale of that frame.
pub fn own_arrow_candidates(
    image: &RgbaImage,
    grid: &MapGrid,
    ui_scale: f64,
) -> Vec<OwnArrowCandidate> {
    let (w, h) = (image.width() as i64, image.height() as i64);
    let scale = if ui_scale.is_finite() && ui_scale > 0.2 {
        ui_scale
    } else {
        1.0
    };
    let margin = grid.cell_width * 0.3;
    let x0 = ((grid.left - margin).floor() as i64).clamp(0, w);
    let x1 = ((grid.right + margin).ceil() as i64).clamp(0, w);
    let y0 = ((grid.top - margin).floor() as i64).clamp(0, h);
    let y1 = ((grid.bottom + margin).ceil() as i64).clamp(0, h);
    if x1 - x0 < 20 || y1 - y0 < 20 {
        return Vec::new();
    }
    let (rw, rh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let pixel =
        |x: usize, y: usize| image.get_pixel((x0 as usize + x) as u32, (y0 as usize + y) as u32);
    let neutral = |p: &image::Rgba<u8>| {
        let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        (hi - lo < 45).then_some(lo)
    };
    // Adaptive brightness: the arrow is the whitest neutral element even when the
    // whole minimap is dimmed (damage/darkness overlays).
    let mut histogram = [0u32; 256];
    let mut total = 0u32;
    for y in 0..rh {
        for x in 0..rw {
            if let Some(v) = neutral(pixel(x, y)) {
                histogram[usize::from(v)] += 1;
                total += 1;
            }
        }
    }
    if total == 0 {
        return Vec::new();
    }
    let target = (f64::from(total) * 0.998).floor() as u32;
    let (mut seen, mut p998) = (0u32, 255usize);
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen > target {
            p998 = value;
            break;
        }
    }
    let threshold = (p998 as f64 * 0.75).max(90.0);
    let mask: Vec<bool> = (0..rw * rh)
        .map(|i| neutral(pixel(i % rw, i / rw)).is_some_and(|v| f64::from(v) > threshold))
        .collect();
    let (min_area, max_area) = (80.0 * scale * scale, 260.0 * scale * scale);
    let templates = arrow_templates(scale);
    let half = (ARROW_WINDOW / 2) as i64;
    let mut seen_px = vec![false; rw * rh];
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    for start in 0..rw * rh {
        if seen_px[start] || !mask[start] {
            continue;
        }
        seen_px[start] = true;
        queue.clear();
        queue.push_back(start);
        let (mut sx, mut sy, mut area) = (0.0, 0.0, 0u32);
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % rw, i / rw);
            sx += x as f64;
            sy += y as f64;
            area += 1;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= rw as i64 || ny >= rh as i64 {
                        continue;
                    }
                    let ni = ny as usize * rw + nx as usize;
                    if !seen_px[ni] && mask[ni] {
                        seen_px[ni] = true;
                        queue.push_back(ni);
                    }
                }
            }
        }
        let area_f = f64::from(area);
        if area_f < min_area || area_f > max_area {
            continue;
        }
        let (cx, cy) = (sx / area_f, sy / area_f);
        let mut best = (0.0f64, 0.0f64);
        for oy in -2i64..=2 {
            for ox in -2i64..=2 {
                let (wx, wy) = (cx.round() as i64 - half + ox, cy.round() as i64 - half + oy);
                let window: Vec<bool> = (0..ARROW_WINDOW * ARROW_WINDOW)
                    .map(|k| {
                        let (x, y) = (
                            wx + (k % ARROW_WINDOW) as i64,
                            wy + (k / ARROW_WINDOW) as i64,
                        );
                        x >= 0
                            && y >= 0
                            && x < rw as i64
                            && y < rh as i64
                            && mask[y as usize * rw + x as usize]
                    })
                    .collect();
                for (angle, _, template) in &templates {
                    let (mut inter, mut union) = (0u32, 0u32);
                    for (a, b) in window.iter().zip(template) {
                        inter += u32::from(*a && *b);
                        union += u32::from(*a || *b);
                    }
                    let iou = if union == 0 {
                        0.0
                    } else {
                        f64::from(inter) / f64::from(union)
                    };
                    if iou > best.0 {
                        best = (iou, *angle);
                    }
                }
            }
        }
        out.push(OwnArrowCandidate {
            pixel: Point {
                x: x0 as f64 + cx,
                y: y0 as f64 + cy,
            },
            area,
            shape_score: best.0,
            heading_deg: best.1,
        });
    }
    out.sort_by(|a, b| b.shape_score.total_cmp(&a.shape_score));
    out
}

fn angle_difference(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnPositionStatus {
    /// One arrow; its heading agrees with the compass (±20°).
    Ready,
    /// One arrow, but the compass is unreadable or points elsewhere (free look).
    Unconfirmed,
    /// Several arrow-shaped components.
    Ambiguous,
    /// Grid visible, no arrow (hidden under another icon, outside, too dark).
    NotFound,
    /// No map grid visible.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnMarker {
    pub pixel: Point,
    pub fraction: Point,
    pub sector: String,
    /// Meters east of the west edge and north of the south edge (known map only).
    pub world: Option<Point>,
    pub heading_deg: f64,
    pub shape_score: f64,
    pub compass_deg: Option<f64>,
    pub compass_agrees: Option<bool>,
    /// Heuristic: 0.6 (compass agrees) / 0.45 (no compass) / 0.3 (disagrees);
    /// 0.9 / 0.75 after two consistent fresh frames. Not a probability.
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnPositionReading {
    pub schema_version: u32,
    pub status: OwnPositionStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub grid: Option<MapGrid>,
    pub map_id: Option<String>,
    pub marker: Option<OwnMarker>,
    pub candidates: Vec<OwnArrowCandidate>,
    pub warnings: Vec<String>,
}

/// Map context: id and side lengths in game meters of the map shown by the grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnMapContext {
    pub map_id: String,
    pub width_meters: f64,
    pub height_meters: f64,
}

/// Pure decision over a detected grid, candidates and an optional compass value.
pub fn decide_own_position(
    timestamp_ms: u64,
    frame: (u32, u32),
    grid: Option<MapGrid>,
    candidates: Vec<OwnArrowCandidate>,
    compass_deg: Option<f64>,
    map: Option<&OwnMapContext>,
) -> OwnPositionReading {
    let mut reading = OwnPositionReading {
        schema_version: 1,
        status: OwnPositionStatus::Blocked,
        timestamp_ms,
        frame_width: frame.0,
        frame_height: frame.1,
        grid,
        map_id: map.map(|m| m.map_id.clone()),
        marker: None,
        candidates,
        warnings: Vec::new(),
    };
    let Some(grid) = grid else {
        reading
            .warnings
            .push("Nie widać minimapy ani mapy z siatką 9 × 9".into());
        return reading;
    };
    let arrows: Vec<&OwnArrowCandidate> = reading
        .candidates
        .iter()
        .filter(|c| c.shape_score >= OWN_ARROW_MIN_SCORE)
        .collect();
    match arrows.len() {
        0 => {
            reading.status = OwnPositionStatus::NotFound;
            reading.warnings.push(
                "Nie znaleziono białej strzałki gracza (może być zasłonięta ikoną pojazdu)".into(),
            );
            return reading;
        }
        1 => {}
        _ => {
            reading.status = OwnPositionStatus::Ambiguous;
            reading
                .warnings
                .push("Kilka obiektów w kształcie strzałki — pozycja nie jest wybierana".into());
            return reading;
        }
    }
    let arrow = arrows[0].clone();
    let Some(fraction) = grid.fraction(arrow.pixel.x, arrow.pixel.y) else {
        reading.status = OwnPositionStatus::NotFound;
        reading
            .warnings
            .push("Strzałka leży poza siatką mapy".into());
        return reading;
    };
    let compass_agrees = compass_deg
        .map(|c| angle_difference(c, arrow.heading_deg) <= OWN_ARROW_COMPASS_TOLERANCE_DEG);
    let (status, confidence) = match compass_agrees {
        Some(true) => (OwnPositionStatus::Ready, 0.6),
        None => (OwnPositionStatus::Unconfirmed, 0.45),
        Some(false) => (OwnPositionStatus::Unconfirmed, 0.3),
    };
    match compass_agrees {
        None => reading.warnings.push("Kompas nieczytelny — kierunek strzałki niepotwierdzony".into()),
        Some(false) => reading.warnings.push(
            "Kierunek strzałki różni się od kompasu o ponad 20° (kamera obrócona?) — pozycja niepotwierdzona".into(),
        ),
        Some(true) => {}
    }
    if map.is_none() {
        reading
            .warnings
            .push("Nie wybrano mapy — brak współrzędnych świata (tylko ułamek i sektor)".into());
    }
    reading.status = status;
    reading.marker = Some(OwnMarker {
        pixel: arrow.pixel,
        sector: grid_sector(fraction).unwrap_or_default(),
        world: map.map(|m| Point {
            x: fraction.x * m.width_meters,
            y: (1.0 - fraction.y) * m.height_meters,
        }),
        fraction,
        heading_deg: arrow.heading_deg,
        shape_score: arrow.shape_score,
        compass_deg,
        compass_agrees,
        confidence,
    });
    reading
}

/// Compass number at the top centre of the HUD (1080p layout).
pub fn read_compass_heading<F>(image: &RgbaImage, recognize: F) -> Option<f64>
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let roi = Roi {
        x: 0.475,
        y: 0.083,
        width: 0.05,
        height: 0.033,
    };
    let (text, _) = crate::read_roi_text(image, roi, 2, crate::OcrKind::Heading, recognize).ok()?;
    let value = crate::parse_numeric("heading", &text, 0)?.value;
    // The centre number has three digits (e.g. "094"); a partial read is refused.
    (text.chars().filter(char::is_ascii_digit).count() == 3).then_some(value)
}

pub fn read_own_position(
    image: &RgbaImage,
    timestamp_ms: u64,
    map: Option<&OwnMapContext>,
) -> OwnPositionReading {
    read_own_position_with(image, timestamp_ms, map, ocr)
}

pub fn read_own_position_with<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    map: Option<&OwnMapContext>,
    recognize: F,
) -> OwnPositionReading
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let grid = detect_map_grid(image);
    let ui_scale = f64::from(image.height()) / 1080.0;
    let candidates = grid
        .map(|g| own_arrow_candidates(image, &g, ui_scale))
        .unwrap_or_default();
    let compass = if candidates
        .iter()
        .any(|c| c.shape_score >= OWN_ARROW_MIN_SCORE)
    {
        read_compass_heading(image, recognize)
    } else {
        None
    };
    decide_own_position(
        timestamp_ms,
        (image.width(), image.height()),
        grid,
        candidates,
        compass,
        map,
    )
}

/// Two fresh readings (≤ 4 s apart) of one arrow that moved at most a third of
/// a grid cell raise the confidence (0.9 with compass agreement in both).
pub fn stabilize_own_position(
    current: &mut OwnPositionReading,
    previous: Option<&OwnPositionReading>,
) {
    let (Some(previous), Some(marker)) = (previous, current.marker.as_mut()) else {
        return;
    };
    let Some(old) = previous.marker.as_ref() else {
        return;
    };
    if previous.timestamp_ms >= current.timestamp_ms
        || current.timestamp_ms - previous.timestamp_ms > 4000
        || previous.map_id != current.map_id
        || previous.frame_width != current.frame_width
        || previous.frame_height != current.frame_height
    {
        return;
    }
    let moved = (marker.fraction.x - old.fraction.x).hypot(marker.fraction.y - old.fraction.y);
    if moved > 1.0 / 27.0 {
        return;
    }
    marker.confidence = match (marker.compass_agrees, old.compass_agrees) {
        (Some(true), Some(true)) => 0.9,
        (Some(false), _) | (_, Some(false)) => marker.confidence.max(0.5),
        _ => 0.75,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(width: u32, height: u32, left: u32, top: u32, cell: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let on_x = x >= left && x <= left + 9 * cell && (x - left).is_multiple_of(cell);
            let on_y = y >= top && y <= top + 9 * cell && (y - top).is_multiple_of(cell);
            let inside = x >= left && x <= left + 9 * cell && y >= top && y <= top + 9 * cell;
            let texture = ((x * 7 + y * 13) % 23) as u8;
            if inside && (on_x || on_y) {
                image::Rgba([200, 210, 200, 255])
            } else {
                image::Rgba([60 + texture, 90 + texture, 40, 255])
            }
        })
    }

    fn minimap_with_arrow(angle: f64) -> RgbaImage {
        // Faint grid lines as on the real minimap (much darker than icons).
        let mut image = RgbaImage::from_fn(380, 380, |x, y| {
            let on = |v: u32| (20..=20 + 9 * 36).contains(&v) && (v - 20).is_multiple_of(36);
            let inside = (20..=344).contains(&x) && (20..=344).contains(&y);
            let t = ((x * 7 + y * 13) % 23) as u8;
            if inside && (on(x) || on(y)) {
                image::Rgba([110, 115, 110, 255])
            } else {
                image::Rgba([60 + t, 90 + t, 40, 255])
            }
        });
        // White vehicle-like rectangle that must not count as an arrow.
        for y in 250..266 {
            for x in 250..272 {
                image.put_pixel(x, y, image::Rgba([235, 235, 235, 255]));
            }
        }
        let template = arrow_template(angle, 1.0);
        let half = (ARROW_WINDOW / 2) as u32;
        for (k, on) in template.iter().enumerate() {
            if *on {
                let (x, y) = (
                    150 + (k % ARROW_WINDOW) as u32 - half,
                    160 + (k / ARROW_WINDOW) as u32 - half,
                );
                image.put_pixel(x, y, image::Rgba([240, 240, 240, 255]));
            }
        }
        image
    }

    fn rohkstov() -> OwnMapContext {
        OwnMapContext {
            map_id: "rohkstov".into(),
            width_meters: 5398.56,
            height_meters: 5398.56,
        }
    }

    #[test]
    fn finds_one_rotated_player_arrow_and_ignores_white_rectangles() {
        let image = minimap_with_arrow(135.0);
        let grid = detect_map_grid(&image).unwrap();
        let candidates = own_arrow_candidates(&image, &grid, 1.0);
        let arrows: Vec<_> = candidates
            .iter()
            .filter(|c| c.shape_score >= OWN_ARROW_MIN_SCORE)
            .collect();
        assert_eq!(arrows.len(), 1, "{candidates:?}");
        assert!(angle_difference(arrows[0].heading_deg, 135.0) <= 5.0);
        assert!((arrows[0].pixel.x - 150.0).abs() < 2.5 && (arrows[0].pixel.y - 160.0).abs() < 2.5);
        let ready = decide_own_position(
            5,
            image.dimensions(),
            Some(grid),
            candidates.clone(),
            Some(140.0),
            Some(&rohkstov()),
        );
        assert_eq!(ready.status, OwnPositionStatus::Ready);
        let marker = ready.marker.as_ref().unwrap();
        assert_eq!(marker.sector, "D4");
        assert_eq!(marker.confidence, 0.6);
        let free_look = decide_own_position(
            5,
            image.dimensions(),
            Some(grid),
            candidates.clone(),
            Some(300.0),
            None,
        );
        assert_eq!(free_look.status, OwnPositionStatus::Unconfirmed);
        assert!(free_look.marker.as_ref().unwrap().world.is_none());
        let two = decide_own_position(
            5,
            image.dimensions(),
            Some(grid),
            [candidates.clone(), candidates].concat(),
            Some(140.0),
            None,
        );
        assert_eq!(two.status, OwnPositionStatus::Ambiguous);
        assert!(two.marker.is_none());
        let none = decide_own_position(5, image.dimensions(), None, Vec::new(), None, None);
        assert_eq!(none.status, OwnPositionStatus::Blocked);
    }

    #[test]
    fn two_consistent_fresh_frames_raise_confidence_only_with_compass_agreement() {
        let image = minimap_with_arrow(90.0);
        let grid = detect_map_grid(&image).unwrap();
        let candidates = own_arrow_candidates(&image, &grid, 1.0);
        let first = decide_own_position(
            1000,
            image.dimensions(),
            Some(grid),
            candidates.clone(),
            Some(92.0),
            Some(&rohkstov()),
        );
        let mut second = decide_own_position(
            2000,
            image.dimensions(),
            Some(grid),
            candidates.clone(),
            Some(88.0),
            Some(&rohkstov()),
        );
        stabilize_own_position(&mut second, Some(&first));
        assert_eq!(second.marker.as_ref().unwrap().confidence, 0.9);
        let mut late = decide_own_position(
            9000,
            image.dimensions(),
            Some(grid),
            candidates.clone(),
            Some(88.0),
            Some(&rohkstov()),
        );
        stabilize_own_position(&mut late, Some(&first));
        assert_eq!(late.marker.as_ref().unwrap().confidence, 0.6);
        let mut unconfirmed = decide_own_position(
            2000,
            image.dimensions(),
            Some(grid),
            candidates,
            None,
            Some(&rohkstov()),
        );
        stabilize_own_position(&mut unconfirmed, Some(&first));
        assert_eq!(unconfirmed.marker.as_ref().unwrap().confidence, 0.75);
    }

    #[test]
    fn detects_synthetic_grid_lines_within_a_pixel() {
        let grid = detect_map_grid(&synthetic(800, 600, 101, 57, 52)).unwrap();
        assert!((grid.left - 101.0).abs() < 1.0, "{grid:?}");
        assert!((grid.top - 57.0).abs() < 1.0);
        assert!((grid.right - 569.0).abs() < 1.0);
        assert!((grid.bottom - 525.0).abs() < 1.0);
    }

    #[test]
    fn refuses_frames_without_a_grid() {
        let plain = RgbaImage::from_fn(640, 480, |x, y| {
            let t = ((x * 31 + y * 17) % 29) as u8;
            image::Rgba([70 + t, 100 + t, 50, 255])
        });
        assert!(detect_map_grid(&plain).is_none());
        assert!(detect_map_grid(&RgbaImage::new(50, 50)).is_none());
    }

    #[test]
    fn sectors_use_columns_without_i_and_rows_from_the_top() {
        assert_eq!(grid_sector(Point { x: 0.0, y: 0.0 }).unwrap(), "A1");
        assert_eq!(grid_sector(Point { x: 0.25, y: 0.7 }).unwrap(), "C7");
        assert_eq!(grid_sector(Point { x: 0.999, y: 0.999 }).unwrap(), "J9");
        assert_eq!(grid_sector(Point { x: 1.0, y: 1.0 }).unwrap(), "J9");
        assert!(grid_sector(Point { x: -0.1, y: 0.5 }).is_none());
    }

    #[test]
    fn parses_noisy_grid_size_labels_but_never_a_partial_number() {
        assert_eq!(parse_grid_size("Grid size: 324 meters"), Some(324.0));
        assert_eq!(parse_grid_size("' • Grid sizeł 324 metŕrs"), Some(324.0));
        assert_eq!(parse_grid_size("Gțid size: 324 mețęrs"), Some(324.0));
        assert_eq!(parse_grid_size("Aid size: 32\"mețprs"), None);
        assert_eq!(parse_grid_size("F d size:"), None);
        assert_eq!(
            parse_grid_size("Grid size: 324 meters Grid size: 160 meters"),
            None
        );
    }

    fn catalog() -> Vec<MapCatalogEntry> {
        [
            ("zone_11", 2918.52),
            ("fulvia_gap", 2877.12),
            ("gensokyo", 1098.0),
            ("japan", 1098.0),
        ]
        .into_iter()
        .map(|(id, size)| MapCatalogEntry {
            id: id.into(),
            name: id.into(),
            size_game_meters: size,
            height_game_meters: None,
        })
        .collect()
    }

    #[test]
    fn identifies_maps_by_cell_size_and_keeps_ties_ambiguous() {
        let found = identify_map_by_cell_size(324.0, &catalog(), 1.5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "zone_11");
        assert_eq!(identify_map_by_cell_size(122.0, &catalog(), 1.5).len(), 2);
        assert!(identify_map_by_cell_size(200.0, &catalog(), 1.5).is_empty());
    }

    #[test]
    fn maps_pixels_to_world_and_back_including_non_square_maps() {
        let grid = MapGrid {
            left: 100.0,
            top: 50.0,
            right: 640.0,
            bottom: 590.0,
            cell_width: 60.0,
            cell_height: 60.0,
            columns: 9,
            rows: 9,
            line_strength: 1.0,
            frame_width: 800,
            frame_height: 600,
        };
        let p = map_grid_point(&grid, Some((900.0, 450.0)), Point { x: 370.0, y: 590.0 }).unwrap();
        assert_eq!(p.fraction, Point { x: 0.5, y: 1.0 });
        assert_eq!(p.world.unwrap(), Point { x: 450.0, y: 0.0 });
        assert_eq!(p.sector, "E9");
        let back = map_world_to_pixel(&grid, (900.0, 450.0), Point { x: 450.0, y: 450.0 }).unwrap();
        assert_eq!(back, Point { x: 370.0, y: 50.0 });
        assert!(map_grid_point(&grid, None, Point { x: 99.0, y: 60.0 }).is_none());
    }

    #[test]
    fn unreadable_label_or_missing_grid_never_names_a_map() {
        let image = synthetic(800, 600, 101, 57, 52);
        let unread = read_mtc_full_map_with_ocr(&image, 5, &catalog(), |_| Ok("Grid".into()));
        assert_eq!(unread.status, MtcMapStatus::Unidentified);
        assert!(unread.map_id.is_none() && unread.grid.is_some());
        let once = std::cell::Cell::new(0);
        let single = read_mtc_full_map_with_ocr(&image, 5, &catalog(), |_| {
            once.set(once.get() + 1);
            Ok(if once.get() == 1 {
                "Grid size: 324 meters"
            } else {
                ""
            }
            .into())
        });
        assert!(single.map_id.is_none(), "one OCR attempt is not enough");
        let ready = read_mtc_full_map_with_ocr(&image, 5, &catalog(), |_| {
            Ok("Grid size: 324 meters".into())
        });
        assert_eq!(ready.status, MtcMapStatus::Ready);
        assert_eq!(ready.map_id.as_deref(), Some("zone_11"));
        let calibration = ready.calibration.unwrap();
        let sw = calibration.transform(Point { x: 101.0, y: 525.0 }).unwrap();
        assert!(sw.x.abs() < 10.0 && sw.y.abs() < 10.0, "{sw:?}");
        let tie = read_mtc_full_map_with_ocr(&image, 5, &catalog(), |_| {
            Ok("Grid size: 122 meters".into())
        });
        assert_eq!(tie.status, MtcMapStatus::Ambiguous);
        assert_eq!(tie.candidates.len(), 2);
        let none = read_mtc_full_map_with_ocr(&RgbaImage::new(400, 300), 5, &catalog(), |_| {
            panic!("no OCR without a grid")
        });
        assert_eq!(none.status, MtcMapStatus::Blocked);
    }
}
