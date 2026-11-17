use crate::{MapCalibration, Point};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapReferenceMetadata {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub unit: String,
    pub source: String,
    pub confirmed: bool,
    pub calibration: MapCalibration,
}
impl MapReferenceMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.id.trim().is_empty()
            || self.id.len() > 256
            || self.name.trim().is_empty()
            || self.source.trim().is_empty()
            || self.source.len() > 2048
            || !["game_units", "game_meters"].contains(&self.unit.as_str())
        {
            return Err("Niepoprawne metadane referencji mapy".into());
        }
        if !self.confirmed {
            return Err("Potwierdź pochodzenie, geometrię i kalibrację referencji mapy".into());
        }
        self.calibration.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapDescriptor {
    pub schema_version: u32,
    pub map_id: String,
    pub width: u32,
    pub height: u32,
    pub grid_size: u32,
    pub gray_samples: Vec<u8>,
    pub texture_rms: f64,
}

/// Clones share the reduced grayscale raster and the precomputed reference
/// features; no full gameplay frame is persisted.
#[derive(Clone)]
pub struct MapReference {
    pub metadata: MapReferenceMetadata,
    pub descriptor: MapDescriptor,
    gray: Arc<GrayRaster>,
    index: Arc<ReferenceIndex>,
}
impl MapReference {
    pub fn validate(&self) -> Result<(), String> {
        self.metadata.validate()?;
        if self.descriptor.schema_version != 1
            || self.descriptor.map_id != self.metadata.id
            || self.descriptor.width != self.gray.original_width
            || self.descriptor.height != self.gray.original_height
            || self.descriptor.grid_size != 16
            || self.descriptor.gray_samples.len() != 256
            || !self.descriptor.texture_rms.is_finite()
            || self.descriptor.texture_rms < 8.0
        {
            return Err("Deskryptor i geometria nie zgadzają się z rastrem referencji mapy".into());
        }
        Ok(())
    }
    /// Builds the reference once: the multi-scale feature index is computed here,
    /// not on every `match_map_view` call.
    pub fn new(metadata: MapReferenceMetadata, image: &RgbaImage) -> Result<Self, String> {
        metadata.validate()?;
        validate_image(image)?;
        let gray = GrayRaster::from_rgba(image, 512);
        let texture_rms = gray.rms();
        if texture_rms < 8.0 {
            return Err("Referencja mapy nie zawiera dostatecznej tekstury".into());
        }
        let mut gray_samples = Vec::with_capacity(256);
        for y in 0..16 {
            for x in 0..16 {
                gray_samples.push(
                    gray.sample(
                        (x as f64 + 0.5) * gray.width as f64 / 16.0,
                        (y as f64 + 0.5) * gray.height as f64 / 16.0,
                    )
                    .round() as u8,
                );
            }
        }
        let descriptor = MapDescriptor {
            schema_version: 1,
            map_id: metadata.id.clone(),
            width: image.width(),
            height: image.height(),
            grid_size: 16,
            gray_samples,
            texture_rms,
        };
        let index = ReferenceIndex::build(image, &gray);
        Ok(Self {
            metadata,
            descriptor,
            gray: Arc::new(gray),
            index: Arc::new(index),
        })
    }
    /// Number of cached multi-scale reference features (diagnostics).
    pub fn feature_count(&self) -> usize {
        self.index.features.len()
    }
}

