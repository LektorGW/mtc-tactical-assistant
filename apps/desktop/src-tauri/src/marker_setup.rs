use crate::{image_data, now_ms, Runtime};
use image::RgbaImage;
use mtc_capture::CaptureBackend;
use mtc_maps::{MapCalibration, Point, SimilarityTransform};
use mtc_vision::{
    AnalysisConfig, MarkerCalibrationStatus, MarkerConfig, MarkerObservation, MarkerProposalStatus,
    MarkerRole, MarkerSelectionConfig, MarkerSelectionHint, MarkerSelectionResult,
    MarkerSelectionState, MarkerSelectionStatus, MarkerSignature, MarkerSignatureProposal, Roi,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeMarkerSelection {
    pub hint: MarkerSelectionHint,
    pub handle: i64,
    pub backend: CaptureBackend,
    pub map_id: Option<String>,
    pub map_roi: Roi,
    pub calibration: Option<MapCalibration>,
    pub capture_epoch: u64,
}

pub struct MarkerSelectionFrameContext {
    pub handle: i64,
    pub backend: CaptureBackend,
    pub auto_map: bool,
    pub map_id: Option<String>,
    pub calibration: Option<MapCalibration>,
    pub frame_width: u32,
    pub frame_height: u32,
    pub timestamp_ms: u64,
    pub now_ms: u64,
    pub capture_epoch: u64,
    pub max_speed_pixels_per_second: f64,
}

#[derive(Clone, PartialEq)]
struct SourceKey {
    handle: i64,
    backend: CaptureBackend,
    epoch: u64,
    frame_width: u32,
    frame_height: u32,
    map_width: u32,
    map_height: u32,
    auto_map: bool,
    map_id: Option<String>,
    map_sha256: Option<String>,
    manual_roi: Option<Roi>,
    manual_view: Option<String>,
    scope: String,
}

#[derive(Clone)]
struct Geometry {
    map_roi: Roi,
    map_width: u32,
    map_height: u32,
    map_id: Option<String>,
    map_sha256: Option<String>,
    calibration: Option<MapCalibration>,
    transform: Option<SimilarityTransform>,
    area_scale: f64,
}

struct SetupFrame {
    token: String,
    image: RgbaImage,
    timestamp_ms: u64,
    source: SourceKey,
    config: AnalysisConfig,
    geometry: Geometry,
}

struct ApprovedSelection {
    selection: NativeMarkerSelection,
    signature: MarkerSignature,
    source: SourceKey,
    bound_calibration: Option<MapCalibration>,
}

/// One transient source image and at most two explicit approvals. No file writes.
#[derive(Default)]
pub struct MarkerSetupState {
    frame: Option<SetupFrame>,
    approved: Vec<ApprovedSelection>,
    tracks: HashMap<String, MarkerSelectionState>,
}

impl MarkerSetupState {
    pub fn clear(&mut self) {
        self.frame = None;
        self.approved.clear();
        self.tracks.clear();
    }

    fn replace_frame(&mut self, frame: SetupFrame) {
        if self.approved.iter().any(|a| a.source != frame.source) {
            self.approved.clear();
            self.tracks.clear();
        }
        self.frame = Some(frame);
    }

    fn approve(
        &mut self,
        selection: NativeMarkerSelection,
        signature: MarkerSignature,
        source: SourceKey,
    ) {
        let removed: Vec<_> = self
            .approved
            .iter()
            .filter(|a| a.selection.hint.role == selection.hint.role)
            .map(|a| a.selection.hint.selection_id.clone())
            .collect();
        for id in removed {
            self.tracks.remove(&id);
        }
        self.approved
            .retain(|a| a.selection.hint.role != selection.hint.role);
        self.approved.push(ApprovedSelection {
            bound_calibration: selection.calibration,
            selection,
            signature,
            source,
        });
    }

    /// IDs/roles of a color class may be coalesced, but source/point/role and RGB
    /// remain bound to the native approval. Shape expansion is bounded by the
    /// union of explicitly calibrated components having that exact RGB.
    pub fn validate_selection(
        &self,
        selection: &NativeMarkerSelection,
        signature: &MarkerSignature,
    ) -> Result<(), String> {
        signature.validate()?;
        let approved = self
            .approved
            .iter()
            .find(|a| a.selection.hint.selection_id == selection.hint.selection_id)
            .ok_or("Wybór markera nie ma zatwierdzenia w tej sesji; wskaż ikonę ponownie")?;
        let mut normalized = selection.clone();
        normalized.hint.signature_id = approved.selection.hint.signature_id.clone();
        if serde_json::to_value(normalized).map_err(|e| e.to_string())?
            != serde_json::to_value(&approved.selection).map_err(|e| e.to_string())?
        {
            return Err("Metadane wyboru zmieniły się po kliknięciu widocznej ikony".into());
        }
        if signature.id != selection.hint.signature_id || signature.rgb != approved.signature.rgb {
            return Err("Kolor lub identyfikator nie odpowiada jawnie wskazanej ikonie".into());
        }
        let group: Vec<_> = self
            .approved
            .iter()
            .filter(|a| a.signature.rgb == approved.signature.rgb)
            .collect();
        let tolerance = group.iter().map(|a| a.signature.tolerance).max().unwrap();
        let min_area = group
            .iter()
            .map(|a| a.signature.min_component_pixels)
            .min()
            .unwrap();
        let max_area = group
            .iter()
            .map(|a| a.signature.max_component_pixels)
            .max()
            .unwrap();
        let min_aspect = group
            .iter()
            .map(|a| a.signature.min_aspect_ratio)
            .fold(f64::INFINITY, f64::min);
        let max_aspect = group
            .iter()
            .map(|a| a.signature.max_aspect_ratio)
            .fold(0.0, f64::max);
        let min_fill = group
            .iter()
            .map(|a| a.signature.min_fill_ratio)
            .fold(1.0, f64::min);
        if signature.tolerance > tolerance
            || signature.min_component_pixels < min_area
            || signature.max_component_pixels > max_area
            || signature.min_aspect_ratio < min_aspect
            || signature.max_aspect_ratio > max_aspect
            || signature.min_fill_ratio < min_fill
        {
            return Err("Progi wzorca rozszerzono poza zatwierdzone widoczne komponenty".into());
        }
        Ok(())
    }

    pub fn apply_selections(
        &mut self,
        selections: &[NativeMarkerSelection],
        signature_config: &MarkerConfig,
        candidates: &[MarkerObservation],
        context: &MarkerSelectionFrameContext,
    ) -> Result<Vec<MarkerSelectionResult>, String> {
        signature_config.validate()?;
        if !signature_config.enabled || !signature_config.coordinate_calibration_confirmed {
            // Disabled markers drop the tracks. A frame in which the automatic map
            // match failed (e.g. a matcher timeout under load) only blocks this frame:
            // the tracks stay and are reused only within their 2 s freshness window.
            if !signature_config.enabled {
                self.tracks.clear();
            }
            return Ok(selections
                .iter()
                .map(|_| {
                    blocked_selection(
                        context.timestamp_ms,
                        "Automatyczne pozycje markerów nie są włączone i potwierdzone".into(),
                    )
                })
                .collect());
        }
        let mut roles = HashSet::new();
        let mut ids = HashSet::new();
        if selections.len() > 2
            || selections.iter().any(|s| {
                !matches!(s.hint.role, MarkerRole::Own | MarkerRole::Target)
                    || !roles.insert(s.hint.role as u8)
                    || !ids.insert(s.hint.selection_id.clone())
            })
        {
            self.tracks.clear();
            return Err("Można zatwierdzić najwyżej jedną własną ikonę i jeden cel".into());
        }
        self.tracks.retain(|id, _| ids.contains(id));
        let mut results = Vec::new();
        for selection in selections {
            let id = &selection.hint.selection_id;
            let apply = (|| {
                let signature = signature_config
                    .signatures
                    .iter()
                    .find(|s| s.id == selection.hint.signature_id)
                    .ok_or("Brak zatwierdzonej sygnatury dla wskazanej ikony")?;
                self.validate_selection(selection, signature)?;
                let approved = self
                    .approved
                    .iter_mut()
                    .find(|a| a.selection.hint.selection_id == *id)
                    .unwrap();
                if selection.handle != context.handle
                    || selection.backend != context.backend
                    || selection.capture_epoch != context.capture_epoch
                    || approved.source.auto_map != context.auto_map
                    || selection.map_id != context.map_id
                    || selection.hint.frame_width != context.frame_width
                    || selection.hint.frame_height != context.frame_height
                    || selection.hint.map_width != signature_config.map_width
                    || selection.hint.map_height != signature_config.map_height
                {
                    return Err("Zmieniło się okno, epoka, mapa lub geometria wyboru".into());
                }
                if !approved.source.auto_map
                    && (approved.source.manual_roi != Some(signature_config.map_roi)
                        || approved.source.manual_view
                            != signature_config
                                .view_signature
                                .as_ref()
                                .map(serde_json::to_string)
                                .transpose()
                                .map_err(|e| e.to_string())?
                        || !signature_config.coordinate_calibration_confirmed)
                {
                    return Err(
                        "Ręczny ROI lub deskryptor mapy zmienił się; wskaż ikonę ponownie".into(),
                    );
                }
                if let Some(bound) = approved.bound_calibration {
                    if !calibration_equal(Some(bound), context.calibration)? {
                        return Err("Kalibracja mapy zmieniła się po potwierdzeniu ikony".into());
                    }
                } else if let Some(calibration) = context.calibration {
                    calibration.validate()?;
                    approved.bound_calibration = Some(calibration);
                }
                let config = MarkerSelectionConfig {
                    scope: selection.hint.scope.clone(),
                    frame_width: context.frame_width,
                    frame_height: context.frame_height,
                    map_width: signature_config.map_width,
                    map_height: signature_config.map_height,
                    max_age_ms: 2000,
                    initial_radius_pixels: 12.0,
                    base_gate_pixels: 3.0,
                    max_association_radius_pixels: 48.0,
                    max_speed_pixels_per_second: context.max_speed_pixels_per_second,
                    ambiguity_margin_pixels: 2.0,
                    min_confidence: 0.6,
                };
                mtc_vision::select_confirmed_marker(
                    candidates,
                    &selection.hint,
                    self.tracks.get(id),
                    context.timestamp_ms,
                    context.now_ms,
                    &config,
                )
            })();
            let result = match apply {
                Ok(result) => result,
                Err(reason) => blocked_selection(context.timestamp_ms, reason),
            };
            if let Some(state) = &result.state {
                self.tracks.insert(id.clone(), state.clone());
            } else {
                self.tracks.remove(id);
            }
            results.push(result);
        }
        if results.len() == 2 {
            let collision = results[0]
                .selected
                .as_ref()
                .zip(results[1].selected.as_ref())
                .is_some_and(|(a, b)| {
                    a.signature_id == b.signature_id
                        && a.timestamp_ms == b.timestamp_ms
                        && ((a.map_position.x - b.map_position.x)
                            .hypot(a.map_position.y - b.map_position.y)
                            < 1e-6
                            || component_overlap(a.bounding_box, b.bounding_box) >= 0.8)
                });
            if collision {
                for (result, selection) in results.iter_mut().zip(selections) {
                    self.tracks.remove(&selection.hint.selection_id);
                    *result = blocked_selection(context.timestamp_ms,
                        "Obie role wskazują ten sam widoczny komponent; potwierdź różne ikony ponownie".into());
                    result.status = MarkerSelectionStatus::Ambiguous;
                }
            }
        }
        Ok(results)
    }
}

fn blocked_selection(timestamp_ms: u64, reason: String) -> MarkerSelectionResult {
    MarkerSelectionResult {
        status: MarkerSelectionStatus::Blocked,
        selected: None,
        track_id: None,
        count: 0,
        confidence: 0.0,
        timestamp_ms,
        state: None,
        warning: Some(reason),
    }
}

fn calibration_equal(a: Option<MapCalibration>, b: Option<MapCalibration>) -> Result<bool, String> {
    Ok(serde_json::to_value(a).map_err(|e| e.to_string())?
        == serde_json::to_value(b).map_err(|e| e.to_string())?)
}

fn component_overlap(a: Roi, b: Roi) -> f64 {
    let width = ((a.x + a.width).min(b.x + b.width) - a.x.max(b.x)).max(0.0);
    let height = ((a.y + a.height).min(b.y + b.height) - a.y.max(b.y)).max(0.0);
    let intersection = width * height;
    intersection / (a.width * a.height + b.width * b.height - intersection).max(1e-12)
}

fn require_fresh(
    timestamp: u64,
    now: u64,
    expected_epoch: u64,
    current_epoch: u64,
) -> Result<(), String> {
    if expected_epoch != current_epoch {
        return Err("Źródło przechwytywania zmieniło się; odśwież podgląd markerów".into());
    }
    if timestamp > now || now - timestamp > 2000 {
        return Err("Podgląd markerów stracił świeżość; odśwież go przed kliknięciem".into());
    }
    Ok(())
}

fn resolve_geometry(
    image: &RgbaImage,
    config: &AnalysisConfig,
    auto_map: bool,
    preferred_map: Option<&str>,
    assets: &crate::automation::AutomationAssets,
) -> Result<Geometry, String> {
    config.validate()?;
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Nieobsługiwany rozmiar podglądu markerów".into());
    }
    let actual = f64::from(image.width()) / f64::from(image.height());
    let reference = f64::from(config.reference_width) / f64::from(config.reference_height);
    if (actual / reference - 1.0).abs() > config.aspect_tolerance {
        return Err("Geometria źródła nie odpowiada konfiguracji podglądu".into());
    }
    if auto_map {
        let references: Vec<_> = assets
            .references
            .iter()
            .filter(|r| preferred_map.is_none_or(|id| r.metadata.id == id))
            .cloned()
            .collect();
        let matched =
            mtc_maps::match_map_view(image, &references, &mtc_maps::MapMatchConfig::default())?;
        if matched.status != mtc_maps::MapViewStatus::Matched {
            return Err(matched
                .warning
                .unwrap_or("Brak jednoznacznego dopasowania mapy do biblioteki".into()));
        }
        let candidate = matched
            .best
            .ok_or("Dopasowanie mapy nie zawiera geometrii")?;
        let map = assets
            .maps
            .iter()
            .find(|m| m.id == candidate.map_id)
            .ok_or("Brak rozpoznanej mapy w bibliotece")?;
        if !map.confirmed
            || map.width != candidate.reference_width
            || map.height != candidate.reference_height
        {
            return Err("Niepotwierdzona lub niespójna referencja mapy".into());
        }
        let area = candidate
            .projected_map_bounds
            .intersect(mtc_maps::PixelRect {
                x: 0.0,
                y: 0.0,
                width: f64::from(image.width()),
                height: f64::from(image.height()),
            })
            .ok_or("Mapa nie leży w widocznej klatce")?;
        Ok(Geometry {
            map_roi: Roi {
                x: area.x / f64::from(image.width()),
                y: area.y / f64::from(image.height()),
                width: area.width / f64::from(image.width()),
                height: area.height / f64::from(image.height()),
            },
            map_width: map.width,
            map_height: map.height,
            map_id: Some(map.id.clone()),
            map_sha256: Some(map.sha256.clone()),
            calibration: Some(map.calibration),
            area_scale: candidate.transform.scale.powi(2),
            transform: Some(candidate.transform),
        })
    } else {
        let markers = config
            .marker_config
            .as_ref()
            .ok_or("Brak ręcznego ROI mapy")?;
        if !markers.coordinate_calibration_confirmed {
            return Err("Potwierdź pełny skalibrowany widok mapy".into());
        }
        let signature = markers
            .view_signature
            .as_ref()
            .ok_or("Zapamiętaj widok pełnej mapy przed kalibracją markerów")?;
        let matched = mtc_vision::match_view_signature(image, markers.map_roi, signature)?;
        if !matched.matched {
            return Err(matched
                .warning
                .unwrap_or("Bieżący obraz nie pasuje do potwierdzonej mapy".into()));
        }
        let (_, _, crop_width, crop_height) =
            markers.map_roi.pixel_rect(image.width(), image.height())?;
        let sx = f64::from(markers.map_width) / f64::from(crop_width);
        let sy = f64::from(markers.map_height) / f64::from(crop_height);
        if (sx / sy - 1.0).abs() > markers.aspect_tolerance.min(0.03) {
            return Err("Proporcje ROI nie odpowiadają jednolitej skali mapy referencyjnej".into());
        }
        let stored = if let Some(id) = preferred_map {
            Some(
                assets
                    .maps
                    .iter()
                    .find(|m| {
                        m.id == id
                            && m.confirmed
                            && m.width == markers.map_width
                            && m.height == markers.map_height
                    })
                    .ok_or("Wybrana mapa nie odpowiada geometrii ręcznego ROI")?,
            )
        } else {
            None
        };
        Ok(Geometry {
            map_roi: markers.map_roi,
            map_width: markers.map_width,
            map_height: markers.map_height,
            map_id: stored.map(|m| m.id.clone()),
            map_sha256: stored.map(|m| m.sha256.clone()),
            calibration: stored.map(|m| m.calibration),
            transform: None,
            area_scale: sx * sy,
        })
    }
}

