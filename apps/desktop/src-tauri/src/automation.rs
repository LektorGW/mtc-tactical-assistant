use crate::{now_ms, read_image, Runtime};
use image::RgbaImage;
use mtc_maps::{MapCalibration, Point};
use mtc_vision::{AnalysisConfig, HudDiscovery, OcrRegionConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredMap {
    pub sha256: String,
    pub id: String,
    pub name: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub unit: String,
    pub source: String,
    pub confirmed: bool,
    pub calibration: MapCalibration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredTerrain {
    pub sha256: String,
    pub path: String,
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

#[derive(Default)]
pub struct AutomationAssets {
    pub maps: Vec<StoredMap>,
    pub references: Vec<mtc_maps::MapReference>,
    pub terrains: Vec<Arc<mtc_maps::HeightMap>>,
    /// Records of the loaded terrains, same order as `terrains`.
    pub terrain_records: Vec<StoredTerrain>,
    /// Entries skipped by a lenient load; never silently replaced by older data.
    pub warnings: Vec<String>,
}

/// What is actually usable in RAM after the latest library load.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsStatus {
    pub maps: Vec<String>,
    pub terrains: Vec<String>,
    pub warnings: Vec<String>,
    pub terrain_bytes: usize,
}

#[derive(Default)]
pub struct AutomationState {
    scope: String,
    previous_hud: Option<HudDiscovery>,
    applied_hud: Vec<OcrRegionConfig>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationOptions {
    #[serde(default)]
    pub auto_hud: bool,
    #[serde(default)]
    pub auto_map: bool,
    #[serde(default)]
    pub preferred_map_id: Option<String>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationReport {
    #[serde(skip)]
    markers_requested: bool,
    pub hud: Option<HudDiscovery>,
    pub hud_applied: bool,
    pub map: Option<mtc_maps::MapViewMatch>,
    pub map_name: Option<mtc_maps::MapNameMatch>,
    pub active_map: Option<StoredMap>,
    pub weapon: Option<mtc_profiles::WeaponResolution>,
    pub warnings: Vec<String>,
}

/// Loads the reference/terrain library into RAM.
///
/// Strict mode (default, used by explicit imports) rejects the whole library on
/// the first invalid entry and returns the map count. `lenient: true` skips
/// missing or changed files with a warning (see `automation_assets_status`), so
/// one deleted cache file cannot disable every other map and heightmap.
/// Unchanged entries (same record and SHA-256) reuse the decoded raster.
#[tauri::command]
pub async fn load_automation_assets(
    app: tauri::AppHandle,
    maps: Vec<StoredMap>,
    terrain: Option<StoredTerrain>,
    terrains: Option<Vec<StoredTerrain>>,
    lenient: Option<bool>,
) -> Result<usize, String> {
    use std::sync::atomic::Ordering;
    let generation = app
        .state::<Runtime>()
        .assets_generation
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let mut terrains = terrains.unwrap_or_default();
        if let Some(terrain) = terrain {
            terrains.retain(|t| t.map_id != terrain.map_id);
            terrains.push(terrain);
        }
        let previous = {
            let state = app.state::<Runtime>();
            let assets = state.assets.lock().map_err(|e| e.to_string())?;
            ReusableAssets::from(&*assets)
        };
        let loaded = load_assets(maps, terrains, lenient.unwrap_or(false), &previous);
        let state = app.state::<Runtime>();
        let _lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
        if generation != state.assets_generation.load(Ordering::Acquire) {
            return Err("Ładowanie zastąpiono nowszą biblioteką".into());
        }
        state.capture_epoch.fetch_add(1, Ordering::AcqRel);
        *state.automation.lock().map_err(|e| e.to_string())? = AutomationState::default();
        *state.vision.lock().map_err(|e| e.to_string())? = crate::VisionState::default();
        state
            .marker_setup
            .lock()
            .map_err(|e| e.to_string())?
            .clear();
        let mut assets = state.assets.lock().map_err(|e| e.to_string())?;
        // A missing/replaced file cannot leave the previous calibrated raster active.
        *assets = AutomationAssets::default();
        *assets = loaded?;
        Ok(assets.maps.len())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn automation_assets_status(state: State<Runtime>) -> Result<AssetsStatus, String> {
    let assets = state.assets.lock().map_err(|e| e.to_string())?;
    Ok(AssetsStatus {
        maps: assets.maps.iter().map(|m| m.id.clone()).collect(),
        terrains: assets
            .terrain_records
            .iter()
            .map(|t| t.map_id.clone())
            .collect(),
        warnings: assets.warnings.clone(),
        terrain_bytes: assets.terrains.iter().map(|t| t.memory_bytes()).sum(),
    })
}

/// Cheap (Arc) handles to the previously decoded library.
#[derive(Default)]
struct ReusableAssets {
    maps: Vec<(serde_json::Value, mtc_maps::MapReference)>,
    terrains: Vec<(serde_json::Value, Arc<mtc_maps::HeightMap>)>,
}
impl From<&AutomationAssets> for ReusableAssets {
    fn from(assets: &AutomationAssets) -> Self {
        Self {
            maps: assets
                .maps
                .iter()
                .filter_map(|m| {
                    let reference = assets.references.iter().find(|r| r.metadata.id == m.id)?;
                    Some((serde_json::to_value(m).ok()?, reference.clone()))
                })
                .collect(),
            terrains: assets
                .terrain_records
                .iter()
                .zip(&assets.terrains)
                .filter_map(|(t, h)| Some((serde_json::to_value(t).ok()?, h.clone())))
                .collect(),
        }
    }
}

fn load_assets(
    maps: Vec<StoredMap>,
    terrains: Vec<StoredTerrain>,
    lenient: bool,
    previous: &ReusableAssets,
) -> Result<AutomationAssets, String> {
    if maps.len() > 32 || terrains.len() > 32 {
        return Err("Biblioteka może zawierać do 32 map".into());
    }
    let mut warnings = Vec::new();
    let mut skip = |message: String| -> Result<(), String> {
        if lenient {
            warnings.push(message);
            Ok(())
        } else {
            Err(message)
        }
    };
    let mut ids = std::collections::HashSet::new();
    let mut loaded_maps = Vec::new();
    let mut references = Vec::new();
    for map in maps {
        if !ids.insert(map.id.clone()) || map.path.trim().is_empty() {
            skip("Powtórzona mapa lub brak ścieżki".into())?;
            continue;
        }
        match load_reference(&map, previous) {
            Ok(reference) => {
                references.push(reference);
                loaded_maps.push(map);
            }
            Err(e) => skip(e)?,
        }
    }
    let mut loaded_terrains = Vec::new();
    let mut terrain_records = Vec::new();
    let mut terrain_ids = std::collections::HashSet::new();
    for t in terrains {
        if !terrain_ids.insert(t.map_id.clone()) {
            skip("Powtórzona mapa wysokości".into())?;
            continue;
        }
        if !loaded_maps.iter().any(|m| {
            m.id == t.map_id
                && m.width == t.map_width
                && m.height == t.map_height
                && m.unit == t.unit
        }) {
            skip(format!(
                "Mapa wysokości {} nie zgadza się z wczytaną biblioteką map",
                t.map_id
            ))?;
            continue;
        }
        match load_terrain(&t, previous) {
            Ok(height) => {
                loaded_terrains.push(height);
                terrain_records.push(t);
            }
            Err(e) => skip(e)?,
        }
    }
    Ok(AutomationAssets {
        maps: loaded_maps,
        references,
        terrains: loaded_terrains,
        terrain_records,
        warnings,
    })
}

fn load_reference(
    map: &StoredMap,
    previous: &ReusableAssets,
) -> Result<mtc_maps::MapReference, String> {
    let (bytes, sha256) = crate::read_file_fingerprint(&map.path)
        .map_err(|e| format!("Mapa {} — nie odczytano pliku: {e}", map.name))?;
    if sha256 != map.sha256 {
        return Err(format!(
            "Zmieniła się zawartość mapy {} — ponów import i kalibrację",
            map.name
        ));
    }
    let record = serde_json::to_value(map).map_err(|e| e.to_string())?;
    if let Some((_, reference)) = previous.maps.iter().find(|(r, _)| *r == record) {
        return Ok(reference.clone());
    }
    let raster = crate::decode_image(bytes)?;
    if raster.width() != map.width || raster.height() != map.height {
        return Err(format!(
            "Zmienił się rozmiar mapy {} — ponów import i kalibrację",
            map.name
        ));
    }
    mtc_maps::MapReference::new(
        mtc_maps::MapReferenceMetadata {
            schema_version: 1,
            id: map.id.clone(),
            name: map.name.clone(),
            unit: map.unit.clone(),
            source: map.source.clone(),
            confirmed: map.confirmed,
            calibration: map.calibration,
        },
        &raster,
    )
}

fn load_terrain(
    t: &StoredTerrain,
    previous: &ReusableAssets,
) -> Result<Arc<mtc_maps::HeightMap>, String> {
    let (bytes, sha256) = crate::read_file_fingerprint(&t.path)
        .map_err(|e| format!("Mapa wysokości {} — nie odczytano pliku: {e}", t.map_id))?;
    if sha256 != t.sha256 {
        return Err(format!(
            "Zmieniła się zawartość mapy wysokości {} — ponów import",
            t.map_id
        ));
    }
    let record = serde_json::to_value(t).map_err(|e| e.to_string())?;
    if let Some((_, height)) = previous.terrains.iter().find(|(r, _)| *r == record) {
        return Ok(height.clone());
    }
    let raster = crate::decode_image(bytes)?;
    Ok(Arc::new(mtc_maps::HeightMap::from_rgba(
        &raster,
        mtc_maps::HeightMapMetadata {
            schema_version: 1,
            map_id: t.map_id.clone(),
            map_width: t.map_width,
            map_height: t.map_height,
            min_height: t.min_height,
            max_height: t.max_height,
            unit: t.unit.clone(),
            source: t.source.clone(),
            confirmed: t.confirmed,
            coordinate_alignment_confirmed: t.coordinate_alignment_confirmed,
        },
    )?))
}

#[tauri::command]
pub fn terrain_height(
    state: State<Runtime>,
    map_id: String,
    unit: String,
    own: Point,
    target: Point,
) -> Result<f64, String> {
    let assets = state.assets.lock().map_err(|e| e.to_string())?;
    assets
        .terrains
        .iter()
        .find(|t| t.metadata().map_id == map_id)
        .ok_or("Nie wczytano mapy wysokości")?
        .height_delta(own, target, &map_id, &unit)
}

#[tauri::command]
pub fn fit_weapon(
    profile: mtc_profiles::WeaponProfile,
    samples: Vec<mtc_ballistics::CalibrationSample>,
    options: mtc_ballistics::CalibrationOptions,
) -> Result<mtc_ballistics::CalibrationFit, String> {
    mtc_ballistics::fit_vacuum(&profile, &samples, &options)
}

#[tauri::command]
pub fn resolve_weapon(
    vehicle: Option<mtc_profiles::OcrWeaponLabel>,
    ammunition: Option<mtc_profiles::OcrWeaponLabel>,
) -> Result<mtc_profiles::WeaponResolution, String> {
    mtc_profiles::resolve_weapon_labels(
        &mtc_profiles::all_profiles(),
        vehicle.as_ref(),
        ammunition.as_ref(),
    )
}

#[tauri::command]
pub async fn discover_hud_file(path: String) -> Result<HudDiscovery, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mtc_vision::discover_hud(&read_image(&path)?, now_ms(), now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Region location agreement is separate from measurement agreement. Values still
/// need their normal fresh two-frame stabilization in the vision pipeline.
fn stabilize_hud(current: &mut HudDiscovery, state: &mut AutomationState) {
    let previous = state.previous_hud.as_ref();
    let mut applied = Vec::new();
    for proposal in &mut current.regions {
        let found = previous.and_then(|p| {
            if p.reference_width != current.reference_width
                || p.reference_height != current.reference_height
                || p.timestamp_ms >= current.timestamp_ms
                || current.timestamp_ms - p.timestamp_ms > 2000
            {
                return None;
            }
            let matches: Vec<_> = p
                .regions
                .iter()
                .filter(|r| {
                    r.region.kind == proposal.region.kind
                        && roi_overlap(r.region.roi, proposal.region.roi) >= 0.5
                })
                .collect();
            (matches.len() == 1).then(|| matches[0])
        });
        if let Some(prior) = found {
            if prior.confidence >= 0.6 && proposal.confidence >= 0.6 {
                proposal.confidence = 0.9;
                // Preserve a stable crop instead of resetting measurement history for OCR jitter.
                if let Some(region) = state.applied_hud.iter().find(|r| {
                    r.kind == proposal.region.kind && roi_overlap(r.roi, proposal.region.roi) >= 0.5
                }) {
                    proposal.region = region.clone();
                }
                applied.push(proposal.region.clone());
            }
        }
    }
    state.applied_hud = applied;
    state.previous_hud = Some(current.clone());
}

fn roi_overlap(a: mtc_vision::Roi, b: mtc_vision::Roi) -> f64 {
    let intersection = (a.x + a.width).min(b.x + b.width) - (a.x.max(b.x));
    let height = (a.y + a.height).min(b.y + b.height) - (a.y.max(b.y));
    let area = intersection.max(0.0) * height.max(0.0);
    area / (a.width * a.height + b.width * b.height - area).max(1e-12)
}

pub fn prepare(
    app: &tauri::AppHandle,
    image: &RgbaImage,
    timestamp: u64,
    scope: &str,
    options: &AutomationOptions,
    config: &mut AnalysisConfig,
) -> Result<AutomationReport, String> {
    let runtime = app.state::<Runtime>();
    let mut report = AutomationReport {
        markers_requested: config.marker_config.as_ref().is_some_and(|m| m.enabled),
        ..AutomationReport::default()
    };
    if options.auto_hud {
        let mut discovery = mtc_vision::discover_hud(image, timestamp, now_ms())?;
        let mut state = runtime.automation.lock().map_err(|e| e.to_string())?;
        if state.scope != scope {
            *state = AutomationState {
                scope: scope.into(),
                ..AutomationState::default()
            };
        }
        stabilize_hud(&mut discovery, &mut state);
        config.ocr_regions = state.applied_hud.clone();
        config.reference_width = image.width();
        config.reference_height = image.height();
        report.hud_applied = !config.ocr_regions.is_empty();
        report.warnings.extend(discovery.warnings.clone());
        report.hud = Some(discovery);
    }
    if options.auto_map {
        let assets = runtime.assets.lock().map_err(|e| e.to_string())?;
        // An explicit user preference narrows candidates, but never replaces geometric proof.
        let selected: Vec<_> = assets
            .references
            .iter()
            .filter(|r| {
                options
                    .preferred_map_id
                    .as_ref()
                    .is_none_or(|id| &r.metadata.id == id)
            })
            .cloned()
            .collect();
        let matched =
            mtc_maps::match_map_view(image, &selected, &mtc_maps::MapMatchConfig::default())?;
        if let Some(candidate) = &matched.best {
            if matched.status == mtc_maps::MapViewStatus::Matched {
                report.active_map = assets
                    .maps
                    .iter()
                    .find(|m| m.id == candidate.map_id)
                    .cloned();
            }
        }
        if report.active_map.is_none() {
            report.warnings.push(
                "Brak jednoznacznej mapy z biblioteki — pozycje automatyczne wstrzymane".into(),
            );
        }
        // Segmentation below runs only after the geometric reference match.
        if let Some(markers) = config.marker_config.as_mut() {
            markers.enabled = false;
        }
        report.map = Some(matched);
    }
    Ok(report)
}

pub fn map_markers(
    image: &RgbaImage,
    timestamp: u64,
    config: &mut AnalysisConfig,
    report: &AutomationReport,
    analysis: &mut mtc_vision::FrameAnalysis,
) -> Result<(), String> {
    analysis.markers.clear();
    if !report.markers_requested {
        analysis.marker_warning =
            Some("Włącz odczyt markerów i skonfiguruj ich role oraz kolory".into());
        return Ok(());
    }
    let Some(reference) = report.active_map.as_ref() else {
        analysis.marker_warning = Some("Nie rozpoznano skalibrowanej mapy".into());
        return Ok(());
    };
    let Some(candidate) = report.map.as_ref().and_then(|m| m.best.as_ref()) else {
        return Ok(());
    };
    let Some(markers) = config.marker_config.as_mut() else {
        return Ok(());
    };
    let bounds = candidate
        .projected_map_bounds
        .intersect(mtc_maps::PixelRect {
            x: 0.0,
            y: 0.0,
            width: image.width() as f64,
            height: image.height() as f64,
        })
        .ok_or("Mapa nie leży w widocznej klatce")?;
    let roi = mtc_vision::Roi {
        x: bounds.x / image.width() as f64,
        y: bounds.y / image.height() as f64,
        width: bounds.width / image.width() as f64,
        height: bounds.height / image.height() as f64,
    };
    let crop = roi.crop(image)?;
    let mut segmentation = markers.clone();
    segmentation.enabled = true;
    segmentation.coordinate_calibration_confirmed = true;
    segmentation.map_roi = roi;
    segmentation.map_width = crop.width();
    segmentation.map_height = crop.height();
    segmentation.view_signature = Some(mtc_vision::calibrate_view_signature(image, roi)?);
    // Segment sizes are configured in reference pixels. The similarity maps
    // observed pixels to reference pixels, so areas scale with scale squared.
    let area_scale = candidate.transform.scale.powi(2);
    for signature in &mut segmentation.signatures {
        signature.min_component_pixels = (signature.min_component_pixels as f64 / area_scale)
            .ceil()
            .clamp(1.0, 1_000_000.0) as u32;
        signature.max_component_pixels = (signature.max_component_pixels as f64 / area_scale)
            .floor()
            .clamp(signature.min_component_pixels as f64, 1_000_000.0)
            as u32;
    }
    let mut detection =
        mtc_vision::detect_markers(image, &segmentation, timestamp, now_ms(), config.max_age_ms)?;
    for marker in &mut detection.markers {
        let point = candidate.transform.map_point(Point {
            x: marker.frame_position_normalized.x * image.width() as f64,
            y: marker.frame_position_normalized.y * image.height() as f64,
        })?;
        marker.map_position = point;
        marker.source = "Widoczny kolor markera + dopasowanie geometrii lokalnej mapy".into();
    }
    detection.markers.retain(|m| {
        m.map_position.x >= 0.0
            && m.map_position.y >= 0.0
            && m.map_position.x < reference.width as f64
            && m.map_position.y < reference.height as f64
    });
    markers.enabled = true;
    markers.coordinate_calibration_confirmed = true;
    markers.map_roi = roi;
    markers.map_width = reference.width;
    markers.map_height = reference.height;
    analysis.markers = detection.markers;
    analysis.marker_warning = detection.warning;
    Ok(())
}

pub fn resolve_analysis(
    report: &mut AutomationReport,
    analysis: &mut mtc_vision::FrameAnalysis,
) -> Result<(), String> {
    let label = |kind| {
        let observations: Vec<_> = analysis
            .text_observations
            .iter()
            .filter(|o| o.kind == kind)
            .collect();
        (observations.len() == 1).then(|| mtc_profiles::OcrWeaponLabel {
            text: observations[0].text.clone(),
            confidence: observations[0].confidence,
        })
    };
    report.weapon = Some(mtc_profiles::resolve_weapon_labels(
        &mtc_profiles::all_profiles(),
        label(mtc_vision::OcrKind::VehicleName).as_ref(),
        label(mtc_vision::OcrKind::WeaponName).as_ref(),
    )?);
    if let Some(text) = label(mtc_vision::OcrKind::MapName) {
        let catalog: Vec<_> = mtc_profiles::community_maps()
            .into_iter()
            .map(|m| mtc_maps::MapNameEntry {
                id: m.id,
                name: m.name,
                aliases: vec![m.key],
            })
            .collect();
        report.map_name = Some(mtc_maps::match_map_name(&text.text, &catalog)?);
        if text.confidence >= 0.85 {
            if let (Some(named), Some(active)) = (
                report.map_name.as_ref().and_then(|m| m.map_id.as_ref()),
                report.active_map.as_ref(),
            ) {
                let geometric = if catalog.iter().any(|m| m.id == active.id) {
                    Some(active.id.clone())
                } else {
                    mtc_maps::match_map_name(&active.name, &catalog)?.map_id
                };
                if geometric.as_ref().is_some_and(|id| id != named) {
                    let warning =
                        "Jawna nazwa mapy przeczy dopasowaniu obrazu — pozycje wstrzymane";
                    report.active_map = None;
                    report.warnings.push(warning.into());
                    if let Some(map) = report.map.as_mut() {
                        map.status = mtc_maps::MapViewStatus::Ambiguous;
                        map.best = None;
                        map.warning = Some(warning.into());
                    }
                    analysis.markers.clear();
                    analysis.marker_warning = Some(warning.into());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn discovery(timestamp: u64) -> HudDiscovery {
        HudDiscovery {
            schema_version: 1,
            status: mtc_vision::HudDiscoveryStatus::Partial,
            timestamp_ms: timestamp,
            reference_width: 800,
            reference_height: 600,
            regions: vec![mtc_vision::HudRegionProposal {
                region: OcrRegionConfig {
                    id: "timer".into(),
                    kind: mtc_vision::OcrKind::Timer,
                    roi: mtc_vision::Roi {
                        x: 0.4,
                        y: 0.01,
                        width: 0.1,
                        height: 0.05,
                    },
                    upscale: 2,
                },
                confidence: 0.75,
                text: "27:42".into(),
                value: Some(1662.0),
                evidence: vec!["test".into()],
            }],
            warnings: vec![],
            elapsed_ms: 0.0,
            source: "test".into(),
        }
    }
    #[test]
    fn requires_distinct_fresh_frames_before_applying_hud() {
        let mut state = AutomationState::default();
        let mut first = discovery(1000);
        stabilize_hud(&mut first, &mut state);
        assert!(state.applied_hud.is_empty());
        let mut duplicate = discovery(1000);
        stabilize_hud(&mut duplicate, &mut state);
        assert!(state.applied_hud.is_empty());
        let mut next = discovery(1500);
        stabilize_hud(&mut next, &mut state);
        assert_eq!(state.applied_hud.len(), 1);
        assert_eq!(next.regions[0].confidence, 0.9);
        let mut stale = discovery(4000);
        stabilize_hud(&mut stale, &mut state);
        assert!(state.applied_hud.is_empty());
    }
    fn synthetic_library() -> (std::path::PathBuf, StoredMap, StoredTerrain) {
        use sha2::Digest;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mtc-assets-test-{}-{}-{}",
            std::process::id(),
            now_ms(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let texture = RgbaImage::from_fn(128, 128, |x, y| {
            let v = ((x * 37 + y * 91 + (x * y) % 53) % 251) as u8;
            image::Rgba([v, v.wrapping_mul(3), 255 - v, 255])
        });
        let height = RgbaImage::from_fn(16, 16, |x, _| {
            let v = x as u8 * 10;
            image::Rgba([v, v, v, 255])
        });
        let write = |name: &str, img: &RgbaImage| {
            let path = root.join(name);
            img.save(&path).unwrap();
            let sha = format!("{:x}", sha2::Sha256::digest(std::fs::read(&path).unwrap()));
            (path.to_string_lossy().to_string(), sha)
        };
        let (map_path, map_sha) = write("map.png", &texture);
        let (height_path, height_sha) = write("height.png", &height);
        let map = StoredMap {
            sha256: map_sha,
            id: "syntetyczna".into(),
            name: "Syntetyczna".into(),
            path: map_path,
            width: 128,
            height: 128,
            unit: "game_meters".into(),
            source: "Syntetyczny test".into(),
            confirmed: true,
            calibration: MapCalibration {
                units_per_pixel: 2.0,
                north_rotation_deg: 0.0,
                origin_pixel: Point { x: 0.0, y: 0.0 },
                origin_world: Point { x: 0.0, y: 0.0 },
                uncertainty_units: 0.0,
            },
        };
        let terrain = StoredTerrain {
            sha256: height_sha,
            path: height_path,
            map_id: "syntetyczna".into(),
            map_width: 128,
            map_height: 128,
            min_height: 0.0,
            max_height: 100.0,
            unit: "game_meters".into(),
            source: "Syntetyczny test".into(),
            confirmed: true,
            coordinate_alignment_confirmed: true,
        };
        (root, map, terrain)
    }
    #[test]
    fn lenient_library_skips_missing_file_but_keeps_other_entries() {
        let (root, map, terrain) = synthetic_library();
        let mut broken = map.clone();
        broken.id = "usunieta".into();
        broken.path = root.join("missing.png").to_string_lossy().to_string();
        let maps = vec![broken.clone(), map.clone()];
        let strict = load_assets(
            maps.clone(),
            vec![terrain.clone()],
            false,
            &ReusableAssets::default(),
        );
        assert!(strict.is_err());
        let loaded = load_assets(maps, vec![terrain], true, &ReusableAssets::default()).unwrap();
        assert_eq!(loaded.maps.len(), 1);
        assert_eq!(loaded.terrains.len(), 1);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("Syntetyczna"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn unchanged_entries_reuse_decoded_rasters_and_changed_files_are_rejected() {
        let (root, map, terrain) = synthetic_library();
        let first = load_assets(
            vec![map.clone()],
            vec![terrain.clone()],
            false,
            &ReusableAssets::default(),
        )
        .unwrap();
        let reusable = ReusableAssets::from(&first);
        let second =
            load_assets(vec![map.clone()], vec![terrain.clone()], false, &reusable).unwrap();
        assert!(Arc::ptr_eq(&first.terrains[0], &second.terrains[0]));
        // A replaced file is never served from the previous decode.
        std::fs::write(&map.path, b"not a png").unwrap();
        assert!(load_assets(vec![map], vec![terrain], false, &reusable)
            .err()
            .unwrap()
            .contains("Zmieniła się"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn location_or_resolution_change_removes_previous_regions() {
        let mut state = AutomationState::default();
        stabilize_hud(&mut discovery(1000), &mut state);
        stabilize_hud(&mut discovery(1500), &mut state);
        let mut moved = discovery(2000);
        moved.regions[0].region.roi.x = 0.8;
        stabilize_hud(&mut moved, &mut state);
        assert!(state.applied_hud.is_empty());
        let mut resized = discovery(2500);
        resized.reference_width = 1000;
        stabilize_hud(&mut resized, &mut state);
        assert!(state.applied_hud.is_empty());
    }
}