#[derive(Default)]
pub struct MapReferenceRegistry {
    references: Vec<MapReference>,
}
impl MapReferenceRegistry {
    pub fn register(&mut self, reference: MapReference) -> Result<(), String> {
        reference.validate()?;
        if let Some(existing) = self
            .references
            .iter_mut()
            .find(|r| r.metadata.id == reference.metadata.id)
        {
            *existing = reference;
            return Ok(());
        }
        if self.references.len() >= 32 {
            return Err("Katalog referencji przekracza limit 32 map".into());
        }
        self.references.push(reference);
        Ok(())
    }
    pub fn references(&self) -> &[MapReference] {
        &self.references
    }
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.references.len();
        self.references.retain(|r| r.metadata.id != id);
        before != self.references.len()
    }
    pub fn descriptors(&self) -> Vec<MapDescriptor> {
        self.references
            .iter()
            .map(|r| r.descriptor.clone())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarityTransform {
    pub scale: f64,
    pub rotation_deg: f64,
    pub offset_x: f64,
    pub offset_y: f64,
}
impl SimilarityTransform {
    pub fn validate(&self) -> Result<(), String> {
        if !self.scale.is_finite()
            || self.scale <= 0.0
            || !self.rotation_deg.is_finite()
            || !self.offset_x.is_finite()
            || !self.offset_y.is_finite()
        {
            return Err("Niepoprawna transformacja mapy".into());
        }
        Ok(())
    }
    /// Query/client pixel -> pixel of the registered reference map.
    pub fn map_point(&self, p: Point) -> Result<Point, String> {
        self.validate()?;
        if !p.valid() {
            return Err("Niepoprawny punkt obrazu".into());
        }
        let a = self.rotation_deg.to_radians();
        let result = Point {
            x: self.offset_x + self.scale * (a.cos() * p.x - a.sin() * p.y),
            y: self.offset_y + self.scale * (a.sin() * p.x + a.cos() * p.y),
        };
        if !result.valid() {
            return Err("Transformacja mapy przekracza zakres liczb".into());
        }
        Ok(result)
    }
    pub fn inverse_point(&self, p: Point) -> Result<Point, String> {
        self.validate()?;
        if !p.valid() {
            return Err("Niepoprawny punkt mapy".into());
        }
        let a = self.rotation_deg.to_radians();
        let x = (p.x - self.offset_x) / self.scale;
        let y = (p.y - self.offset_y) / self.scale;
        let result = Point {
            x: a.cos() * x + a.sin() * y,
            y: -a.sin() * x + a.cos() * y,
        };
        if !result.valid() {
            return Err("Odwrotna transformacja przekracza zakres liczb".into());
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl PixelRect {
    pub fn intersect(&self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (x.is_finite() && y.is_finite() && right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MapMatchConfig {
    /// Longest axis of the working query raster.
    pub max_image_dimension: u32,
    /// Query corner limit per pyramid level.
    pub max_features_per_level: usize,
    /// Query pyramid, relative to the working raster. The reference pyramid is
    /// fixed, dense and cached at registration.
    pub pyramid_scales: Vec<f64>,
    pub rotations_deg: Vec<f64>,
    pub min_scale: f64,
    pub max_scale: f64,
    pub max_descriptor_hamming: u32,
    pub descriptor_ratio: f64,
    /// Lower bound of the inlier tolerance in reference pixels; coarse pyramid
    /// levels widen it to two pixels of the matched reference level.
    pub max_reprojection_error: f64,
    pub min_inliers: usize,
    pub min_inlier_ratio: f64,
    pub ambiguity_margin: f64,
    pub max_candidates: usize,
    pub max_hypotheses: usize,
    pub max_processing_ms: u64,
    /// Correlation of the aligned query and reference intensities required at
    /// moderate support (2–3× `min_inliers`); +0.25 at the minimal consensus,
    /// −0.15 at ≥9× `min_inliers`.
    pub min_verification_correlation: f64,
}
impl Default for MapMatchConfig {
    fn default() -> Self {
        Self {
            max_image_dimension: 1024,
            max_features_per_level: 512,
            pyramid_scales: vec![1.0, 0.7, 0.5],
            // The MTC map view and the minimap are north-up.
            rotations_deg: vec![0.0],
            min_scale: 0.1,
            max_scale: 10.0,
            max_descriptor_hamming: 34,
            descriptor_ratio: 0.9,
            max_reprojection_error: 4.0,
            min_inliers: 6,
            min_inlier_ratio: 0.1,
            ambiguity_margin: 0.1,
            max_candidates: 5,
            max_hypotheses: 512,
            max_processing_ms: 500,
            min_verification_correlation: 0.4,
        }
    }
}
impl MapMatchConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(64..=1024).contains(&self.max_image_dimension)
            || !(16..=1024).contains(&self.max_features_per_level)
            || self.pyramid_scales.is_empty()
            || self.pyramid_scales.len() > 6
            || self
                .pyramid_scales
                .iter()
                .any(|s| !s.is_finite() || *s < 0.2 || *s > 1.0)
            || self.rotations_deg.is_empty()
            || self.rotations_deg.len() > 8
            || self
                .rotations_deg
                .iter()
                .any(|a| !a.is_finite() || a.abs() > 360.0)
            || !self.min_scale.is_finite()
            || !self.max_scale.is_finite()
            || self.min_scale <= 0.0
            || self.max_scale < self.min_scale
            || self.max_scale > 100.0
            || !(1..=40).contains(&self.max_descriptor_hamming)
            || !self.descriptor_ratio.is_finite()
            || !(0.1..0.95).contains(&self.descriptor_ratio)
            || !self.max_reprojection_error.is_finite()
            || self.max_reprojection_error <= 0.0
            || self.max_reprojection_error > 32.0
            || !(4..=128).contains(&self.min_inliers)
            || !self.min_inlier_ratio.is_finite()
            || !(0.05..=1.0).contains(&self.min_inlier_ratio)
            || !self.ambiguity_margin.is_finite()
            || !(0.01..=0.5).contains(&self.ambiguity_margin)
            || !(1..=16).contains(&self.max_candidates)
            || !(16..=4096).contains(&self.max_hypotheses)
            || !(1..=1500).contains(&self.max_processing_ms)
            || !self.min_verification_correlation.is_finite()
            || !(0.1..=0.95).contains(&self.min_verification_correlation)
        {
            return Err("Niepoprawne lub nieograniczone parametry dopasowania mapy".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapViewStatus {
    Matched,
    Ambiguous,
    NoMatch,
    MissingReferences,
    TimedOut,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapViewCandidate {
    pub map_id: String,
    pub score: f64,
    pub inliers: usize,
    pub tentative_matches: usize,
    pub inlier_ratio: f64,
    pub transform: SimilarityTransform,
    pub projected_map_bounds: PixelRect,
    pub visible_reference_bounds: PixelRect,
    pub coverage_ratio: f64,
    pub reference_width: u32,
    pub reference_height: u32,
    pub query_width: u32,
    pub query_height: u32,
    pub unit: String,
    pub calibration: MapCalibration,
    /// Pearson correlation of aligned query/reference intensities (−1..1).
    #[serde(default)]
    pub verification_correlation: f64,
}
impl MapViewCandidate {
    pub fn reference_point(&self, point: Point) -> Result<Point, String> {
        if !point.valid()
            || point.x < 0.0
            || point.y < 0.0
            || point.x > self.query_width as f64 - 1.0
            || point.y > self.query_height as f64 - 1.0
        {
            return Err("Punkt poza źródłową klatką".into());
        }
        let mapped = self.transform.map_point(point)?;
        if mapped.x < 0.0
            || mapped.y < 0.0
            || mapped.x > self.reference_width as f64 - 1.0
            || mapped.y > self.reference_height as f64 - 1.0
        {
            return Err("Punkt poza rozpoznanym obszarem referencji mapy".into());
        }
        Ok(mapped)
    }
    pub fn calibrated_world_point(&self, point: Point) -> Result<Point, String> {
        self.calibration.transform(self.reference_point(point)?)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapViewMatch {
    pub status: MapViewStatus,
    pub best: Option<MapViewCandidate>,
    pub candidates: Vec<MapViewCandidate>,
    pub warning: Option<String>,
    pub elapsed_ms: u64,
}

/// Matches only user-provided map rasters, including a map rectangle inside a larger HUD frame.
pub fn match_map_view(
    query: &RgbaImage,
    references: &[MapReference],
    config: &MapMatchConfig,
) -> Result<MapViewMatch, String> {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(config.max_processing_ms);
    match_map_view_with_clock(query, references, config, Clock(Some(deadline)), started)
}

/// Cooperative work budget. `None` disables the wall clock: geometry tests must
/// not depend on scheduler load; the public entry point always passes a deadline.
#[derive(Clone, Copy)]
struct Clock(Option<Instant>);
impl Clock {
    fn expired(&self) -> bool {
        self.0.is_some_and(|deadline| Instant::now() >= deadline)
    }
}

fn match_map_view_with_clock(
    query: &RgbaImage,
    references: &[MapReference],
    config: &MapMatchConfig,
    clock: Clock,
    started: Instant,
) -> Result<MapViewMatch, String> {
    config.validate()?;
    validate_image(query)?;
    if references.len() > 32 {
        return Err("Zbyt wiele referencji mapy".into());
    }
    if references.is_empty() {
        return Ok(result(
            MapViewStatus::MissingReferences,
            vec![],
            started,
            Some("Brak lokalnych rastrów referencyjnych map".into()),
        ));
    }
    let mut ids = BTreeSet::new();
    for reference in references {
        reference.validate()?;
        if !ids.insert(&reference.metadata.id) {
            return Err("Powtórzone ID referencji map".into());
        }
    }
    let timed_out = |candidates| {
        Ok(result(
            MapViewStatus::TimedOut,
            candidates,
            started,
            Some("Nie porównano całego katalogu w budżecie czasu; wynik zablokowany".into()),
        ))
    };
    let query_gray = GrayRaster::from_rgba(query, config.max_image_dimension);
    if query_gray.rms() < 8.0 {
        return Ok(result(
            MapViewStatus::NoMatch,
            vec![],
            started,
            Some("Obraz nie ma dostatecznej tekstury".into()),
        ));
    }
    let Some(features) = extract_query_features(&query_gray, config, clock) else {
        return timed_out(vec![]);
    };
    let query_levels = query_gray.halving_pyramid();
    // References are independent; results are collected in catalog order, so
    // the outcome does not depend on thread scheduling.
    let fits = map_parallel(references, |reference| {
        if clock.expired() {
            return Fit::TimedOut;
        }
        let Some(pairs) = match_features(&features, &reference.index.features, config, clock)
        else {
            return Fit::TimedOut;
        };
        fit_reference(
            &pairs,
            reference,
            &query_levels,
            query.width(),
            query.height(),
            config,
            clock,
        )
    });
    let mut candidates = Vec::new();
    let mut incomplete = false;
    for fit in fits {
        match fit {
            Fit::Candidate(candidate) => candidates.push(*candidate),
            Fit::Rejected => {}
            Fit::TimedOut => incomplete = true,
        }
    }
    if incomplete || clock.expired() {
        return timed_out(candidates);
    }
    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.map_id.cmp(&b.map_id))
    });
    let status = if candidates.is_empty() {
        MapViewStatus::NoMatch
    } else if candidates.len() > 1
        && candidates[0].score - candidates[1].score < config.ambiguity_margin
    {
        MapViewStatus::Ambiguous
    } else {
        MapViewStatus::Matched
    };
    candidates.truncate(config.max_candidates);
    let warning = match status {
        MapViewStatus::NoMatch => {
            Some("Brak wiarygodnej zgodności z lokalnymi referencjami".into())
        }
        MapViewStatus::Ambiguous => {
            Some("Dwie mapy pasują podobnie; wybierz referencję ręcznie".into())
        }
        _ => None,
    };
    Ok(result(status, candidates, started, warning))
}
/// Order-preserving parallel map over a short slice (catalog, pyramid, row bands).
fn map_parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(items.len());
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut slots: Vec<Option<R>> = items.iter().map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        done.push((index, f(item)));
                    }
                    done
                })
            })
            .collect();
        for worker in workers {
            match worker.join() {
                Ok(done) => {
                    for (index, value) in done {
                        slots[index] = Some(value);
                    }
                }
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    slots
        .into_iter()
        .map(|slot| slot.expect("every index is processed exactly once"))
        .collect()
}
fn result(
    status: MapViewStatus,
    candidates: Vec<MapViewCandidate>,
    started: Instant,
    warning: Option<String>,
) -> MapViewMatch {
    MapViewMatch {
        status,
        best: (status == MapViewStatus::Matched).then(|| candidates[0].clone()),
        candidates,
        warning,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}
fn validate_image(image: &RgbaImage) -> Result<(), String> {
    if image.width() < 32
        || image.height() < 32
        || image.width() as u64 * image.height() as u64 > 16_777_216
    {
        return Err("Nieobsługiwany rozmiar obrazu referencji (32px do 16M pikseli)".into());
    }
    Ok(())
}

/// Grayscale raster. A level pixel `x` covers original pixels
/// `[x·r, (x+1)·r)`, so its centre is the original coordinate `(x+0.5)·r−0.5`.
#[derive(Clone)]
struct GrayRaster {
    width: u32,
    height: u32,
    /// Luma premultiplied by alpha (transparent texture margins read as black).
    data: Vec<f32>,
    /// Coverage 0–1; empty when the source is fully opaque.
    alpha: Vec<f32>,
    original_width: u32,
    original_height: u32,
}
/// Output pixel → covered source range `[start, end)` (area footprint).
fn footprints(source: u32, target: u32) -> Vec<(usize, usize)> {
    let ratio = source as f64 / target as f64;
    (0..target)
        .map(|i| {
            let start = ((i as f64 * ratio).floor() as u32).min(source - 1);
            let end = (((i + 1) as f64 * ratio).floor() as u32).clamp(start + 1, source);
            (start as usize, end as usize)
        })
        .collect()
}
/// Averages one source row into the output columns.
fn reduce_row(row: &[f32], xs: &[(usize, usize)], out: &mut Vec<f32>) {
    for &(x0, x1) in xs {
        let sum: f32 = row[x0..x1].iter().sum();
        out.push(sum / (x1 - x0) as f32);
    }
}
/// Averages horizontally reduced rows into output rows.
fn reduce_columns(rows: &[f32], width: usize, ys: &[(usize, usize)]) -> Vec<f32> {
    let mut out = vec![0.0f32; width * ys.len()];
    for (target, &(y0, y1)) in out.chunks_exact_mut(width).zip(ys) {
        for row in rows[y0 * width..y1 * width].chunks_exact(width) {
            for (o, v) in target.iter_mut().zip(row) {
                *o += *v;
            }
        }
        let n = (y1 - y0) as f32;
        for o in target.iter_mut() {
            *o /= n;
        }
    }
    out
}
impl GrayRaster {
    /// Exact separable area average, so thin grid lines and JPEG noise do not
    /// alias into unstable corners. Linear in the input area (≤16M pixels).
    fn from_rgba(image: &RgbaImage, max_dimension: u32) -> Self {
        let (iw, ih) = image.dimensions();
        let ratio = (max_dimension as f64 / iw.max(ih) as f64).min(1.0);
        let width = ((iw as f64 * ratio).round() as u32).clamp(1, iw);
        let height = ((ih as f64 * ratio).round() as u32).clamp(1, ih);
        let xs = footprints(iw, width);
        let ys = footprints(ih, height);
        // Horizontal pass in independent row bands; bands are concatenated in order.
        let stride = iw as usize * 4;
        let bands: Vec<&[u8]> = image.as_raw().chunks(stride * 64).collect();
        let reduced = map_parallel(&bands, |band| {
            let mut luma_rows = Vec::with_capacity(width as usize * band.len() / stride);
            let mut alpha_rows = Vec::with_capacity(width as usize * band.len() / stride);
            let mut luma = vec![0.0f32; iw as usize];
            let mut cover = vec![0.0f32; iw as usize];
            let mut transparent = false;
            for row in band.chunks_exact(stride) {
                for ((p, l), c) in row.as_chunks::<4>().0.iter().zip(&mut luma).zip(&mut cover) {
                    let a = p[3] as f32 * (1.0 / 255.0);
                    *l = (p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114) * a;
                    *c = a;
                    transparent |= p[3] != 255;
                }
                reduce_row(&luma, &xs, &mut luma_rows);
                reduce_row(&cover, &xs, &mut alpha_rows);
            }
            (luma_rows, alpha_rows, transparent)
        });
        let mut luma_rows = Vec::with_capacity(width as usize * ih as usize);
        let mut alpha_rows = Vec::with_capacity(width as usize * ih as usize);
        let mut transparent = false;
        for (luma, alpha, band_transparent) in reduced {
            luma_rows.extend(luma);
            alpha_rows.extend(alpha);
            transparent |= band_transparent;
        }
        Self {
            width,
            height,
            data: reduce_columns(&luma_rows, width as usize, &ys),
            alpha: if transparent {
                reduce_columns(&alpha_rows, width as usize, &ys)
            } else {
                Vec::new()
            },
            original_width: iw,
            original_height: ih,
        }
    }
    /// Separable box (area) reduction to an explicit size not larger than the source.
    fn resized(&self, width: u32, height: u32) -> Self {
        let width = width.clamp(1, self.width);
        let height = height.clamp(1, self.height);
        let xs = footprints(self.width, width);
        let ys = footprints(self.height, height);
        let reduce = |values: &[f32]| {
            let mut rows = Vec::with_capacity(width as usize * self.height as usize);
            for row in values.chunks_exact(self.width as usize) {
                reduce_row(row, &xs, &mut rows);
            }
            reduce_columns(&rows, width as usize, &ys)
        };
        Self {
            width,
            height,
            data: reduce(&self.data),
            alpha: if self.alpha.is_empty() {
                Vec::new()
            } else {
                reduce(&self.alpha)
            },
            original_width: self.original_width,
            original_height: self.original_height,
        }
    }
    fn scaled(&self, scale: f64) -> Self {
        self.resized(
            (self.width as f64 * scale).round().max(1.0) as u32,
            (self.height as f64 * scale).round().max(1.0) as u32,
        )
    }
    /// Binomial [1 4 6 4 1]/16 smoothing (σ≈1) used before corner detection and BRIEF tests.
    fn smoothed(&self) -> Self {
        let (w, h) = (self.width as usize, self.height as usize);
        const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let clamped = |i: usize, k: usize, n: usize| (i + k).saturating_sub(2).min(n - 1);
        let mut tmp = vec![0.0f32; w * h];
        for (src, dst) in self.data.chunks_exact(w).zip(tmp.chunks_exact_mut(w)) {
            for (x, d) in dst.iter_mut().enumerate() {
                *d = if x >= 2 && x + 2 < w {
                    (src[x - 2] + src[x + 2]) * K[0]
                        + (src[x - 1] + src[x + 1]) * K[1]
                        + src[x] * K[2]
                } else {
                    (0..5).map(|k| src[clamped(x, k, w)] * K[k]).sum()
                };
            }
        }
        let mut data = vec![0.0f32; w * h];
        for (y, dst) in data.chunks_exact_mut(w).enumerate() {
            let row = |k: usize| &tmp[clamped(y, k, h) * w..clamped(y, k, h) * w + w];
            let (r0, r1, r2, r3, r4) = (row(0), row(1), row(2), row(3), row(4));
            for (x, d) in dst.iter_mut().enumerate() {
                *d = (r0[x] + r4[x]) * K[0] + (r1[x] + r3[x]) * K[1] + r2[x] * K[2];
            }
        }
        Self {
            data,
            alpha: self.alpha.clone(),
            ..*self
        }
    }
    /// Base raster and its successive halvings down to 32 px (verification sampling).
    fn halving_pyramid(&self) -> Vec<GrayRaster> {
        let mut levels = vec![self.clone()];
        while let Some(last) = levels.last() {
            if last.width.min(last.height) < 64 || levels.len() >= 6 {
                break;
            }
            let next = last.resized(last.width / 2, last.height / 2);
            levels.push(next);
        }
        levels
    }
    /// Original pixels per level pixel along x (equal to y up to rounding).
    fn unit(&self) -> f64 {
        self.original_width as f64 / self.width as f64
    }
    fn original_point(&self, x: f64, y: f64) -> Point {
        Point {
            x: (x + 0.5) * self.original_width as f64 / self.width as f64 - 0.5,
            y: (y + 0.5) * self.original_height as f64 / self.height as f64 - 0.5,
        }
    }
    fn level_point(&self, p: Point) -> Point {
        Point {
            x: (p.x + 0.5) * self.width as f64 / self.original_width as f64 - 0.5,
            y: (p.y + 0.5) * self.height as f64 / self.original_height as f64 - 0.5,
        }
    }
    fn sample(&self, x: f64, y: f64) -> f32 {
        let x = x.clamp(0.0, self.width as f64 - 1.0);
        let y = y.clamp(0.0, self.height as f64 - 1.0);
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let dx = (x - x0 as f64) as f32;
        let dy = (y - y0 as f64) as f32;
        self.data[y0 as usize * self.width as usize + x0 as usize] * (1.0 - dx) * (1.0 - dy)
            + self.data[y0 as usize * self.width as usize + x1 as usize] * dx * (1.0 - dy)
            + self.data[y1 as usize * self.width as usize + x0 as usize] * (1.0 - dx) * dy
            + self.data[y1 as usize * self.width as usize + x1 as usize] * dx * dy
    }
    /// Nearest-pixel coverage; outside the raster counts as transparent.
    fn alpha_at(&self, x: f64, y: f64) -> f32 {
        if !(x > -0.5 && y > -0.5 && x < self.width as f64 - 0.5 && y < self.height as f64 - 0.5) {
            return 0.0;
        }
        if self.alpha.is_empty() {
            return 1.0;
        }
        self.alpha[y.round() as usize * self.width as usize + x.round() as usize]
    }
    fn rms(&self) -> f64 {
        let mean = self.data.iter().map(|v| *v as f64).sum::<f64>() / self.data.len() as f64;
        (self
            .data
            .iter()
            .map(|v| (*v as f64 - mean).powi(2))
            .sum::<f64>()
            / self.data.len() as f64)
            .sqrt()
    }
}

/// Descriptor support: radius-23 BRIEF pattern on the smoothed level.
const BRIEF_BITS: usize = 128;
/// Corners closer to the level border than this are not described.
const BORDER: usize = 12;
/// Minimum Shi–Tomasi response on the smoothed level (contrast ≈ 6 gray levels).
const MIN_CORNER_SCORE: f32 = 12.0;
/// Reference pyramid: up to 1024 px, ×0.8 per level, down to ~96 px.
const REFERENCE_MAX_DIMENSION: u32 = 1024;
const REFERENCE_LEVEL_STEP: f64 = 0.8;
const REFERENCE_MIN_DIMENSION: f64 = 96.0;
/// |ln(scale / descriptor scale hint)|: a matched level pair implies the scale.
const SCALE_TOLERANCE: f64 = 0.3;
const ROTATION_TOLERANCE_DEG: f64 = 15.0;
/// Verification grid per axis over the visible map.
const VERIFICATION_GRID: usize = 40;
const MIN_VERIFICATION_SAMPLES: usize = 150;
/// Verification tiles per axis (local correlation).
const VERIFICATION_TILES: usize = 4;

fn brief_pattern() -> &'static [[f32; 4]; BRIEF_BITS] {
    static PATTERN: OnceLock<[[f32; 4]; BRIEF_BITS]> = OnceLock::new();
    PATTERN.get_or_init(|| {
        let mut seed = 0x9e3779b9u32;
        // A large support distinguishes neighbourhoods of big, flat tiles that share
        // the ordering of four colours in a small window.
        let mut coordinate = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            ((seed >> 16) % 47) as f32 - 23.0
        };
        let mut pattern = [[0.0; 4]; BRIEF_BITS];
        for test in pattern.iter_mut() {
            *test = [coordinate(), coordinate(), coordinate(), coordinate()];
        }
        pattern
    })
}

#[derive(Clone)]
struct Feature {
    /// Original-image coordinates.
    point: Point,
    bits: [u64; 2],
    /// Original pixels per level pixel of the level that produced the feature.
    unit: f64,
    /// Descriptor sampling rotation (query only; reference features are upright).
    rotation_deg: f64,
}

struct ReferenceIndex {
    features: Vec<Feature>,
    /// Halvings of the 512 px raster (index 0 = half), for verification.
    verification: Vec<GrayRaster>,
    /// Full-length thin lines (the community textures' 9×9 grid), original px.
    /// Identical in every texture, so they are no evidence for one map.
    line_columns: Vec<f64>,
    line_rows: Vec<f64>,
}
impl ReferenceIndex {
    fn build(image: &RgbaImage, gray: &GrayRaster) -> Self {
        let base = GrayRaster::from_rgba(image, REFERENCE_MAX_DIMENSION);
        let mut scales = Vec::new();
        let mut scale = 1.0f64;
        while (base.width.max(base.height) as f64 * scale) >= REFERENCE_MIN_DIMENSION
            && (base.width.min(base.height) as f64 * scale) >= 32.0
        {
            scales.push(scale);
            scale *= REFERENCE_LEVEL_STEP;
        }
        let levels = map_parallel(&scales, |scale| {
            let level = if *scale == 1.0 {
                base.clone()
            } else {
                base.scaled(*scale)
            };
            let area = level.width as f64 * level.height as f64;
            let limit = (area / 1600.0).clamp(48.0, 400.0) as usize;
            let smooth = level.smoothed();
            let mut features = Vec::new();
            for (x, y) in detect_corners(&smooth, limit, Clock(None)).unwrap_or_default() {
                if !level.alpha.is_empty() && transparent_support(&level, x, y) {
                    continue;
                }
                features.push(Feature {
                    point: smooth.original_point(x as f64, y as f64),
                    bits: brief(&smooth, x as f64, y as f64, 0.0),
                    unit: smooth.unit(),
                    rotation_deg: 0.0,
                });
            }
            features
        });
        let mut verification = gray.halving_pyramid();
        verification.remove(0);
        Self {
            features: levels.into_iter().flatten().collect(),
            verification,
            line_columns: full_length_lines(gray, true),
            line_rows: full_length_lines(gray, false),
        }
    }
    fn near_line(&self, p: Point, tolerance: f64) -> bool {
        self.line_columns
            .iter()
            .any(|x| (p.x - x).abs() <= tolerance)
            || self.line_rows.iter().any(|y| (p.y - y).abs() <= tolerance)
    }
}
/// Columns (or rows) where a thin line differs from both neighbours along most
/// of the raster length. Step edges (map/margin borders) are not lines.
fn full_length_lines(gray: &GrayRaster, vertical: bool) -> Vec<f64> {
    let (w, h) = (gray.width as usize, gray.height as usize);
    let (count, length) = if vertical { (w, h) } else { (h, w) };
    let at = |i: usize, j: usize| {
        if vertical {
            gray.data[j * w + i]
        } else {
            gray.data[i * w + j]
        }
    };
    // Fraction of the length where the line is brighter (or darker) than both
    // neighbours three pixels away.
    let fraction: Vec<f64> = (0..count)
        .map(|i| {
            if i < 3 || i + 3 >= count {
                return 0.0;
            }
            let (mut bright, mut dark) = (0usize, 0usize);
            for j in 0..length {
                let (c, l, r) = (at(i, j), at(i - 3, j), at(i + 3, j));
                bright += usize::from(c - l > 10.0 && c - r > 10.0);
                dark += usize::from(l - c > 10.0 && r - c > 10.0);
            }
            bright.max(dark) as f64 / length as f64
        })
        .collect();
    let mut lines = Vec::new();
    for i in 3..count.saturating_sub(3) {
        let peak = (i - 2..=i + 2)
            .all(|k| k == i || fraction[i] > fraction[k] || (k > i && fraction[i] >= fraction[k]));
        if fraction[i] > 0.6 && peak {
            let p = gray.original_point(i as f64, i as f64);
            lines.push(if vertical { p.x } else { p.y });
        }
    }
    // Only a sparse overlay grid is masked; dense matches mean texture, not grid.
    if lines.len() > 32 {
        lines.clear();
    }
    lines
}
/// Texture margins are transparent in community rasters; a descriptor whose
/// support crosses them would encode the margin colour, not the map.
fn transparent_support(level: &GrayRaster, x: u32, y: u32) -> bool {
    let mut missing = 0;
    if level.alpha_at(x as f64, y as f64) < 0.5 {
        return true;
    }
    for [ax, ay, bx, by] in brief_pattern() {
        for (dx, dy) in [(ax, ay), (bx, by)] {
            if level.alpha_at(x as f64 + *dx as f64, y as f64 + *dy as f64) < 0.5 {
                missing += 1;
            }
        }
    }
    missing > 2 * BRIEF_BITS / 16
}

fn extract_query_features(
    base: &GrayRaster,
    config: &MapMatchConfig,
    clock: Clock,
) -> Option<Vec<Feature>> {
    let levels = map_parallel(&config.pyramid_scales, |scale| {
        if clock.expired() {
            return None;
        }
        let level = if *scale == 1.0 {
            base.smoothed()
        } else {
            base.scaled(*scale).smoothed()
        };
        let mut result = Vec::new();
        if level.width < 32 || level.height < 32 {
            return Some(result);
        }
        let corners = detect_corners(&level, config.max_features_per_level, clock)?;
        for (x, y) in corners {
            for angle in &config.rotations_deg {
                result.push(Feature {
                    point: level.original_point(x as f64, y as f64),
                    bits: brief(&level, x as f64, y as f64, *angle),
                    unit: level.unit(),
                    rotation_deg: *angle,
                });
            }
        }
        Some(result)
    });
    let mut result = Vec::new();
    for level in levels {
        result.extend(level?);
    }
    Some(result)
}

/// Shi–Tomasi corners with 3×3 non-maximum suppression and a spatially spread
/// selection: one corner per bucket first, so a stronger HUD cannot take the
/// whole budget from a small map, then the strongest remaining corners.
fn detect_corners(level: &GrayRaster, limit: usize, clock: Clock) -> Option<Vec<(u32, u32)>> {
    let (w, h) = (level.width as usize, level.height as usize);
    if w < 2 * BORDER + 3 || h < 2 * BORDER + 3 {
        return Some(vec![]);
    }
    let d = &level.data;
    let mut gxx = vec![0.0f32; w * h];
    let mut gyy = vec![0.0f32; w * h];
    let mut gxy = vec![0.0f32; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let gx = d[i + 1] - d[i - 1];
            let gy = d[i + w] - d[i - w];
            gxx[i] = gx * gx;
            gyy[i] = gy * gy;
            gxy[i] = gx * gy;
        }
    }
    if clock.expired() {
        return None;
    }
    let box3 = |values: &mut Vec<f32>| {
        let mut tmp = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 1..w - 1 {
                let i = y * w + x;
                tmp[i] = values[i - 1] + values[i] + values[i + 1];
            }
        }
        for y in 1..h - 1 {
            for x in 0..w {
                let i = y * w + x;
                values[i] = tmp[i - w] + tmp[i] + tmp[i + w];
            }
        }
    };
    box3(&mut gxx);
    box3(&mut gyy);
    box3(&mut gxy);
    let mut score = vec![0.0f32; w * h];
    for i in 0..w * h {
        let (a, b, c) = (gxx[i], gyy[i], gxy[i]);
        score[i] = (a + b - ((a - b) * (a - b) + 4.0 * c * c).sqrt()) * 0.5;
    }
    let mut candidates = Vec::new();
    for y in BORDER..h - BORDER {
        for x in BORDER..w - BORDER {
            let i = y * w + x;
            let s = score[i];
            if s < MIN_CORNER_SCORE {
                continue;
            }
            // Strict on preceding neighbours, non-strict on following: one peak per plateau.
            if s <= score[i - w - 1]
                || s <= score[i - w]
                || s <= score[i - w + 1]
                || s <= score[i - 1]
                || s < score[i + 1]
                || s < score[i + w - 1]
                || s < score[i + w]
                || s < score[i + w + 1]
            {
                continue;
            }
            candidates.push((s, x as u32, y as u32));
        }
    }
    if clock.expired() {
        return None;
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
    let buckets = (limit / 2).max(1) as f64;
    let columns = ((buckets * w as f64 / h as f64).sqrt().round() as usize).clamp(1, 64);
    let rows = ((buckets / columns as f64).round() as usize).clamp(1, 64);
    let mut occupied = vec![false; columns * rows];
    // Occupancy grid with 3 px cells: at most one chosen corner per cell, and
    // a 5 px minimum separation is checked against the 5×5 cell neighbourhood.
    const CELL: usize = 3;
    let (cw, ch) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut taken: Vec<Option<(u32, u32)>> = vec![None; cw * ch];
    let mut chosen = Vec::new();
    for spatial_pass in [true, false] {
        for &(_, x, y) in &candidates {
            if chosen.len() >= limit {
                break;
            }
            let bucket = (y as usize * rows / h) * columns + x as usize * columns / w;
            if spatial_pass && occupied[bucket] {
                continue;
            }
            let (cx, cy) = (x as usize / CELL, y as usize / CELL);
            let mut near = false;
            'scan: for ny in cy.saturating_sub(2)..(cy + 3).min(ch) {
                for nx in cx.saturating_sub(2)..(cx + 3).min(cw) {
                    if let Some((px, py)) = taken[ny * cw + nx] {
                        if (px as f64 - x as f64).hypot(py as f64 - y as f64) < 5.0 {
                            near = true;
                            break 'scan;
                        }
                    }
                }
            }
            if near {
                continue;
            }
            taken[cy * cw + cx] = Some((x, y));
            occupied[bucket] = true;
            chosen.push((x, y));
        }
    }
    Some(chosen)
}

fn brief(raster: &GrayRaster, x: f64, y: f64, angle: f64) -> [u64; 2] {
    let mut bits = [0, 0];
    let (sin, cos) = angle.to_radians().sin_cos();
    for (bit, [ax, ay, bx, by]) in brief_pattern().iter().enumerate() {
        let (ax, ay, bx, by) = (*ax as f64, *ay as f64, *bx as f64, *by as f64);
        let a = raster.sample(x + cos * ax - sin * ay, y + sin * ax + cos * ay);
        let b = raster.sample(x + cos * bx - sin * by, y + sin * bx + cos * by);
        if a < b {
            bits[bit / 64] |= 1u64 << (bit % 64);
        }
    }
    bits
}
fn hamming(a: [u64; 2], b: [u64; 2]) -> u32 {
    (a[0] ^ b[0]).count_ones() + (a[1] ^ b[1]).count_ones()
}

#[derive(Clone)]
struct Pair {
    query: Point,
    reference: Point,
    distance: u32,
    query_unit: f64,
    reference_unit: f64,
    /// Rotation implied by the descriptor orientation (query → reference).
    rotation_deg: f64,
}
impl Pair {
    fn scale_hint(&self) -> f64 {
        self.reference_unit / self.query_unit
    }
    fn tolerance(&self, config: &MapMatchConfig) -> f64 {
        config.max_reprojection_error.max(2.0 * self.reference_unit)
    }
}

/// The same physical point found on two adjacent levels is not a "second" match.
#[inline(always)]
fn distinct(a: &Feature, b: &Feature) -> bool {
    distance_between(a.point, b.point) > 3.0 * a.unit.max(b.unit)
}
/// Best distance/index and the best distance of a spatially distinct point.
#[inline(always)]
fn nearest_two_scan(query: [u64; 2], reference: &[Feature]) -> (u32, usize, u32) {
    let mut best = u32::MAX;
    let mut best_index = 0;
    let mut second = u32::MAX;
    for (i, r) in reference.iter().enumerate() {
        let d = hamming(query, r.bits);
        if d < second {
            if d < best {
                if best != u32::MAX && distinct(r, &reference[best_index]) {
                    second = best;
                }
                best = d;
                best_index = i;
            } else if distinct(r, &reference[best_index]) {
                second = d;
            }
        }
    }
    (best, best_index, second)
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "popcnt")]
fn nearest_two_popcnt(query: [u64; 2], reference: &[Feature]) -> (u32, usize, u32) {
    nearest_two_scan(query, reference)
}
fn nearest_two(query: [u64; 2], reference: &[Feature]) -> (u32, usize, u32) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("popcnt") {
        // SAFETY: POPCNT support was verified at runtime just above.
        return unsafe { nearest_two_popcnt(query, reference) };
    }
    nearest_two_scan(query, reference)
}
fn match_features(
    query: &[Feature],
    reference: &[Feature],
    config: &MapMatchConfig,
    clock: Clock,
) -> Option<Vec<Pair>> {
    let mut pairs = Vec::new();
    if reference.is_empty() {
        return Some(pairs);
    }
    for (index, q) in query.iter().enumerate() {
        if index.is_multiple_of(64) && clock.expired() {
            return None;
        }
        let (best, best_index, second) = nearest_two(q.bits, reference);
        if best > config.max_descriptor_hamming {
            continue;
        }
        // Equal distances (also 0 vs 0) are rejected: no unique correspondence.
        if best as f64 >= second.min(BRIEF_BITS as u32) as f64 * config.descriptor_ratio {
            continue;
        }
        let selected = &reference[best_index];
        pairs.push(Pair {
            query: q.point,
            reference: selected.point,
            distance: best,
            query_unit: q.unit,
            reference_unit: selected.unit,
            rotation_deg: -q.rotation_deg,
        });
    }
    pairs.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then(a.query.x.total_cmp(&b.query.x))
            .then(a.query.y.total_cmp(&b.query.y))
    });
    let mut unique = Vec::<Pair>::new();
    for pair in pairs {
        if !unique.iter().any(|p| {
            distance_between(p.query, pair.query) < 2.0 * p.query_unit.max(pair.query_unit)
                || distance_between(p.reference, pair.reference)
                    < 2.0 * p.reference_unit.max(pair.reference_unit)
        }) {
            unique.push(pair);
        }
    }
    Some(unique)
}
fn distance_between(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}
fn angle_difference(a: f64, b: f64) -> f64 {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs()
}
/// A pair supports a transform when its descriptor level pair implies the same
/// scale/rotation and the reprojection lands within the level-aware tolerance.
fn supports(transform: &SimilarityTransform, pair: &Pair, config: &MapMatchConfig) -> bool {
    (transform.scale / pair.scale_hint()).ln().abs() <= SCALE_TOLERANCE
        && angle_difference(transform.rotation_deg, pair.rotation_deg) <= ROTATION_TOLERANCE_DEG
        && transform
            .map_point(pair.query)
            .is_ok_and(|mapped| distance_between(mapped, pair.reference) <= pair.tolerance(config))
}
fn inliers_of(
    transform: &SimilarityTransform,
    pairs: &[Pair],
    config: &MapMatchConfig,
) -> Vec<usize> {
    pairs
        .iter()
        .enumerate()
        .filter(|(_, p)| supports(transform, p, config))
        .map(|(i, _)| i)
        .collect()
}