fn source_key(
    identity: (i64, CaptureBackend, u64, String),
    image: &RgbaImage,
    config: &AnalysisConfig,
    geometry: &Geometry,
    auto_map: bool,
) -> Result<SourceKey, String> {
    let (handle, backend, epoch, scope) = identity;
    Ok(SourceKey {
        handle,
        backend,
        epoch,
        frame_width: image.width(),
        frame_height: image.height(),
        map_width: geometry.map_width,
        map_height: geometry.map_height,
        auto_map,
        map_id: geometry.map_id.clone(),
        map_sha256: geometry.map_sha256.clone(),
        manual_roi: (!auto_map).then_some(geometry.map_roi),
        manual_view: if auto_map {
            None
        } else {
            config
                .marker_config
                .as_ref()
                .and_then(|m| m.view_signature.as_ref())
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| e.to_string())?
        },
        scope,
    })
}

fn reference_point(
    image: &RgbaImage,
    geometry: &Geometry,
    normalized: Point,
) -> Result<Point, String> {
    let px = normalized.x * f64::from(image.width());
    let py = normalized.y * f64::from(image.height());
    let point = if let Some(transform) = geometry.transform {
        transform.map_point(Point { x: px, y: py })?
    } else {
        // Same pixel rectangle as detect_markers, without copying the map crop.
        let (x, y, width, height) = geometry.map_roi.pixel_rect(image.width(), image.height())?;
        Point {
            x: (px - f64::from(x)) / f64::from(width) * f64::from(geometry.map_width),
            y: (py - f64::from(y)) / f64::from(height) * f64::from(geometry.map_height),
        }
    };
    if !point.valid()
        || point.x < 0.0
        || point.y < 0.0
        || point.x >= f64::from(geometry.map_width)
        || point.y >= f64::from(geometry.map_height)
    {
        return Err("Wskazany komponent leży poza mapą referencyjną".into());
    }
    Ok(point)
}