enum Fit {
    Candidate(Box<MapViewCandidate>),
    Rejected,
    TimedOut,
}

fn fit_reference(
    pairs: &[Pair],
    reference: &MapReference,
    query_levels: &[GrayRaster],
    query_width: u32,
    query_height: u32,
    config: &MapMatchConfig,
    clock: Clock,
) -> Fit {
    let n = pairs.len();
    if n < config.min_inliers {
        return Fit::Rejected;
    }
    let mut best = Vec::new();
    // Small sets are enumerated exhaustively; larger ones use a deterministic LCG.
    let exhaustive = n * (n - 1) / 2 <= config.max_hypotheses;
    let mut seed = 0x243f6a88u64;
    let mut next = |bound: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize % bound
    };
    let mut hypotheses = Vec::with_capacity(config.max_hypotheses);
    if exhaustive {
        for i in 0..n {
            for j in i + 1..n {
                hypotheses.push((i, j));
            }
        }
    } else {
        for _ in 0..config.max_hypotheses {
            hypotheses.push((next(n), next(n)));
        }
    }
    for (iteration, (i, j)) in hypotheses.into_iter().enumerate() {
        if iteration.is_multiple_of(16) && clock.expired() {
            return Fit::TimedOut;
        }
        let (a, b) = (&pairs[i], &pairs[j]);
        if i == j
            || distance_between(a.query, b.query) < 6.0 * a.query_unit.max(b.query_unit)
            || distance_between(a.reference, b.reference)
                < 6.0 * a.reference_unit.max(b.reference_unit)
        {
            continue;
        }
        // A degenerate sample only skips this hypothesis, never the whole reference.
        let Some(transform) = fit_pairs(&[a.clone(), b.clone()]) else {
            continue;
        };
        if !allowed_transform(transform, config)
            || !supports(&transform, a, config)
            || !supports(&transform, b, config)
        {
            continue;
        }
        let inliers = inliers_of(&transform, pairs, config);
        if inliers.len() > best.len() {
            best = inliers;
        }
    }
    if best.len() < config.min_inliers {
        return Fit::Rejected;
    }
    // Least-squares refinement on the consensus set.
    let mut transform = None;
    for _ in 0..3 {
        let accepted: Vec<_> = best.iter().map(|i| pairs[*i].clone()).collect();
        let Some(refined) = fit_pairs(&accepted) else {
            break;
        };
        if !allowed_transform(refined, config) {
            break;
        }
        let refreshed = inliers_of(&refined, pairs, config);
        if refreshed.len() < best.len() {
            if transform.is_none() {
                transform = Some(refined);
            }
            break;
        }
        transform = Some(refined);
        if refreshed == best {
            break;
        }
        best = refreshed;
    }
    let Some(transform) = transform else {
        return Fit::Rejected;
    };
    let inlier_ratio = best.len() as f64 / n as f64;
    if best.len() < config.min_inliers || inlier_ratio < config.min_inlier_ratio {
        return Fit::Rejected;
    }
    let accepted: Vec<_> = best.iter().map(|i| pairs[*i].clone()).collect();
    match describe_candidate(
        &accepted,
        n,
        transform,
        reference,
        query_levels,
        query_width,
        query_height,
        config,
    ) {
        Some(candidate) => Fit::Candidate(Box::new(candidate)),
        None => Fit::Rejected,
    }
}

#[allow(clippy::too_many_arguments)]
fn describe_candidate(
    accepted: &[Pair],
    tentative: usize,
    transform: SimilarityTransform,
    reference: &MapReference,
    query_levels: &[GrayRaster],
    query_width: u32,
    query_height: u32,
    config: &MapMatchConfig,
) -> Option<MapViewCandidate> {
    let width = reference.descriptor.width;
    let height = reference.descriptor.height;
    let corners = |w: f64, h: f64| {
        [
            Point { x: 0.0, y: 0.0 },
            Point { x: w, y: 0.0 },
            Point { x: 0.0, y: h },
            Point { x: w, y: h },
        ]
    };
    let projected_map_bounds = bounds(
        corners(width as f64, height as f64)
            .into_iter()
            .filter_map(|p| transform.inverse_point(p).ok()),
    )?;
    let visible_reference_bounds = bounds(
        corners(query_width as f64 - 1.0, query_height as f64 - 1.0)
            .into_iter()
            .filter_map(|p| transform.map_point(p).ok()),
    )?
    .intersect(PixelRect {
        x: 0.0,
        y: 0.0,
        width: width as f64,
        height: height as f64,
    })?;
    // Inliers must spread over the visible part of the map, not a small cluster.
    let spread = bounds(accepted.iter().map(|p| p.reference))?;
    let visible_area = visible_reference_bounds.width * visible_reference_bounds.height;
    let spread_ratio = spread.width * spread.height / visible_area;
    if spread_ratio < 0.03 {
        return None;
    }
    let correlation = verify_alignment(
        &transform,
        reference,
        query_levels,
        projected_map_bounds,
        accepted,
        config,
    )?;
    if correlation < required_correlation(accepted.len(), spread_ratio, config) {
        return None;
    }
    let mut covered = 0;
    for y in 0..16 {
        for x in 0..16 {
            if transform
                .inverse_point(Point {
                    x: (x as f64 + 0.5) * width as f64 / 16.0,
                    y: (y as f64 + 0.5) * height as f64 / 16.0,
                })
                .is_ok_and(|p| {
                    p.x >= 0.0
                        && p.y >= 0.0
                        && p.x < query_width as f64
                        && p.y < query_height as f64
                })
            {
                covered += 1;
            }
        }
    }
    let inlier_ratio = accepted.len() as f64 / tentative as f64;
    let descriptor_quality = accepted
        .iter()
        .map(|p| 1.0 - p.distance as f64 / BRIEF_BITS as f64)
        .sum::<f64>()
        / accepted.len() as f64;
    let score = (0.5 * correlation.clamp(0.0, 1.0)
        + 0.5 * inlier_ratio.sqrt() * descriptor_quality)
        .clamp(0.0, 1.0);
    Some(MapViewCandidate {
        map_id: reference.metadata.id.clone(),
        score,
        inliers: accepted.len(),
        tentative_matches: tentative,
        inlier_ratio,
        transform,
        projected_map_bounds,
        visible_reference_bounds,
        coverage_ratio: covered as f64 / 256.0,
        reference_width: width,
        reference_height: height,
        query_width,
        query_height,
        unit: reference.metadata.unit.clone(),
        calibration: reference.metadata.calibration,
        verification_correlation: correlation,
    })
}