fn scale_signature(signature: &mut MarkerSignature, scale: f64) -> Result<(), String> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Niepoprawna skala powierzchni markera".into());
    }
    let min = (f64::from(signature.min_component_pixels) * scale)
        .ceil()
        .max(1.0);
    let max = (f64::from(signature.max_component_pixels) * scale)
        .floor()
        .max(min);
    if max > 1_000_000.0 {
        return Err("Przeskalowany marker przekracza limit powierzchni".into());
    }
    signature.min_component_pixels = min as u32;
    signature.max_component_pixels = max as u32;
    signature.validate()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSetupPreview {
    token: String,
    preview: String,
    width: u32,
    height: u32,
    preview_width: u32,
    preview_height: u32,
    timestamp_ms: u64,
    map_roi: Roi,
    map_id: Option<String>,
    reference_map_width: u32,
    reference_map_height: u32,
    observed_map_width: u32,
    observed_map_height: u32,
    proposals: Vec<MarkerSignatureProposal>,
    status: MarkerProposalStatus,
    warnings: Vec<String>,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn capture_marker_setup(
    app: tauri::AppHandle,
    handle: i64,
    backend: CaptureBackend,
    mut config: AnalysisConfig,
    auto_map: bool,
    map_id: Option<String>,
    scope: String,
) -> Result<MarkerSetupPreview, String> {
    if scope.trim().is_empty() || scope.len() > 8192 {
        return Err("Niepoprawny identyfikator źródła konfiguracji markerów".into());
    }
    let epoch = app.state::<Runtime>().capture_epoch.load(Ordering::Acquire);
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        loop {
            crate::require_epoch(&app.state::<Runtime>(), epoch)?;
            {
                let state = app.state::<Runtime>();
                let mut busy = state.capture_busy.lock().map_err(|e| e.to_string())?;
                if !*busy {
                    *busy = true;
                    break;
                }
            }
            if started.elapsed() > std::time::Duration::from_millis(800) {
                return Err(
                    "Trwa analiza klatki; odśwież podgląd markerów po jej zakończeniu".into(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _busy = crate::BusyGuard(app.clone());
        let state = app.state::<Runtime>();
        let _lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
        crate::require_epoch(&state, epoch)?;
        let captured =
            mtc_capture::capture_window_with_backend(handle, backend).map_err(|e| e.to_string())?;
        require_fresh(
            captured.captured_at_ms,
            now_ms(),
            epoch,
            state.capture_epoch.load(Ordering::Acquire),
        )?;
        if config.reference_width == 0 && config.reference_height == 0 {
            config.reference_width = captured.image.width();
            config.reference_height = captured.image.height();
        }
        let geometry = {
            let assets = state.assets.lock().map_err(|e| e.to_string())?;
            resolve_geometry(
                &captured.image,
                &config,
                auto_map,
                map_id.as_deref(),
                &assets,
            )?
        };
        let report = mtc_vision::propose_marker_signatures(
            &captured.image,
            geometry.map_roi,
            captured.captured_at_ms,
            now_ms(),
        )?;
        let thumbnail = mtc_vision::preview_thumbnail(&captured.image, 1440, 900);
        let preview = image_data(&thumbnail)?;
        require_fresh(
            captured.captured_at_ms,
            now_ms(),
            epoch,
            state.capture_epoch.load(Ordering::Acquire),
        )?;
        let token = format!(
            "{:x}-{:x}-{:x}",
            epoch,
            captured.captured_at_ms,
            NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
        );
        let source = source_key(
            (handle, backend, epoch, scope),
            &captured.image,
            &config,
            &geometry,
            auto_map,
        )?;
        let output = MarkerSetupPreview {
            token: token.clone(),
            preview,
            width: captured.image.width(),
            height: captured.image.height(),
            preview_width: thumbnail.width(),
            preview_height: thumbnail.height(),
            timestamp_ms: captured.captured_at_ms,
            map_roi: geometry.map_roi,
            map_id: geometry.map_id.clone(),
            reference_map_width: geometry.map_width,
            reference_map_height: geometry.map_height,
            observed_map_width: report.observed_map_width,
            observed_map_height: report.observed_map_height,
            proposals: report.proposals,
            status: report.status,
            warnings: report.warnings,
        };
        state
            .marker_setup
            .lock()
            .map_err(|e| e.to_string())?
            .replace_frame(SetupFrame {
                token,
                image: captured.image,
                timestamp_ms: captured.captured_at_ms,
                source,
                config,
                geometry,
            });
        Ok(output)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleMarkerCalibration {
    signature: MarkerSignature,
    selection: NativeMarkerSelection,
    preview: String,
    width: u32,
    height: u32,
    preview_width: u32,
    preview_height: u32,
    timestamp_ms: u64,
    proposal: MarkerSignatureProposal,
}

#[tauri::command]
pub async fn calibrate_visible_marker(
    app: tauri::AppHandle,
    token: String,
    click: Point,
    role: MarkerRole,
) -> Result<VisibleMarkerCalibration, String> {
    if !matches!(role, MarkerRole::Own | MarkerRole::Target) {
        return Err("Wybierz jawnie własną ikonę lub cel".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Runtime>();
        let _lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
        let mut setup = state.marker_setup.lock().map_err(|e| e.to_string())?;
        let frame = setup
            .frame
            .as_ref()
            .ok_or("Odśwież podgląd przed kliknięciem markera")?;
        if frame.token != token {
            return Err("Podgląd został zastąpiony nowszą klatką".into());
        }
        require_fresh(
            frame.timestamp_ms,
            now_ms(),
            frame.source.epoch,
            state.capture_epoch.load(Ordering::Acquire),
        )?;
        let geometry = {
            let assets = state.assets.lock().map_err(|e| e.to_string())?;
            resolve_geometry(
                &frame.image,
                &frame.config,
                frame.source.auto_map,
                frame.geometry.map_id.as_deref(),
                &assets,
            )?
        };
        if geometry.map_id != frame.geometry.map_id
            || geometry.map_sha256 != frame.geometry.map_sha256
            || geometry.map_width != frame.geometry.map_width
            || geometry.map_height != frame.geometry.map_height
            || !calibration_equal(geometry.calibration, frame.geometry.calibration)?
        {
            return Err("Referencja mapy zmieniła się; odśwież podgląd".into());
        }
        let calibrated = mtc_vision::calibrate_marker_signature(
            &frame.image,
            frame.geometry.map_roi,
            click,
            frame.timestamp_ms,
            now_ms(),
        )?;
        if calibrated.status != MarkerCalibrationStatus::Calibrated {
            return Err(calibrated.warnings.join("; "));
        }
        let proposal = calibrated
            .proposal
            .ok_or("Nie znaleziono jednoznacznego widocznego komponentu")?;
        let point = reference_point(
            &frame.image,
            &frame.geometry,
            proposal.frame_position_normalized,
        )?;
        if setup.approved.iter().any(|a| {
            a.selection.hint.role != role
                && a.selection.hint.timestamp_ms == frame.timestamp_ms
                && a.selection.hint.confirmed_point == point
        }) {
            return Err("Ta sama widoczna ikona została już wskazana dla drugiej roli".into());
        }
        let role_id = if role == MarkerRole::Own {
            "own"
        } else {
            "target"
        };
        let mut signature = proposal.signature.clone();
        signature.id = format!("marker-{token}-{role_id}");
        signature.role = MarkerRole::Unassigned;
        scale_signature(&mut signature, frame.geometry.area_scale)?;
        let selection = NativeMarkerSelection {
            hint: MarkerSelectionHint {
                selection_id: format!("selection-{token}-{role_id}"),
                signature_id: signature.id.clone(),
                role,
                confirmed_point: point,
                timestamp_ms: frame.timestamp_ms,
                scope: format!("marker-setup:{token}"),
                frame_width: frame.image.width(),
                frame_height: frame.image.height(),
                map_width: frame.geometry.map_width,
                map_height: frame.geometry.map_height,
            },
            handle: frame.source.handle,
            backend: frame.source.backend,
            map_id: frame.geometry.map_id.clone(),
            map_roi: frame.geometry.map_roi,
            calibration: frame.geometry.calibration,
            capture_epoch: frame.source.epoch,
        };
        let thumbnail = mtc_vision::preview_thumbnail(&frame.image, 1440, 900);
        let preview = image_data(&thumbnail)?;
        let output = VisibleMarkerCalibration {
            signature: signature.clone(),
            selection: selection.clone(),
            preview,
            width: frame.image.width(),
            height: frame.image.height(),
            preview_width: thumbnail.width(),
            preview_height: thumbnail.height(),
            timestamp_ms: frame.timestamp_ms,
            proposal,
        };
        require_fresh(
            frame.timestamp_ms,
            now_ms(),
            frame.source.epoch,
            state.capture_epoch.load(Ordering::Acquire),
        )?;
        let source = frame.source.clone();
        setup.approve(selection, signature, source);
        Ok(output)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn image() -> RgbaImage {
        RgbaImage::from_fn(160, 120, |x, y| {
            let gray = 40 + ((x / 4 * 37 + y / 4 * 53 + x / 4 * (y / 4) * 11) % 100) as u8;
            Rgba([gray, gray, gray, 255])
        })
    }
    fn signature() -> MarkerSignature {
        MarkerSignature {
            id: "shared".into(),
            role: MarkerRole::Unassigned,
            rgb: [240, 30, 30],
            tolerance: 12,
            min_component_pixels: 3,
            max_component_pixels: 200,
            min_aspect_ratio: 0.25,
            max_aspect_ratio: 4.0,
            min_fill_ratio: 0.1,
        }
    }
    fn config() -> AnalysisConfig {
        let image = image();
        let roi = Roi {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        AnalysisConfig {
            schema_version: 1,
            reference_width: 160,
            reference_height: 120,
            aspect_tolerance: 0.03,
            max_age_ms: 2000,
            ocr_regions: vec![],
            marker_config: Some(MarkerConfig {
                enabled: true,
                map_roi: roi,
                map_width: 320,
                map_height: 240,
                coordinate_calibration_confirmed: true,
                view_signature: Some(mtc_vision::calibrate_view_signature(&image, roi).unwrap()),
                aspect_tolerance: 0.03,
                signatures: vec![signature()],
            }),
        }
    }
    fn geometry() -> Geometry {
        resolve_geometry(
            &image(),
            &config(),
            false,
            None,
            &crate::automation::AutomationAssets::default(),
        )
        .unwrap()
    }
    fn source() -> SourceKey {
        source_key(
            (17, CaptureBackend::Gdi, 7, "source".into()),
            &image(),
            &config(),
            &geometry(),
            false,
        )
        .unwrap()
    }
    fn selection(role: MarkerRole, point: f64, time: u64) -> NativeMarkerSelection {
        let id = if role == MarkerRole::Own {
            "own-click"
        } else {
            "target-click"
        };
        NativeMarkerSelection {
            hint: MarkerSelectionHint {
                selection_id: id.into(),
                signature_id: "shared".into(),
                role,
                confirmed_point: Point { x: point, y: 100.0 },
                timestamp_ms: time,
                scope: format!("native-{id}"),
                frame_width: 160,
                frame_height: 120,
                map_width: 320,
                map_height: 240,
            },
            handle: 17,
            backend: CaptureBackend::Gdi,
            map_id: None,
            map_roi: config().marker_config.unwrap().map_roi,
            calibration: None,
            capture_epoch: 7,
        }
    }
    fn context(time: u64) -> MarkerSelectionFrameContext {
        MarkerSelectionFrameContext {
            handle: 17,
            backend: CaptureBackend::Gdi,
            auto_map: false,
            map_id: None,
            calibration: None,
            frame_width: 160,
            frame_height: 120,
            timestamp_ms: time,
            now_ms: time + 100,
            capture_epoch: 7,
            max_speed_pixels_per_second: 100.0,
        }
    }
    fn candidate(x: f64, time: u64) -> MarkerObservation {
        MarkerObservation {
            signature_id: "shared".into(),
            role: MarkerRole::Unassigned,
            map_position: Point { x, y: 100.0 },
            frame_position_normalized: Point {
                x: x / 320.0,
                y: 100.0 / 240.0,
            },
            bounding_box: Roi {
                x: x / 320.0 - 0.01,
                y: 100.0 / 240.0 - 0.01,
                width: 0.02,
                height: 0.02,
            },
            pixel_count: 25,
            confidence: 0.8,
            timestamp_ms: time,
            source: "Syntetyczna widoczna ikona".into(),
        }
    }
    fn approve(state: &mut MarkerSetupState, selection: &NativeMarkerSelection) {
        state.approve(selection.clone(), signature(), source());
    }

    #[test]
    fn source_freshness_never_restamps_stale_future_or_invalid_epoch_preview() {
        assert!(require_fresh(1000, 1100, 7, 7).is_ok());
        for (time, now, epoch) in [(1000, 4000, 7), (1200, 1100, 7), (1000, 1100, 8)] {
            assert!(require_fresh(time, now, 7, epoch).is_err());
        }
    }
    #[test]
    fn manual_pixel_geometry_requires_confirmed_matching_view_and_uniform_reference_scale() {
        let image = image();
        let mut c = config();
        let g = resolve_geometry(&image, &c, false, None, &Default::default()).unwrap();
        let p = reference_point(&image, &g, Point { x: 0.5, y: 0.5 }).unwrap();
        assert_eq!(p, Point { x: 160.0, y: 120.0 });
        assert_eq!(g.area_scale, 4.0);
        c.marker_config.as_mut().unwrap().map_height = 200;
        assert!(resolve_geometry(&image, &c, false, None, &Default::default()).is_err());
        c = config();
        c.marker_config
            .as_mut()
            .unwrap()
            .coordinate_calibration_confirmed = false;
        assert!(resolve_geometry(&image, &c, false, None, &Default::default()).is_err());
        c = config();
        c.marker_config.as_mut().unwrap().view_signature = None;
        assert!(resolve_geometry(&image, &c, false, None, &Default::default()).is_err());
        assert!(resolve_geometry(
            &RgbaImage::new(160, 120),
            &config(),
            false,
            None,
            &Default::default()
        )
        .is_err());
    }
    #[test]
    fn scales_signature_area_without_changing_role_color_or_inventing_world_position() {
        let mut s = signature();
        scale_signature(&mut s, 4.0).unwrap();
        assert_eq!((s.min_component_pixels, s.max_component_pixels), (12, 800));
        assert_eq!(s.role, MarkerRole::Unassigned);
        assert_eq!(s.rgb, [240, 30, 30]);
        assert!(scale_signature(&mut s, f64::NAN).is_err());
        assert!(scale_signature(&mut s, 1_000_000.0).is_err());
    }
    #[test]
    fn approvals_accept_color_id_coalescing_but_reject_tampered_metadata_rgb_and_overwide_shapes() {
        let mut state = MarkerSetupState::default();
        let own = selection(MarkerRole::Own, 100.0, 1000);
        approve(&mut state, &own);
        let mut renamed = own.clone();
        renamed.hint.signature_id = "coalesced".into();
        let mut sig = signature();
        sig.id = "coalesced".into();
        state.validate_selection(&renamed, &sig).unwrap();
        for scenario in 0..4 {
            let mut altered = renamed.clone();
            let mut signature = sig.clone();
            match scenario {
                0 => altered.handle = 18,
                1 => altered.hint.confirmed_point.x += 1.0,
                2 => signature.rgb = [0, 240, 0],
                _ => signature.max_component_pixels += 1,
            }
            assert!(state.validate_selection(&altered, &signature).is_err());
        }
        state.clear();
        assert!(state.validate_selection(&renamed, &sig).is_err());
    }
    #[test]
    fn adding_second_role_and_normalizing_capture_references_preserves_fresh_own_track() {
        let mut state = MarkerSetupState::default();
        let own = selection(MarkerRole::Own, 100.0, 1000);
        approve(&mut state, &own);
        let cfg = config().marker_config.unwrap();
        let first = state
            .apply_selections(
                std::slice::from_ref(&own),
                &cfg,
                &[candidate(100.0, 1000)],
                &context(1000),
            )
            .unwrap();
        assert_eq!(first[0].status, MarkerSelectionStatus::Collecting);
        let second = state
            .apply_selections(
                std::slice::from_ref(&own),
                &cfg,
                &[candidate(105.0, 1500)],
                &context(1500),
            )
            .unwrap();
        assert_eq!(second[0].status, MarkerSelectionStatus::Ready);
        let mut normalized = config();
        normalized.reference_width = 0;
        normalized.reference_height = 0;
        let source_zero = source_key(
            (17, CaptureBackend::Gdi, 7, "source".into()),
            &image(),
            &normalized,
            &geometry(),
            false,
        )
        .unwrap();
        assert!(source_zero == source());
        state.replace_frame(SetupFrame {
            token: "refresh".into(),
            image: image(),
            timestamp_ms: 1600,
            source: source_zero,
            config: config(),
            geometry: geometry(),
        });
        let target = selection(MarkerRole::Target, 200.0, 1600);
        approve(&mut state, &target);
        let both = state
            .apply_selections(
                &[own, target],
                &cfg,
                &[candidate(107.0, 1700), candidate(200.0, 1700)],
                &context(1700),
            )
            .unwrap();
        assert_eq!(both[0].status, MarkerSelectionStatus::Ready);
        assert_eq!(both[0].count, 3);
        assert_eq!(both[1].status, MarkerSelectionStatus::Collecting);
    }
    #[test]
    fn map_backend_epoch_frame_or_calibration_change_blocks_old_selection() {
        for scenario in 0..6 {
            let mut state = MarkerSetupState::default();
            let own = selection(MarkerRole::Own, 100.0, 1000);
            approve(&mut state, &own);
            let cfg = config().marker_config.unwrap();
            let mut first = context(1000);
            first.calibration = Some(MapCalibration {
                units_per_pixel: 1.0,
                north_rotation_deg: 0.0,
                origin_pixel: Point { x: 0.0, y: 0.0 },
                origin_world: Point { x: 0.0, y: 0.0 },
                uncertainty_units: 0.0,
            });
            state
                .apply_selections(
                    std::slice::from_ref(&own),
                    &cfg,
                    &[candidate(100.0, 1000)],
                    &first,
                )
                .unwrap();
            let mut changed = context(1500);
            changed.calibration = first.calibration;
            match scenario {
                0 => changed.backend = CaptureBackend::WindowsGraphicsCapture,
                1 => changed.capture_epoch = 8,
                2 => changed.frame_width = 320,
                3 => changed.map_id = Some("another-map".into()),
                4 => changed.auto_map = true,
                _ => changed.calibration.as_mut().unwrap().units_per_pixel = 2.0,
            }
            let result = state
                .apply_selections(
                    std::slice::from_ref(&own),
                    &cfg,
                    &[candidate(100.0, 1500)],
                    &changed,
                )
                .unwrap();
            assert_eq!(result[0].status, MarkerSelectionStatus::Blocked);
            assert!(result[0].selected.is_none());
            assert!(state.tracks.is_empty());
        }
    }
    #[test]
    fn one_frame_without_map_confirmation_keeps_fresh_tracks_but_disabling_clears_them() {
        let mut state = MarkerSetupState::default();
        let own = selection(MarkerRole::Own, 100.0, 1000);
        approve(&mut state, &own);
        let cfg = config().marker_config.unwrap();
        for (x, time) in [(100.0, 1000), (105.0, 1500)] {
            state
                .apply_selections(
                    std::slice::from_ref(&own),
                    &cfg,
                    &[candidate(x, time)],
                    &context(time),
                )
                .unwrap();
        }
        let mut unmatched = cfg.clone();
        unmatched.coordinate_calibration_confirmed = false;
        let blocked = state
            .apply_selections(
                std::slice::from_ref(&own),
                &unmatched,
                &[candidate(107.0, 1700)],
                &context(1700),
            )
            .unwrap();
        assert_eq!(blocked[0].status, MarkerSelectionStatus::Blocked);
        assert!(blocked[0].selected.is_none());
        assert!(state.tracks.contains_key(&own.hint.selection_id));
        // The click (t=1000) is older than 2 s, yet the fresh track continues.
        let resumed = state
            .apply_selections(
                std::slice::from_ref(&own),
                &cfg,
                &[candidate(109.0, 3100)],
                &context(3100),
            )
            .unwrap();
        assert_eq!(resumed[0].status, MarkerSelectionStatus::Ready);
        assert_eq!(resumed[0].count, 3);
        // A gap longer than 2 s since the last observation still loses the track.
        let late = state
            .apply_selections(
                std::slice::from_ref(&own),
                &cfg,
                &[candidate(109.0, 5200)],
                &context(5200),
            )
            .unwrap();
        assert_eq!(late[0].status, MarkerSelectionStatus::Stale);
        let mut disabled = cfg.clone();
        disabled.enabled = false;
        state
            .apply_selections(
                std::slice::from_ref(&own),
                &disabled,
                &[candidate(109.0, 5300)],
                &context(5300),
            )
            .unwrap();
        assert!(state.tracks.is_empty());
    }
    #[test]
    fn lost_history_and_stale_click_never_reacquire_automatic_target() {
        let mut state = MarkerSetupState::default();
        let own = selection(MarkerRole::Own, 100.0, 1000);
        approve(&mut state, &own);
        let cfg = config().marker_config.unwrap();
        let late = state
            .apply_selections(
                std::slice::from_ref(&own),
                &cfg,
                &[candidate(100.0, 4000)],
                &context(4000),
            )
            .unwrap();
        assert_eq!(late[0].status, MarkerSelectionStatus::Stale);
        assert!(late[0].selected.is_none());
        state.clear();
        let forged = state
            .apply_selections(&[own], &cfg, &[candidate(100.0, 4000)], &context(4000))
            .unwrap();
        assert_eq!(forged[0].status, MarkerSelectionStatus::Blocked);
    }
    #[test]
    fn thumbnail_metadata_separates_source_geometry_from_actual_preview_pixels() {
        let image = RgbaImage::new(1920, 1080);
        let thumbnail = image::DynamicImage::ImageRgba8(image)
            .thumbnail(1440, 900)
            .to_rgba8();
        let preview = MarkerSetupPreview {
            token: "test".into(),
            preview: "data:image/png;base64,".into(),
            width: 1920,
            height: 1080,
            preview_width: thumbnail.width(),
            preview_height: thumbnail.height(),
            timestamp_ms: 1000,
            map_roi: Roi {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            map_id: None,
            reference_map_width: 1920,
            reference_map_height: 1080,
            observed_map_width: 1920,
            observed_map_height: 1080,
            proposals: vec![],
            status: MarkerProposalStatus::Blocked,
            warnings: vec![],
        };
        let json = serde_json::to_value(preview).unwrap();
        assert_eq!(json["width"], 1920);
        assert_eq!(json["height"], 1080);
        assert_eq!(json["previewWidth"], 1440);
        assert_eq!(json["previewHeight"], 810);
    }

    #[test]
    fn merged_same_color_component_never_becomes_both_own_position_and_target() {
        let mut state = MarkerSetupState::default();
        let own = selection(MarkerRole::Own, 100.0, 1000);
        let target = selection(MarkerRole::Target, 110.0, 1000);
        approve(&mut state, &own);
        approve(&mut state, &target);
        let selections = vec![own, target];
        let cfg = config().marker_config.unwrap();
        let initial = state
            .apply_selections(
                &selections,
                &cfg,
                &[candidate(100.0, 1000), candidate(110.0, 1000)],
                &context(1000),
            )
            .unwrap();
        assert!(initial
            .iter()
            .all(|r| r.status == MarkerSelectionStatus::Collecting));
        let merged = state
            .apply_selections(&selections, &cfg, &[candidate(105.0, 1500)], &context(1500))
            .unwrap();
        assert!(merged
            .iter()
            .all(|r| r.status == MarkerSelectionStatus::Ambiguous
                && r.selected.is_none()
                && r.state.is_none()));
        assert!(state.tracks.is_empty());
    }
}