/// Photometric check of the aligned area covered by the inliers: sparse feature agreement can be
/// produced by repeated structure (grid lines, margins); the image content cannot.
/// Transparent reference pixels are skipped. Returns `None` with too few samples.
fn verify_alignment(
    transform: &SimilarityTransform,
    reference: &MapReference,
    query_levels: &[GrayRaster],
    projected: PixelRect,
    accepted: &[Pair],
    config: &MapMatchConfig,
) -> Option<f64> {
    let base = &query_levels[0];
    let frame = PixelRect {
        x: 0.0,
        y: 0.0,
        width: base.original_width as f64,
        height: base.original_height as f64,
    };
    // A zoomed map may be clipped by its panel: verify where the inliers are,
    // not over the whole projected reference rectangle.
    let visible = projected.intersect(frame)?;
    let support = bounds(accepted.iter().map(|p| p.query))?;
    // Concentrated inliers may sit exactly where two versions of a map (or a
    // repeated pattern) agree locally. The check covers at least 20% of the
    // visible map around them, and up to 50% for a barely sufficient consensus.
    let support_strength = accepted.len() as f64 / config.min_inliers as f64;
    let fraction = 0.2 + 0.3 * ((3.0 - support_strength) / 2.0).clamp(0.0, 1.0);
    let grow = (fraction * visible.width * visible.height / (support.width * support.height))
        .sqrt()
        .max(1.2);
    let support = PixelRect {
        x: support.x - 0.5 * (grow - 1.0) * support.width,
        y: support.y - 0.5 * (grow - 1.0) * support.height,
        width: support.width * grow,
        height: support.height * grow,
    };
    let region = visible.intersect(support)?;
    let spacing = region.width.max(region.height) / VERIFICATION_GRID as f64;
    // Compare both images at the resolution of the sampling grid.
    let query_level = query_levels
        .iter()
        .rev()
        .find(|l| l.unit() <= spacing * 0.5)
        .unwrap_or(base);
    let reference_spacing = spacing * transform.scale;
    let reference_level = reference
        .index
        .verification
        .iter()
        .rev()
        .find(|l| l.unit() <= reference_spacing * 0.5)
        .unwrap_or(&reference.gray);
    let line_tolerance = 1.5 * reference_level.unit().max(spacing * transform.scale * 0.5);
    let mut samples = Vec::with_capacity(VERIFICATION_GRID * VERIFICATION_GRID);
    let mut tiles = Vec::with_capacity(VERIFICATION_GRID * VERIFICATION_GRID);
    for gy in 0..VERIFICATION_GRID {
        for gx in 0..VERIFICATION_GRID {
            let q = Point {
                x: region.x + (gx as f64 + 0.5) * region.width / VERIFICATION_GRID as f64,
                y: region.y + (gy as f64 + 0.5) * region.height / VERIFICATION_GRID as f64,
            };
            let Ok(r) = transform.map_point(q) else {
                continue;
            };
            let rl = reference_level.level_point(r);
            if reference_level.alpha_at(rl.x, rl.y) < 0.9
                || reference.index.near_line(r, line_tolerance)
            {
                continue;
            }
            let ql = query_level.level_point(q);
            // A perfectly flat query neighbourhood (UI panel, solid background)
            // is not map content and would only add a brightness step.
            let centre = query_level.sample(ql.x, ql.y);
            if [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
                .iter()
                .all(|(dx, dy)| (query_level.sample(ql.x + dx, ql.y + dy) - centre).abs() < 0.5)
            {
                continue;
            }
            let tile = (gy * VERIFICATION_TILES / VERIFICATION_GRID) * VERIFICATION_TILES
                + gx * VERIFICATION_TILES / VERIFICATION_GRID;
            samples.push((
                query_level.sample(ql.x, ql.y) as f64,
                reference_level.sample(rl.x, rl.y) as f64,
            ));
            tiles.push(tile);
        }
    }
    if samples.len() < MIN_VERIFICATION_SAMPLES {
        return None;
    }
    let global = robust_correlation(&samples)?;
    // Within-tile correlation ignores brightness steps between regions (a flat
    // panel or background next to the map), which can dominate a global value.
    let (mut weighted, mut weight, mut informative) = (0.0, 0.0, 0);
    for tile in 0..VERIFICATION_TILES * VERIFICATION_TILES {
        let local: Vec<_> = samples
            .iter()
            .zip(&tiles)
            .filter(|(_, t)| **t == tile)
            .map(|(s, _)| *s)
            .collect();
        if local.len() < 30 {
            continue;
        }
        if let Some(c) = robust_correlation(&local) {
            weighted += c * local.len() as f64;
            weight += local.len() as f64;
            informative += 1;
        }
    }
    if informative < 2 {
        return None;
    }
    Some(global.min(weighted / weight))
}
/// Pearson correlation after dropping extreme samples. Overlays (markers,
/// labels) are extreme query values; texture-free extremes are extreme
/// reference values. Selecting on each image separately keeps the correlation
/// of unrelated images near zero. Flat inputs carry no evidence.
fn robust_correlation(samples: &[(f64, f64)]) -> Option<f64> {
    let (mq, sq, mr, sr) = moments(samples);
    if sq < 2.0 || sr < 2.0 {
        return None;
    }
    let inliers: Vec<_> = samples
        .iter()
        .copied()
        .filter(|(q, r)| ((q - mq) / sq).abs() <= 2.0 && ((r - mr) / sr).abs() <= 2.5)
        .collect();
    let samples = if inliers.len() * 10 >= samples.len() * 7 {
        &inliers[..]
    } else {
        samples
    };
    let (mq, sq, mr, sr) = moments(samples);
    if sq < 1.0 || sr < 1.0 {
        return None;
    }
    let covariance = samples
        .iter()
        .map(|(q, r)| (q - mq) * (r - mr))
        .sum::<f64>()
        / samples.len() as f64;
    Some(covariance / (sq * sr))
}
/// Means and standard deviations of (query, reference) samples.
fn moments(samples: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let n = samples.len() as f64;
    let (mq, mr) = samples
        .iter()
        .fold((0.0, 0.0), |(a, b), (q, r)| (a + q / n, b + r / n));
    let (vq, vr) = samples.iter().fold((0.0, 0.0), |(a, b), (q, r)| {
        (a + (q - mq).powi(2) / n, b + (r - mr).powi(2) / n)
    });
    (mq, vq.sqrt(), mr, vr.sqrt())
}

/// Photometric evidence required for a given geometric support. A barely
/// sufficient or spatially concentrated consensus (near-duplicate map versions, repeated structure) needs
/// clearly higher correlation; overwhelming consensus tolerates overlays that
/// lower the correlation (grid, markers, low-texture zoom).
fn required_correlation(inliers: usize, spread_ratio: f64, config: &MapMatchConfig) -> f64 {
    let support = inliers as f64 / config.min_inliers as f64;
    let weak = ((3.0 - support) / 2.0).clamp(0.0, 1.0);
    let strong = ((support - 3.0) / 6.0).clamp(0.0, 1.0);
    // Inliers concentrated in a small part of the visible map are weaker evidence.
    let concentrated = ((0.2 - spread_ratio) / 0.1).clamp(0.0, 1.0);
    (config.min_verification_correlation + 0.25 * weak - 0.15 * strong + 0.15 * concentrated)
        .clamp(0.1, 0.95)
}
fn allowed_transform(transform: SimilarityTransform, config: &MapMatchConfig) -> bool {
    transform.scale >= config.min_scale
        && transform.scale <= config.max_scale
        && config
            .rotations_deg
            .iter()
            .any(|angle| angle_difference(transform.rotation_deg, -angle) <= ROTATION_TOLERANCE_DEG)
}
fn fit_pairs(pairs: &[Pair]) -> Option<SimilarityTransform> {
    if pairs.len() < 2 {
        return None;
    }
    let n = pairs.len() as f64;
    let q = Point {
        x: pairs.iter().map(|p| p.query.x).sum::<f64>() / n,
        y: pairs.iter().map(|p| p.query.y).sum::<f64>() / n,
    };
    let r = Point {
        x: pairs.iter().map(|p| p.reference.x).sum::<f64>() / n,
        y: pairs.iter().map(|p| p.reference.y).sum::<f64>() / n,
    };
    let mut a = 0.0;
    let mut b = 0.0;
    let mut denominator = 0.0;
    for pair in pairs {
        let qx = pair.query.x - q.x;
        let qy = pair.query.y - q.y;
        let rx = pair.reference.x - r.x;
        let ry = pair.reference.y - r.y;
        a += qx * rx + qy * ry;
        b += qx * ry - qy * rx;
        denominator += qx * qx + qy * qy;
    }
    if denominator < 1e-9 {
        return None;
    }
    a /= denominator;
    b /= denominator;
    let result = SimilarityTransform {
        scale: a.hypot(b),
        rotation_deg: b.atan2(a).to_degrees(),
        offset_x: r.x - a * q.x + b * q.y,
        offset_y: r.y - b * q.x - a * q.y,
    };
    result.validate().ok()?;
    Some(result)
}
fn bounds(points: impl IntoIterator<Item = Point>) -> Option<PixelRect> {
    let mut min = Point {
        x: f64::INFINITY,
        y: f64::INFINITY,
    };
    let mut max = Point {
        x: f64::NEG_INFINITY,
        y: f64::NEG_INFINITY,
    };
    for p in points {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    (min.valid() && max.valid() && max.x > min.x && max.y > min.y).then_some(PixelRect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops::FilterType;
    fn metadata(id: &str) -> MapReferenceMetadata {
        MapReferenceMetadata {
            schema_version: 1,
            id: id.into(),
            name: id.into(),
            unit: "game_units".into(),
            source: "Syntetyczna referencja testowa".into(),
            confirmed: true,
            calibration: MapCalibration {
                units_per_pixel: 2.0,
                north_rotation_deg: 0.0,
                origin_pixel: Point { x: 0.0, y: 0.0 },
                origin_world: Point { x: 0.0, y: 0.0 },
                uncertainty_units: 1.0,
            },
        }
    }
    fn textured(seed: u32) -> RgbaImage {
        RgbaImage::from_fn(160, 160, |x, y| {
            let mut state = seed
                .wrapping_add((x / 5).wrapping_mul(7_919))
                .wrapping_add((y / 5).wrapping_mul(104_729));
            state ^= state >> 16;
            state = state.wrapping_mul(0x7feb352d);
            state ^= state >> 15;
            state = state.wrapping_mul(0x846ca68b);
            state ^= state >> 16;
            let gray = (40 + (state % 170)) as u8;
            image::Rgba([gray, gray, gray, 255])
        })
    }
    fn config() -> MapMatchConfig {
        MapMatchConfig::default()
    }
    fn rotating_config() -> MapMatchConfig {
        MapMatchConfig {
            rotations_deg: vec![0.0, 90.0, 180.0, 270.0],
            ..config()
        }
    }
    /// Geometry is deterministic; the wall clock is tested separately.
    fn matched(
        query: &RgbaImage,
        references: &[MapReference],
        config: &MapMatchConfig,
    ) -> MapViewMatch {
        match_map_view_with_clock(query, references, config, Clock(None), Instant::now()).unwrap()
    }
    fn render_frame(
        reference: &RgbaImage,
        transform: SimilarityTransform,
        width: u32,
        height: u32,
    ) -> RgbaImage {
        let gray = GrayRaster::from_rgba(reference, 512);
        RgbaImage::from_fn(width, height, |x, y| {
            let p = transform
                .map_point(Point {
                    x: x as f64,
                    y: y as f64,
                })
                .unwrap();
            let value = if p.x >= 0.0
                && p.y >= 0.0
                && p.x <= reference.width() as f64 - 1.0
                && p.y <= reference.height() as f64 - 1.0
            {
                gray.sample(p.x, p.y).round() as u8
            } else {
                20
            };
            image::Rgba([value, value, value, 255])
        })
    }
    fn assert_point(candidate: &MapViewCandidate, point: Point, expected: Point) {
        let observed = candidate.reference_point(point).unwrap();
        assert!(
            distance_between(observed, expected) < 4.0,
            "Expected {:?}, observed {:?}, transform {:?}",
            expected,
            observed,
            candidate.transform
        );
        let world = candidate.calibrated_world_point(point).unwrap();
        assert!(
            distance_between(
                world,
                Point {
                    x: expected.x * 2.0,
                    y: -expected.y * 2.0
                }
            ) < 8.0
        );
    }
    #[test]
    fn matches_partial_crop_and_maps_query_pixels_to_reference_world() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let crop = image::imageops::crop_imm(&image, 31, 25, 112, 112).to_image();
        let result = matched(&crop, &[reference], &config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        let candidate = result.best.unwrap();
        assert_point(
            &candidate,
            Point { x: 40.0, y: 50.0 },
            Point { x: 71.0, y: 75.0 },
        );
        assert!(candidate.coverage_ratio < 1.0);
        assert!(candidate.verification_correlation > 0.9);
        assert!(candidate
            .reference_point(Point { x: -1.0, y: 10.0 })
            .is_err());
    }
    #[test]
    fn finds_scaled_map_inside_a_full_client_frame() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let expected = SimilarityTransform {
            scale: 0.5,
            rotation_deg: 0.0,
            offset_x: -20.0,
            offset_y: -15.0,
        };
        let frame = render_frame(&image, expected, 400, 380);
        let result = matched(&frame, &[reference], &config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        let candidate = result.best.unwrap();
        assert_point(
            &candidate,
            Point { x: 140.0, y: 130.0 },
            Point { x: 50.0, y: 50.0 },
        );
        assert!((candidate.projected_map_bounds.x - 40.0).abs() < 5.0);
        assert!(candidate.reference_point(Point { x: 0.0, y: 0.0 }).is_err());
    }
    #[test]
    fn finds_large_flat_tiles_under_fontlike_hud_in_a_982_by_653_client() {
        let tiles = RgbaImage::from_fn(350, 200, |x, y| {
            let row = y / 20;
            let column = x / 25;
            let gray = (40 + (row * 37 + column * 61 + row * column * 7) % 170) as u8;
            image::Rgba([gray, gray, gray, 255])
        });
        let reference = MapReference::new(metadata("tiles"), &tiles).unwrap();
        let mut frame = RgbaImage::from_pixel(982, 653, image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut frame, &tiles, 270, 240);
        // Strong separated glyph-like corners at the real helper's HUD locations.
        for (left, top, letters, size) in [
            (400u32, 50u32, 5u32, 48u32),
            (40, 130, 12, 24),
            (40, 180, 19, 18),
            (40, 215, 24, 14),
            (40, 465, 19, 18),
            (40, 510, 9, 18),
            (40, 555, 12, 18),
        ] {
            for character in 0..letters {
                for y in 0..size {
                    for x in 0..size / 2 {
                        if x < 3
                            || y < 3
                            || y.abs_diff(size / 2) < 2
                            || (character % 2 == 0 && x > size / 2 - 4)
                        {
                            frame.put_pixel(
                                left + character * (size / 2 + 4) + x,
                                top + y,
                                image::Rgba([0, 0, 0, 255]),
                            );
                        }
                    }
                }
            }
        }
        for (left, top, rgb) in [(310, 270, [0, 255, 0, 255]), (500, 370, [255, 0, 0, 255])] {
            for y in top..top + 12 {
                for x in left..left + 12 {
                    frame.put_pixel(x, y, image::Rgba(rgb));
                }
            }
        }
        let result = matched(&frame, &[reference], &config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        let candidate = result.best.unwrap();
        assert_point(
            &candidate,
            Point { x: 316.0, y: 276.0 },
            Point { x: 46.0, y: 36.0 },
        );
        assert_point(
            &candidate,
            Point { x: 506.0, y: 376.0 },
            Point { x: 236.0, y: 136.0 },
        );
        assert!(candidate.inliers >= 12 && candidate.inlier_ratio >= 0.3);
        assert!((candidate.projected_map_bounds.x - 270.0).abs() < 3.0);
        assert!((candidate.projected_map_bounds.y - 240.0).abs() < 3.0);
        let wrong_tiles = RgbaImage::from_fn(350, 200, |x, y| {
            let row = y / 20;
            let column = x / 25;
            let gray = (40 + (row * 53 + column * 29 + row * column * 11) % 170) as u8;
            image::Rgba([gray, gray, gray, 255])
        });
        let wrong = matched(
            &frame,
            &[MapReference::new(metadata("wrong-tiles"), &wrong_tiles).unwrap()],
            &config(),
        );
        assert_eq!(wrong.status, MapViewStatus::NoMatch, "{wrong:?}");
    }
    #[test]
    fn finds_rotated_map_in_frame_without_confusing_image_and_world_axes() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let expected = SimilarityTransform {
            scale: 1.0,
            rotation_deg: -90.0,
            offset_x: -25.0,
            offset_y: 200.0,
        };
        let frame = render_frame(&image, expected, 250, 220);
        let result = matched(&frame, std::slice::from_ref(&reference), &rotating_config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        assert_point(
            &result.best.unwrap(),
            Point { x: 130.0, y: 65.0 },
            Point { x: 40.0, y: 70.0 },
        );
        // The default search is north-up only and refuses the rotated view.
        let upright = matched(&frame, &[reference], &config());
        assert_ne!(upright.status, MapViewStatus::Matched, "{upright:?}");
    }
    #[test]
    fn matches_zoomed_rotated_partial_view() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let expected = SimilarityTransform {
            scale: 0.75,
            rotation_deg: 90.0,
            offset_x: 140.0,
            offset_y: -40.0,
        };
        let frame = render_frame(&image, expected, 224, 192);
        let result = matched(&frame, &[reference], &rotating_config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        assert_point(
            &result.best.unwrap(),
            Point { x: 120.0, y: 120.0 },
            Point { x: 50.0, y: 50.0 },
        );
    }
    #[test]
    fn small_map_inside_stronger_checkerboard_scene_is_located_or_refused_never_misplaced() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let expected = SimilarityTransform {
            scale: 1.0,
            rotation_deg: 0.0,
            offset_x: -400.0,
            offset_y: -250.0,
        };
        let mut frame = render_frame(&image, expected, 640, 480);
        for y in 0u32..480 {
            for x in 0u32..640 {
                if !(400..560).contains(&x) || !(250..410).contains(&y) {
                    let value = if (x / 8 + y / 8).is_multiple_of(2) {
                        0
                    } else {
                        255
                    };
                    frame.put_pixel(x, y, image::Rgba([value, value, value, 255]));
                }
            }
        }
        let result = matched(&frame, &[reference], &config());
        match result.status {
            MapViewStatus::Matched => assert_point(
                result.best.as_ref().unwrap(),
                Point { x: 450.0, y: 300.0 },
                Point { x: 50.0, y: 50.0 },
            ),
            MapViewStatus::NoMatch => assert!(result.best.is_none()),
            other => panic!("unexpected {other:?}"),
        }
    }
    #[test]
    fn configured_oblique_rotation_is_an_explicit_search_choice() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let expected = SimilarityTransform {
            scale: 1.0,
            rotation_deg: -45.0,
            offset_x: -75.0,
            offset_y: 80.0,
        };
        let frame = render_frame(&image, expected, 300, 300);
        let mut config = config();
        config.rotations_deg = vec![0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let result = matched(&frame, &[reference], &config);
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        let query = expected.inverse_point(Point { x: 80.0, y: 80.0 }).unwrap();
        assert_point(&result.best.unwrap(), query, Point { x: 80.0, y: 80.0 });
    }
    #[test]
    fn exhausted_time_budget_never_publishes_a_partial_catalog_winner() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let query = image::imageops::resize(&image, 800, 800, FilterType::Triangle);
        let config = MapMatchConfig {
            max_processing_ms: 1,
            ..Default::default()
        };
        let result = match_map_view(&query, &[reference], &config).unwrap();
        assert_eq!(result.status, MapViewStatus::TimedOut);
        assert!(result.best.is_none());
    }
    #[test]
    fn expired_clock_is_reported_before_any_candidate_is_published() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let started = Instant::now();
        let result = match_map_view_with_clock(
            &image,
            &[reference],
            &config(),
            Clock(Some(started)),
            started,
        )
        .unwrap();
        assert_eq!(result.status, MapViewStatus::TimedOut);
        assert!(result.best.is_none());
    }
    #[test]
    fn rejects_wrong_map_and_ambiguity_between_identical_references() {
        let a = textured(123);
        let b = textured(456);
        let result = matched(
            &b,
            &[MapReference::new(metadata("map-a"), &a).unwrap()],
            &config(),
        );
        assert_eq!(result.status, MapViewStatus::NoMatch, "{result:?}");
        let result = matched(
            &a,
            &[
                MapReference::new(metadata("map-a"), &a).unwrap(),
                MapReference::new(metadata("map-b"), &a).unwrap(),
            ],
            &config(),
        );
        assert_eq!(result.status, MapViewStatus::Ambiguous, "{result:?}");
        assert!(result.best.is_none());
    }
    #[test]
    fn repeated_texture_does_not_assign_an_arbitrary_location_on_the_same_map() {
        let image = RgbaImage::from_fn(160, 160, |x, y| {
            let v = if (x / 5 + y / 5).is_multiple_of(2) {
                40
            } else {
                200
            };
            image::Rgba([v, v, v, 255])
        });
        let reference = MapReference::new(metadata("repeated"), &image).unwrap();
        let crop = image::imageops::crop_imm(&image, 25, 25, 110, 110).to_image();
        let result = matched(&crop, &[reference], &config());
        assert_eq!(result.status, MapViewStatus::NoMatch, "{result:?}");
        assert!(result.best.is_none());
    }
    #[test]
    fn reference_registry_replaces_same_identity_and_rejects_unconfirmed_or_blank_assets() {
        let image = textured(123);
        let mut registry = MapReferenceRegistry::default();
        registry
            .register(MapReference::new(metadata("map-a"), &image).unwrap())
            .unwrap();
        registry
            .register(MapReference::new(metadata("map-a"), &image).unwrap())
            .unwrap();
        assert_eq!(registry.references().len(), 1);
        assert_eq!(registry.descriptors()[0].gray_samples.len(), 256);
        let mut inconsistent = registry.references()[0].clone();
        inconsistent.descriptor.width += 1;
        assert!(registry.register(inconsistent).is_err());
        let mut m = metadata("map-a");
        m.confirmed = false;
        assert!(MapReference::new(m, &image).is_err());
        assert!(MapReference::new(
            metadata("blank"),
            &RgbaImage::from_pixel(160, 160, image::Rgba([50, 50, 50, 255]))
        )
        .is_err());
        assert!(registry.remove("map-a"));
        assert!(registry.references().is_empty());
        assert_eq!(
            matched(&image, &[], &config()).status,
            MapViewStatus::MissingReferences
        );
    }
    #[test]
    fn inverse_transform_round_trips_and_configuration_is_bounded() {
        let transform = SimilarityTransform {
            scale: 0.75,
            rotation_deg: 45.0,
            offset_x: 17.0,
            offset_y: -23.0,
        };
        let point = Point { x: 52.0, y: 73.0 };
        assert!(
            distance_between(
                transform
                    .inverse_point(transform.map_point(point).unwrap())
                    .unwrap(),
                point
            ) < 1e-8
        );
        let mut config = config();
        config.max_processing_ms = 0;
        assert!(config.validate().is_err());
        config.max_processing_ms = 1501;
        assert!(config.validate().is_err());
        let config = MapMatchConfig {
            min_verification_correlation: f64::NAN,
            ..MapMatchConfig::default()
        };
        assert!(config.validate().is_err());
    }
    #[test]
    fn reference_features_are_computed_once_at_registration_and_shared_by_clones() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        assert!(reference.feature_count() > 50);
        let clone = reference.clone();
        assert!(Arc::ptr_eq(&reference.index, &clone.index));
    }
    #[test]
    fn degenerate_ransac_sample_does_not_discard_the_reference() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let shift = Point { x: 3.0, y: -2.0 };
        let mut pairs = Vec::new();
        // Coincident query points first: fitting them is degenerate.
        for _ in 0..3 {
            pairs.push(Pair {
                query: Point { x: 50.0, y: 50.0 },
                reference: Point { x: 53.0, y: 48.0 },
                distance: 0,
                query_unit: 1.0,
                reference_unit: 1.0,
                rotation_deg: 0.0,
            });
        }
        for i in 0..12 {
            let q = Point {
                x: 20.0 + (i % 4) as f64 * 35.0,
                y: 20.0 + (i / 4) as f64 * 40.0,
            };
            pairs.push(Pair {
                query: q,
                reference: Point {
                    x: q.x + shift.x,
                    y: q.y + shift.y,
                },
                distance: 5,
                query_unit: 1.0,
                reference_unit: 1.0,
                rotation_deg: 0.0,
            });
        }
        let shifted = RgbaImage::from_fn(160, 160, |x, y| {
            *image.get_pixel(
                (x as i64 + 3).clamp(0, 159) as u32,
                (y as i64 - 2).clamp(0, 159) as u32,
            )
        });
        let levels = GrayRaster::from_rgba(&shifted, 1024).halving_pyramid();
        let fit = fit_reference(
            &pairs,
            &reference,
            &levels,
            160,
            160,
            &config(),
            Clock(None),
        );
        let Fit::Candidate(candidate) = fit else {
            panic!("expected a candidate");
        };
        assert!((candidate.transform.offset_x - shift.x).abs() < 0.5);
        assert!((candidate.transform.scale - 1.0).abs() < 0.01);
    }
    #[test]
    fn area_reduction_preserves_thin_grid_lines_instead_of_aliasing_them() {
        // One-pixel lines every 7 px: point sampling at a 4× reduction hits or
        // misses them depending on phase; area averaging keeps the mean.
        let image = RgbaImage::from_fn(1024, 1024, |x, y| {
            let v = if x % 7 == 0 || y % 7 == 0 { 255 } else { 0 };
            image::Rgba([v, v, v, 255])
        });
        let gray = GrayRaster::from_rgba(&image, 256);
        let mean = gray.data.iter().sum::<f32>() / gray.data.len() as f32;
        let expected = 255.0 * (1.0 - (6.0f32 / 7.0).powi(2));
        assert!((mean - expected).abs() < 6.0, "{mean} vs {expected}");
        let (lo, hi) = gray
            .data
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(hi - lo < 140.0, "{lo}..{hi}");
    }
    #[test]
    fn level_coordinates_use_pixel_centres_in_both_directions() {
        let image = textured(1);
        let gray = GrayRaster::from_rgba(&image, 80);
        let p = gray.original_point(10.0, 20.0);
        assert!((p.x - 20.5).abs() < 1e-9 && (p.y - 40.5).abs() < 1e-9);
        let back = gray.level_point(p);
        assert!((back.x - 10.0).abs() < 1e-9 && (back.y - 20.0).abs() < 1e-9);
    }
    #[test]
    fn transparent_texture_margins_produce_no_reference_features() {
        let mut image = textured(77);
        for y in 0..50 {
            for x in 0..160 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        let reference = MapReference::new(metadata("margins"), &image).unwrap();
        assert!(reference.feature_count() > 10);
        assert!(reference.index.features.iter().all(|f| f.point.y >= 49.0));
    }
    #[test]
    fn darkened_low_contrast_view_still_matches() {
        let image = textured(321);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let mut crop = image::imageops::crop_imm(&image, 20, 30, 120, 120).to_image();
        for p in crop.pixels_mut() {
            for c in 0..3 {
                p[c] = (((p[c] as f64 / 255.0).powf(1.3) * 255.0) * 0.35 + 10.0) as u8;
            }
        }
        let result = matched(&crop, &[reference], &config());
        assert_eq!(result.status, MapViewStatus::Matched, "{result:?}");
        assert_point(
            &result.best.unwrap(),
            Point { x: 50.0, y: 40.0 },
            Point { x: 70.0, y: 70.0 },
        );
    }
    #[test]
    fn matching_without_a_clock_is_deterministic() {
        let image = textured(123);
        let reference = MapReference::new(metadata("map-a"), &image).unwrap();
        let crop = image::imageops::crop_imm(&image, 31, 25, 112, 112).to_image();
        let a = matched(&crop, std::slice::from_ref(&reference), &config());
        let b = matched(&crop, &[reference], &config());
        let (a, b) = (a.best.unwrap(), b.best.unwrap());
        assert_eq!(a.inliers, b.inliers);
        assert_eq!(
            a.transform.offset_x.to_bits(),
            b.transform.offset_x.to_bits()
        );
        assert_eq!(a.score.to_bits(), b.score.to_bits());
    }
}
