#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod automation;
mod ballistics_commands;
mod catalog_maps;
mod map_hotkeys;
mod marker_setup;
mod mtc_hud;
mod recording;
mod resources;
use base64::{engine::general_purpose::STANDARD, Engine};
use mtc_maps::{MapCalibration, Point};
use mtc_profiles::WeaponProfile;
use mtc_vision::{Observation, Roi};
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Default)]
struct Runtime {
    click_through: Mutex<bool>,
    latest: Mutex<serde_json::Value>,
    capture_busy: Mutex<bool>,
    wgc_session: Mutex<Option<mtc_capture::WindowCaptureSession>>,
    vision: Mutex<VisionState>,
    automation: Mutex<automation::AutomationState>,
    assets: Mutex<automation::AutomationAssets>,
    lifecycle: Mutex<()>,
    capture_epoch: AtomicU64,
    assets_generation: AtomicU64,
    resource_cache: Mutex<Option<mtc_assets::AssetCache>>,
    marker_setup: Mutex<marker_setup::MarkerSetupState>,
}

#[derive(Default)]
struct VisionState {
    scope: String,
    previous: Option<mtc_vision::FrameAnalysis>,
    target_samples: Vec<mtc_vision::PositionSample>,
    own_samples: Vec<mtc_vision::PositionSample>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalculationInput {
    calibration: MapCalibration,
    own: Point,
    target: Point,
    profile: WeaponProfile,
    height_delta: Option<f64>,
    target_velocity: Option<[f64; 2]>,
    #[serde(default)]
    own_velocity: Option<[f64; 2]>,
    #[serde(default)]
    map_unit: Option<String>,
    #[serde(default)]
    max_lead_time: Option<f64>,
    #[serde(default)]
    motion_3d: Option<Motion3d>,
    #[serde(default)]
    terrain_map_id: Option<String>,
    #[serde(default)]
    current_map_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Motion3d {
    target_vertical_velocity: f64,
    own_vertical_velocity: f64,
    inherit_own_velocity: bool,
    max_time: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CalculationResult {
    bearing: mtc_maps::Bearing,
    solution: Option<mtc_ballistics::Solution>,
    ballistic_warning: Option<String>,
    lead: Option<[f64; 2]>,
    lead_warning: Option<String>,
    intercept_3d: Option<mtc_ballistics::InterceptSolution3d>,
    dynamic_solution: Option<mtc_ballistics::DynamicSolution>,
    /// Target minus own height actually used by the solver, if any.
    height_delta: Option<f64>,
    /// `terrain` (loaded heightmap) or `manual` (user input); `None` without height.
    height_source: Option<&'static str>,
}

#[tauri::command]
fn profiles() -> Vec<WeaponProfile> {
    mtc_profiles::all_profiles()
}
#[tauri::command]
fn community_maps() -> Vec<mtc_profiles::CommunityMap> {
    mtc_profiles::community_maps()
}
#[tauri::command]
fn community_map_scale(id: String, width: f64, source_acknowledged: bool) -> Result<f64, String> {
    mtc_profiles::community_maps()
        .iter()
        .find(|m| m.id == id)
        .ok_or("Nieznana mapa")?
        .game_meters_per_pixel(width, source_acknowledged)
}
#[tauri::command]
fn calculate(state: State<Runtime>, input: CalculationInput) -> Result<CalculationResult, String> {
    calculate_data(&state, input)
}
fn calculate_data(
    state: &Runtime,
    mut input: CalculationInput,
) -> Result<CalculationResult, String> {
    input.profile.validate()?;
    let mut height_source = input.height_delta.map(|_| "manual");
    let mut terrain_warning = None;
    if let Some(id) = &input.terrain_map_id {
        if input.current_map_id.as_ref() != Some(id) {
            return Err("Mapa wysokości dotyczy innej bieżącej mapy".into());
        }
        let assets = state.assets.lock().map_err(|e| e.to_string())?;
        let map = assets
            .maps
            .iter()
            .find(|m| &m.id == id)
            .ok_or("Brak bieżącej mapy w bibliotece")?;
        // Uncertainty does not move positions; only the pixel→world mapping must agree.
        if !map.calibration.same_geometry(&input.calibration) {
            return Err("Kalibracja pozycji nie pasuje do mapy wysokości".into());
        }
        let terrain = assets
            .terrains
            .iter()
            .find(|t| &t.metadata().map_id == id)
            .ok_or("Brak mapy wysokości")?;
        // A missing sample at one point keeps distance/azimuth; it never becomes height 0.
        match terrain.height_delta(
            input.own,
            input.target,
            id,
            input.map_unit.as_deref().unwrap_or("game_units"),
        ) {
            Ok(delta) => {
                input.height_delta = Some(delta);
                height_source = Some("terrain");
            }
            Err(e) => {
                terrain_warning = Some(format!(
                    "Mapa wysokości: {e}{}",
                    if input.height_delta.is_some() {
                        " — użyto ręcznej różnicy wysokości"
                    } else {
                        ""
                    }
                ))
            }
        }
    }
    let bearing = mtc_maps::bearing(&input.calibration, input.own, input.target)?;
    let units_match = input.map_unit.as_deref().unwrap_or("game_units") == input.profile.unit;
    let dynamic = input.profile.dynamics_model() != mtc_profiles::DynamicsModel::Vacuum;
    let moving = [input.target_velocity, input.own_velocity]
        .into_iter()
        .flatten()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e-9);
    let mut dynamic_solution = None;
    let (solution, ballistic_warning) = if !units_match {
        (
            None,
            Some("Jednostki mapy i profilu pocisku są różne".into()),
        )
    } else if dynamic && (moving || input.motion_3d.is_some()) {
        (None,Some("Model oporu i napędu wymaga nieruchomego celu i stanowiska w tym obliczeniu; dla ruchomego celu użyj iteracyjnego przechwycenia beta (dynamic_intercept) z jawnym ruchem 3D".into()))
    } else if let Some(height) = input.height_delta {
        if dynamic {
            match mtc_ballistics::solve_dynamic(
                &input.profile,
                &mtc_ballistics::DynamicSolveInput {
                    distance: bearing.distance,
                    height_delta: height,
                    azimuth_deg: bearing.azimuth_deg,
                    unit: input.map_unit.clone().unwrap_or("game_units".into()),
                    options: mtc_ballistics::DynamicSolveOptions::default(),
                },
            ) {
                Ok(s) => {
                    let presentation = mtc_ballistics::Solution {
                        arcs: s
                            .arcs
                            .iter()
                            .map(|arc| mtc_ballistics::ArcSolution {
                                elevation_deg: arc.elevation_deg,
                                flight_time: arc.flight_time,
                                high_arc: arc.high_arc,
                            })
                            .collect(),
                        dispersion_radius_95: None,
                        assumptions: s.assumptions.clone(),
                    };
                    dynamic_solution = Some(s);
                    (Some(presentation), None)
                }
                Err(e) => (None, Some(e)),
            }
        } else {
            match mtc_ballistics::solve(&input.profile, bearing.distance, height) {
                Ok(s) => (Some(s), None),
                Err(e) => (None, Some(e)),
            }
        }
    } else {
        (
            None,
            Some(terrain_warning.clone().unwrap_or_else(|| {
                "Brak zaobserwowanej różnicy wysokości — dostępny dystans i azymut".into()
            })),
        )
    };
    let ballistic_warning = match (ballistic_warning, &terrain_warning) {
        (None, Some(warning)) => Some(warning.clone()),
        (warning, _) => warning,
    };
    let (mut lead, mut lead_warning) = if let Some(v) = input.target_velocity {
        if dynamic {
            (
                None,
                Some("Wyprzedzenie prostoliniowe nie obsługuje oporu i napędu".into()),
            )
        } else if input.profile.can_calculate().is_err() || !units_match {
            (
                None,
                Some("Wyprzedzenie wymaga skalibrowanej prędkości pocisku".into()),
            )
        } else {
            let own = input.own_velocity.unwrap_or([0.0, 0.0]);
            let relative_velocity = [v[0] - own[0], v[1] - own[1]];
            let a = input.calibration.transform(input.own)?;
            let b = input.calibration.transform(input.target)?;
            match mtc_ballistics::intercept(
                [b.x - a.x, b.y - a.y],
                relative_velocity,
                input.profile.speed.ok_or("Brak prędkości pocisku")?,
            ) {
                Ok((time, _))
                    if input.max_lead_time.is_some_and(|limit| {
                        !limit.is_finite() || limit <= 0.0 || time > limit
                    }) =>
                {
                    (
                        None,
                        Some("Czas wyprzedzenia przekracza horyzont obserwowanego ruchu".into()),
                    )
                }
                Ok((_, l)) => (
                    Some(l),
                    Some(if input.own_velocity.is_some() {
                        "Przybliżenie prostoliniowe bez grawitacji; zakłada dziedziczenie prędkości stanowiska".into()
                    } else {
                        "Przybliżenie prostoliniowe bez grawitacji i ruchu własnego".into()
                    }),
                ),
                Err(e) => (None, Some(e)),
            }
        }
    } else {
        (None, None)
    };
    // The closed-form 3D intercept is vacuum-only; for drag/rocket keep the
    // bearing and the warning instead of failing the whole calculation.
    let intercept_3d = if let Some(motion) = input.motion_3d.filter(|_| !dynamic) {
        if motion.inherit_own_velocity && input.own_velocity.is_none() {
            return Err("Model 3D wymaga pełnej prędkości własnego stanowiska".into());
        }
        let own = input.calibration.transform(input.own)?;
        let target = input.calibration.transform(input.target)?;
        let velocity = input
            .target_velocity
            .ok_or("Model 3D wymaga świeżej prędkości celu")?;
        let own_velocity = input.own_velocity.unwrap_or([0.0, 0.0]);
        Some(mtc_ballistics::solve_intercept_3d(
            &input.profile,
            &mtc_ballistics::InterceptInput3d {
                relative: [
                    target.x - own.x,
                    target.y - own.y,
                    input
                        .height_delta
                        .ok_or("Model 3D wymaga różnicy wysokości")?,
                ],
                target_velocity: [velocity[0], velocity[1], motion.target_vertical_velocity],
                own_velocity: [
                    own_velocity[0],
                    own_velocity[1],
                    motion.own_vertical_velocity,
                ],
                inherit_own_velocity: Some(motion.inherit_own_velocity),
                unit: input.map_unit.unwrap_or("game_units".into()),
                max_time: motion
                    .max_time
                    .min(input.max_lead_time.unwrap_or(motion.max_time)),
            },
        )?)
    } else {
        None
    };
    if let Some(solution) = &intercept_3d {
        lead = solution.arcs.first().map(|a| [a.lead[0], a.lead[1]]);
        lead_warning = None;
    }
    Ok(CalculationResult {
        bearing,
        solution,
        ballistic_warning,
        lead,
        lead_warning,
        intercept_3d,
        dynamic_solution,
        height_delta: input.height_delta,
        height_source,
    })
}

#[tauri::command]
async fn projectile_simulation(
    profile: WeaponProfile,
    input: mtc_ballistics::SimulationInput,
) -> Result<mtc_ballistics::TrajectorySimulation, String> {
    tauri::async_runtime::spawn_blocking(move || mtc_ballistics::simulate(&profile, &input))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn dynamic_solve(
    profile: WeaponProfile,
    input: mtc_ballistics::DynamicSolveInput,
) -> Result<mtc_ballistics::DynamicSolution, String> {
    tauri::async_runtime::spawn_blocking(move || mtc_ballistics::solve_dynamic(&profile, &input))
        .await
        .map_err(|e| e.to_string())?
}

fn require_epoch(state: &Runtime, epoch: u64) -> Result<(), String> {
    if epoch != state.capture_epoch.load(Ordering::Acquire) {
        Err("Analiza została unieważniona przez zmianę źródła lub zatrzymanie".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn calibrate_map(a: Point, b: Point, distance: f64) -> Result<f64, String> {
    mtc_maps::calibrate_two_points(a, b, distance)
}

#[tauri::command]
fn publish(
    app: tauri::AppHandle,
    state: State<Runtime>,
    value: serde_json::Value,
) -> Result<(), String> {
    *state.latest.lock().map_err(|e| e.to_string())? = value.clone();
    app.emit_to("overlay", "tactical-update", value)
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn latest(state: State<Runtime>) -> serde_json::Value {
    state.latest.lock().unwrap().clone()
}

#[cfg(windows)]
fn show_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    // Keep both flags in Tao's cache: a raw Win32 show leaves VISIBLE=false,
    // causing a later click-through style update to hide the overlay again.
    // The passive read-only overlay keeps WS_EX_NOACTIVATE through Tao's
    // FOCUSABLE=false flag, including when cursor-event styles are rebuilt.
    window.set_focusable(false).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    // This synchronous dispatcher getter follows the queued flag updates and
    // ensures their completion before overlay_control applies click-through.
    if !window.is_visible().map_err(|e| e.to_string())? {
        return Err("Nie można pokazać pasywnej nakładki".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn show_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

#[tauri::command]
fn overlay_control(
    app: tauri::AppHandle,
    state: State<Runtime>,
    visible: Option<bool>,
    click_through: Option<bool>,
) -> Result<bool, String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("Brak okna nakładki")?;
    if let Some(v) = visible {
        if v {
            show_overlay(&window)?;
        } else {
            window.hide().map_err(|e| e.to_string())?;
        }
    }
    let mut click = state.click_through.lock().map_err(|e| e.to_string())?;
    if let Some(v) = click_through {
        window
            .set_ignore_cursor_events(v)
            .map_err(|e| e.to_string())?;
        *click = v;
    }
    app.emit("overlay-state", serde_json::json!({"clickThrough":*click,"visible":window.is_visible().map_err(|e|e.to_string())?}))
        .map_err(|e| e.to_string())?;
    Ok(*click)
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let args: Vec<String> = std::env::args().collect();
    let dir = if let Some(index) = args.iter().position(|v| v == "--test-data-dir") {
        std::path::PathBuf::from(
            args.get(index + 1)
                .ok_or("Brak katalogu danych testowych")?,
        )
    } else {
        app.path().app_data_dir().map_err(|e| e.to_string())?
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings-v4.json"))
}
#[tauri::command]
fn load_settings(app: tauri::AppHandle) -> Result<Option<serde_json::Value>, String> {
    let mut path = settings_path(&app)?;
    for filename in ["settings-v3.json", "settings-v2.json", "settings-v1.json"] {
        if !path.exists() {
            path = path.with_file_name(filename);
        }
    }
    if !path.exists() {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Uszkodzone ustawienia: {e}"))?;
    if ![Some(1), Some(2), Some(3), Some(4)]
        .contains(&value.get("schemaVersion").and_then(|v| v.as_u64()))
    {
        return Err("Nieobsługiwana wersja ustawień".into());
    }
    Ok(Some(value))
}
/// Copies the current settings file aside before a corrupted/unsupported file
/// could be replaced by defaults. Returns the backup path, or None without a file.
#[tauri::command]
fn backup_settings(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let path = settings_path(&app)?;
    let mut source = path.clone();
    for filename in ["settings-v3.json", "settings-v2.json", "settings-v1.json"] {
        if !source.exists() {
            source = path.with_file_name(filename);
        }
    }
    if !source.exists() {
        return Ok(None);
    }
    let backup = path.with_file_name(format!("settings-backup-invalid-{}.json", now_ms()));
    std::fs::copy(&source, &backup).map_err(|e| format!("Nie utworzono kopii ustawień: {e}"))?;
    Ok(Some(backup.to_string_lossy().into_owned()))
}
#[tauri::command]
fn save_settings(app: tauri::AppHandle, value: serde_json::Value) -> Result<(), String> {
    if value.get("schemaVersion").and_then(|v| v.as_u64()) != Some(4) {
        return Err("Brak wersji ustawień".into());
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    if bytes.len() > 512_000 {
        return Err("Ustawienia przekraczają limit rozmiaru".into());
    }
    let path = settings_path(&app)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn image_data(img: &image::RgbaImage) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}
fn read_image(path: &str) -> Result<image::RgbaImage, String> {
    Ok(read_image_fingerprint(path)?.0)
}
fn read_image_fingerprint(path: &str) -> Result<(image::RgbaImage, String), String> {
    let (bytes, sha) = read_file_fingerprint(path)?;
    Ok((decode_image(bytes)?, sha))
}
/// Bounded read plus SHA-256, without decoding (cheap check of an unchanged file).
fn read_file_fingerprint(path: &str) -> Result<(Vec<u8>, String), String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Plik obrazu przekracza 64 MB".into());
    }
    let sha = format!("{:x}", Sha256::digest(&bytes));
    Ok((bytes, sha))
}
fn decode_image(bytes: Vec<u8>) -> Result<image::RgbaImage, String> {
    Ok(decode_oriented(bytes)?.0)
}
/// Decodes with limits and applies EXIF orientation, so Rust pixel coordinates
/// match what the WebView shows. Returns whether a transform was applied.
fn decode_oriented(bytes: Vec<u8>) -> Result<(image::RgbaImage, bool), String> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    let oriented = orientation != image::metadata::Orientation::NoTransforms;
    if oriented {
        image.apply_orientation(orientation);
    }
    Ok((image.to_rgba8(), oriented))
}
/// The original PNG/JPEG bytes are shown as-is (no slow re-encoding of large
/// maps); the decode still validates format, limits and the reported size.
fn import_image_data(path: &str) -> Result<serde_json::Value, String> {
    let (bytes, sha256) = read_file_fingerprint(path)?;
    let mime = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => Some("image/png"),
        Ok(image::ImageFormat::Jpeg) => Some("image/jpeg"),
        _ => None,
    };
    let encoded = mime.map(|mime| format!("data:{mime};base64,{}", STANDARD.encode(&bytes)));
    let (img, oriented) = decode_oriented(bytes)?;
    // An EXIF-rotated file is re-encoded from the oriented pixels: WebView and
    // Rust must agree on the pixel grid used for clicks and calibration.
    let data_url = match encoded {
        Some(url) if !oriented => url,
        _ => image_data(&img)?,
    };
    Ok(
        serde_json::json!({"dataUrl":data_url,"width":img.width(),"height":img.height(),"sha256":sha256}),
    )
}
#[tauri::command]
async fn image_fingerprint(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(read_image_fingerprint(&path)?.1))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn import_image(path: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || import_image_data(&path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn import_profile(path: String) -> Result<WeaponProfile, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 32_000 {
        return Err("Profil przekracza limit rozmiaru".into());
    }
    let p: WeaponProfile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    p.validate()?;
    Ok(p)
}
#[tauri::command]
fn export_json(path: String, value: serde_json::Value) -> Result<(), String> {
    if !path.to_lowercase().ends_with(".json") {
        return Err("Wybierz plik .json".into());
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn import_markers(path: String) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 100_000 {
        return Err("Zbyt duży plik".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[tauri::command]
fn capture_windows() -> Result<Vec<mtc_capture::CaptureWindow>, String> {
    mtc_capture::list_windows()
}

#[tauri::command]
fn diagnostics(app: tauri::AppHandle, state: State<Runtime>) -> Result<serde_json::Value, String> {
    let w = app.get_webview_window("overlay").ok_or("Brak nakładki")?;
    let shortcuts = [Code::KeyO, Code::KeyI, Code::KeyM].map(|key| {
        app.global_shortcut().is_registered(Shortcut::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            key,
        ))
    });
    Ok(
        serde_json::json!({"overlayVisible":w.is_visible().map_err(|e|e.to_string())?,"alwaysOnTop":w.is_always_on_top().map_err(|e|e.to_string())?,"decorated":w.is_decorated().map_err(|e|e.to_string())?,"clickThrough":*state.click_through.lock().map_err(|e|e.to_string())?,"dpiScale":w.scale_factor().map_err(|e|e.to_string())?,"shortcutsRegistered":shortcuts,"settingsPath":settings_path(&app)?}),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Analysis {
    observation: Option<Observation>,
    text: String,
    preview: String,
    elapsed_ms: u128,
    timestamp_ms: u64,
    width: u32,
    height: u32,
}

fn analyse_image(
    img: image::RgbaImage,
    roi: Roi,
    kind: String,
    timestamp: u64,
) -> Result<Analysis, String> {
    let started = std::time::Instant::now();
    let crop = roi.crop(&img)?;
    // Same uniform 2x enlargement and light-text fallback as the batch analysis;
    // the previous per-axis 2500 px cap could stretch wide ROIs non-uniformly.
    let ocr_kind = serde_json::from_value::<mtc_vision::OcrKind>(serde_json::json!(kind))
        .unwrap_or(mtc_vision::OcrKind::Text);
    let (text, _) = mtc_vision::read_roi_text(&img, roi, 2, ocr_kind, mtc_vision::ocr)?;
    let observation = mtc_vision::parse_numeric(&kind, &text, timestamp);
    Ok(Analysis {
        observation,
        text,
        preview: image_data(&crop)?,
        elapsed_ms: started.elapsed().as_millis(),
        timestamp_ms: timestamp,
        width: img.width(),
        height: img.height(),
    })
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn capture_frame(
    app: &tauri::AppHandle,
    handle: i64,
    backend: mtc_capture::CaptureBackend,
    interval: std::time::Duration,
) -> Result<mtc_capture::CapturedFrame, String> {
    let state = app.state::<Runtime>();
    let mut session = state.wgc_session.lock().map_err(|e| e.to_string())?;
    match backend {
        mtc_capture::CaptureBackend::WindowsGraphicsCapture => {
            if session.as_ref().is_some_and(|s| s.handle() != handle) {
                if let Some(mut old) = session.take() {
                    old.stop().map_err(|e| e.to_string())?;
                }
            }
            if session.is_none() {
                *session = Some(
                    mtc_capture::WindowCaptureSession::start_with_interval(handle, interval)
                        .map_err(|e| e.to_string())?,
                );
            }
            session
                .as_mut()
                .ok_or("Brak sesji przechwytywania")?
                .set_frame_interval(interval);
            let result = session
                .as_mut()
                .ok_or("Brak sesji przechwytywania")?
                .next_frame(std::time::Duration::from_secs(2));
            if let Err(error) = &result {
                // A lost GPU device or dead worker never recovers inside the old
                // session; drop it so the next request starts a new one.
                if error.requires_session_restart() {
                    if let Some(mut old) = session.take() {
                        let _ = old.stop();
                    }
                }
            }
            result.map_err(|e| e.to_string())
        }
        mtc_capture::CaptureBackend::Gdi => {
            if let Some(mut old) = session.take() {
                old.stop().map_err(|e| e.to_string())?;
            }
            mtc_capture::capture_window_with_backend(handle, backend).map_err(|e| e.to_string())
        }
    }
}

#[tauri::command]
async fn stop_capture(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<Runtime>()
        .capture_epoch
        .fetch_add(1, Ordering::AcqRel);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Runtime>();
        let _lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
        if let Some(mut session) = state.wgc_session.lock().map_err(|e| e.to_string())?.take() {
            session.stop().map_err(|e| e.to_string())?;
        }
        *state.vision.lock().map_err(|e| e.to_string())? = VisionState::default();
        *state.automation.lock().map_err(|e| e.to_string())? =
            automation::AutomationState::default();
        state
            .marker_setup
            .lock()
            .map_err(|e| e.to_string())?
            .clear();
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn capture_preview(
    handle: i64,
    backend: mtc_capture::CaptureBackend,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let frame=mtc_capture::capture_window_with_backend(handle,backend).map_err(|e|e.to_string())?;
        let thumbnail=mtc_vision::preview_thumbnail(&frame.image,1440,900);
        Ok(serde_json::json!({"dataUrl":image_data(&thumbnail)?,"width":frame.image.width(),"height":frame.image.height(),"timestampMs":frame.captured_at_ms,"backend":frame.backend}))
    }).await.map_err(|e|e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchAnalysis {
    analysis: mtc_vision::FrameAnalysis,
    configuration: mtc_vision::AnalysisConfig,
    preview: Option<String>,
    elapsed_ms: f64,
    timestamp_ms: u64,
    width: u32,
    height: u32,
    backend: String,
    target_motion: Option<mtc_vision::MotionEstimate>,
    own_motion: Option<mtc_vision::MotionEstimate>,
    automation: automation::AutomationReport,
    marker_selections: Vec<mtc_vision::MarkerSelectionResult>,
}

struct BatchContext {
    handle: Option<i64>,
    selections: Vec<marker_setup::NativeMarkerSelection>,
    timestamp: u64,
    backend: String,
    config: mtc_vision::AnalysisConfig,
    calibration: Option<MapCalibration>,
    scope: String,
    max_speed: f64,
    include_preview: bool,
    automation: automation::AutomationOptions,
    epoch: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchRequest {
    handle: i64,
    backend: mtc_capture::CaptureBackend,
    config: mtc_vision::AnalysisConfig,
    calibration: Option<MapCalibration>,
    scope: String,
    max_speed: f64,
    include_preview: bool,
    fps: f64,
    #[serde(default)]
    automation: automation::AutomationOptions,
    #[serde(default)]
    selections: Vec<marker_setup::NativeMarkerSelection>,
}
fn batch_image(
    app: &tauri::AppHandle,
    img: image::RgbaImage,
    context: BatchContext,
) -> Result<BatchAnalysis, String> {
    let BatchContext {
        handle,
        selections,
        timestamp,
        backend,
        mut config,
        mut calibration,
        scope,
        max_speed,
        include_preview,
        automation: options,
        epoch,
    } = context;
    let state = app.state::<Runtime>();
    let _lifecycle = state.lifecycle.lock().map_err(|e| e.to_string())?;
    require_epoch(&state, epoch)?;
    if config.reference_width == 0 && config.reference_height == 0 {
        config.reference_width = img.width();
        config.reference_height = img.height();
    }
    let scope = format!(
        "{scope}:{}",
        serde_json::to_string(&(&config, &calibration, &options)).map_err(|e| e.to_string())?
    );
    let started = std::time::Instant::now();
    let mut automation = automation::prepare(app, &img, timestamp, &scope, &options, &mut config)?;
    if let Some(map) = &automation.active_map {
        calibration = Some(map.calibration);
    }
    let scope = format!(
        "{scope}:{}",
        automation
            .active_map
            .as_ref()
            .map(|m| m.id.as_str())
            .unwrap_or("manual")
    );
    let mut analysis = if options.auto_hud {
        mtc_vision::analyze_frame_from_discovery(
            &img,
            timestamp,
            now_ms(),
            &config,
            automation.hud.as_ref().ok_or("Brak wykrytego układu HUD")?,
        )?
    } else {
        mtc_vision::analyze_frame(&img, timestamp, now_ms(), &config)?
    };
    if options.auto_map {
        automation::map_markers(&img, timestamp, &mut config, &automation, &mut analysis)?;
    }
    let state = app.state::<Runtime>();
    let mut vision = state.vision.lock().map_err(|e| e.to_string())?;
    if vision.scope != scope || backend == "file" {
        *vision = VisionState {
            scope,
            ..VisionState::default()
        };
    }
    mtc_vision::stabilize_frame(&mut analysis, vision.previous.as_ref());
    // No selected component or role is inferred when multiple visible candidates match.
    for role in [mtc_vision::MarkerRole::Own, mtc_vision::MarkerRole::Target] {
        let indices: Vec<_> = analysis
            .markers
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == role)
            .map(|(i, _)| i)
            .collect();
        if indices.len() == 1 {
            let index = indices[0];
            let current = &analysis.markers[index];
            let previous: Vec<_> = vision
                .previous
                .as_ref()
                .map(|p| p.markers.iter().filter(|m| m.role == role).collect())
                .unwrap_or_default();
            if current.confidence >= 0.6
                && previous.len() == 1
                && previous[0].confidence >= 0.6
                && previous[0].signature_id == current.signature_id
                && previous[0].timestamp_ms <= current.timestamp_ms
                && current.timestamp_ms - previous[0].timestamp_ms <= 2000
            {
                let map = config
                    .marker_config
                    .as_ref()
                    .ok_or("Brak kalibracji markerów")?;
                let delta = (previous[0].map_position.x - current.map_position.x)
                    .hypot(previous[0].map_position.y - current.map_position.y);
                if delta < (map.map_width as f64).hypot(map.map_height as f64) * 0.1 {
                    analysis.markers[index].confidence = 0.9;
                }
            }
        }
    }
    if now_ms().saturating_sub(timestamp) > config.max_age_ms {
        mtc_vision::invalidate_frame(
            &mut analysis,
            mtc_vision::AnalysisStatus::Stale,
            "Przetwarzanie trwało zbyt długo — klatka została unieważniona",
        );
    }
    let mut marker_selections = Vec::new();
    if !selections.is_empty() {
        let map = config
            .marker_config
            .as_ref()
            .ok_or("Brak konfiguracji markerów")?;
        let capture_backend = if backend == "gdi" {
            mtc_capture::CaptureBackend::Gdi
        } else {
            mtc_capture::CaptureBackend::WindowsGraphicsCapture
        };
        let context = marker_setup::MarkerSelectionFrameContext {
            handle: handle.ok_or("Wybór markera wymaga aktywnego okna")?,
            backend: capture_backend,
            map_id: automation
                .active_map
                .as_ref()
                .map(|m| m.id.clone())
                .or_else(|| options.preferred_map_id.clone()),
            calibration,
            auto_map: options.auto_map,
            frame_width: img.width(),
            frame_height: img.height(),
            timestamp_ms: timestamp,
            now_ms: now_ms(),
            capture_epoch: epoch,
            max_speed_pixels_per_second: calibration
                .map(|c| max_speed / c.units_per_pixel)
                .unwrap_or(max_speed),
        };
        marker_selections = state
            .marker_setup
            .lock()
            .map_err(|e| e.to_string())?
            .apply_selections(&selections, map, &analysis.markers, &context)?;
        // A clicked role never falls back to another same-color component.
        for selection in &selections {
            analysis.markers.retain(|m| m.role != selection.hint.role);
        }
        for selection in &marker_selections {
            if selection.status == mtc_vision::MarkerSelectionStatus::Ready {
                if let (Some(marker), Some(track_id)) = (&selection.selected, &selection.track_id) {
                    let mut marker = marker.clone();
                    marker.signature_id = track_id.clone();
                    analysis.markers.push(marker);
                }
            }
        }
    }
    let mut target_motion = None;
    let mut own_motion = None;
    if let (Some(calibration), Some(map)) = (calibration, config.marker_config.as_ref()) {
        if backend != "file" && map.enabled && map.coordinate_calibration_confirmed {
            let tracker = mtc_vision::TrackerConfig {
                map_width: map.map_width,
                map_height: map.map_height,
                max_age_ms: 2000,
                max_speed_units_per_second: max_speed,
                ..mtc_vision::TrackerConfig::default()
            };
            for role in [mtc_vision::MarkerRole::Own, mtc_vision::MarkerRole::Target] {
                let markers: Vec<_> = analysis.markers.iter().filter(|m| m.role == role).collect();
                let samples = if role == mtc_vision::MarkerRole::Own {
                    &mut vision.own_samples
                } else {
                    &mut vision.target_samples
                };
                if markers.len() == 1 {
                    let m = markers[0];
                    samples.push(mtc_vision::PositionSample {
                        source_id: m.signature_id.clone(),
                        map_position: m.map_position,
                        timestamp_ms: m.timestamp_ms,
                        confidence: m.confidence,
                    });
                    samples.retain(|s| {
                        s.timestamp_ms <= timestamp && timestamp - s.timestamp_ms <= 2000
                    });
                    if samples.len() > 8 {
                        samples.drain(..samples.len() - 8);
                    }
                } else {
                    samples.clear();
                }
                let motion =
                    mtc_vision::estimate_motion(samples, &calibration, now_ms(), &tracker)?;
                if motion.status == mtc_vision::MotionStatus::Rejected {
                    for m in analysis.markers.iter_mut().filter(|m| m.role == role) {
                        m.confidence = 0.5;
                    }
                }
                if role == mtc_vision::MarkerRole::Own {
                    own_motion = Some(motion);
                } else {
                    target_motion = Some(motion);
                }
            }
        }
    }
    automation::resolve_analysis(&mut automation, &mut analysis)?;
    if options.auto_map && automation.active_map.is_none() {
        vision.target_samples.clear();
        vision.own_samples.clear();
        target_motion = None;
        own_motion = None;
    }
    vision.previous = Some(analysis.clone());
    let preview = if include_preview {
        Some(image_data(
            &image::DynamicImage::ImageRgba8(img.clone())
                .thumbnail(1440, 900)
                .to_rgba8(),
        )?)
    } else {
        None
    };
    Ok(BatchAnalysis {
        analysis,
        configuration: config,
        preview,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        timestamp_ms: timestamp,
        width: img.width(),
        height: img.height(),
        backend,
        target_motion,
        own_motion,
        automation,
        marker_selections,
    })
}

#[tauri::command]
async fn analyse_batch(
    app: tauri::AppHandle,
    request: BatchRequest,
) -> Result<BatchAnalysis, String> {
    let BatchRequest {
        handle,
        backend,
        config,
        calibration,
        scope,
        max_speed,
        include_preview,
        fps,
        automation,
        selections,
    } = request;
    if !fps.is_finite() || !(0.25..=3.0).contains(&fps) {
        return Err("Niepoprawny limit FPS".into());
    }
    let epoch = app.state::<Runtime>().capture_epoch.load(Ordering::Acquire);
    {
        let state = app.state::<Runtime>();
        let mut busy = state.capture_busy.lock().map_err(|e| e.to_string())?;
        if *busy {
            return Err("Poprzednia klatka jest przetwarzana".into());
        }
        *busy = true;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = BusyGuard(app.clone());
        let frame = capture_frame(
            &app,
            handle,
            backend,
            std::time::Duration::from_secs_f64(1.0 / fps),
        )?;
        batch_image(
            &app,
            frame.image,
            BatchContext {
                handle: Some(handle),
                selections,
                timestamp: frame.captured_at_ms,
                backend: if backend == mtc_capture::CaptureBackend::Gdi {
                    "gdi".into()
                } else {
                    "wgc".into()
                },
                config,
                calibration,
                scope: format!("live:{handle}:{backend:?}:{scope}"),
                max_speed,
                include_preview,
                automation,
                epoch,
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn analyse_batch_file(
    app: tauri::AppHandle,
    path: String,
    config: mtc_vision::AnalysisConfig,
    calibration: Option<MapCalibration>,
    max_speed: f64,
) -> Result<BatchAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        batch_image(
            &app,
            read_image(&path)?,
            BatchContext {
                handle: None,
                selections: Vec::new(),
                timestamp: now_ms(),
                backend: "file".into(),
                config,
                calibration,
                scope: format!("file:{path}"),
                max_speed,
                include_preview: true,
                automation: automation::AutomationOptions::default(),
                epoch: app.state::<Runtime>().capture_epoch.load(Ordering::Acquire),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn calibrate_map_view(
    handle: i64,
    backend: mtc_capture::CaptureBackend,
    roi: Roi,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move|| {
        let frame=mtc_capture::capture_window_with_backend(handle,backend).map_err(|e|e.to_string())?;
        let signature=mtc_vision::calibrate_view_signature(&frame.image,roi)?;
        let crop=roi.crop(&frame.image)?;
        Ok(serde_json::json!({"signature":signature,"mapImage":image_data(&crop)?,"width":crop.width(),"height":crop.height(),"referenceWidth":frame.image.width(),"referenceHeight":frame.image.height()}))
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
async fn analyse_batch_fixture(
    app: tauri::AppHandle,
    name: String,
) -> Result<BatchAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, roi) = match name.as_str() {
            "bm21" => (
                include_bytes!("../../../../tests/fixtures/bm21_grad_rozgrywka.jpg").as_slice(),
                Roi {
                    x: 0.465,
                    y: 0.123,
                    width: 0.095,
                    height: 0.043,
                },
            ),
            "helicopter" => (
                include_bytes!("../../../../tests/fixtures/smiglowiec_rozgrywka.jpg").as_slice(),
                Roi {
                    x: 0.455,
                    y: 0.188,
                    width: 0.096,
                    height: 0.047,
                },
            ),
            _ => return Err("Nieznana referencja".into()),
        };
        let img = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let config = mtc_vision::AnalysisConfig {
            schema_version: 1,
            reference_width: img.width(),
            reference_height: img.height(),
            aspect_tolerance: 0.03,
            max_age_ms: 2000,
            ocr_regions: vec![mtc_vision::OcrRegionConfig {
                id: "timer".into(),
                kind: mtc_vision::OcrKind::Timer,
                roi,
                upscale: 2,
            }],
            marker_config: None,
        };
        batch_image(
            &app,
            img,
            BatchContext {
                handle: None,
                selections: Vec::new(),
                timestamp: now_ms(),
                backend: "file".into(),
                config,
                calibration: None,
                scope: format!("fixture:{name}"),
                max_speed: 500.0,
                include_preview: true,
                automation: automation::AutomationOptions::default(),
                epoch: app.state::<Runtime>().capture_epoch.load(Ordering::Acquire),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

struct BusyGuard(tauri::AppHandle);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        // Never panic inside Drop: a poisoned flag is still reset.
        *self
            .0
            .state::<Runtime>()
            .capture_busy
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = false;
    }
}
#[tauri::command]
async fn analyse_capture(
    app: tauri::AppHandle,
    handle: i64,
    roi: Roi,
    kind: String,
) -> Result<Analysis, String> {
    {
        let state = app.state::<Runtime>();
        let mut busy = state.capture_busy.lock().map_err(|e| e.to_string())?;
        if *busy {
            return Err("Poprzednia klatka jest przetwarzana".into());
        }
        *busy = true;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = BusyGuard(app);
        let img = mtc_capture::capture_window(handle)?;
        let timestamp = now_ms();
        analyse_image(img, roi, kind, timestamp)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn analyse_file(path: String, roi: Roi, kind: String) -> Result<Analysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        analyse_image(read_image(&path)?, roi, kind, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn analyse_fixture(name: String) -> Result<Analysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, roi) = match name.as_str() {
            "bm21" => (
                include_bytes!("../../../../tests/fixtures/bm21_grad_rozgrywka.jpg").as_slice(),
                Roi {
                    x: 0.465,
                    y: 0.123,
                    width: 0.095,
                    height: 0.043,
                },
            ),
            "helicopter" => (
                include_bytes!("../../../../tests/fixtures/smiglowiec_rozgrywka.jpg").as_slice(),
                Roi {
                    x: 0.455,
                    y: 0.188,
                    width: 0.096,
                    height: 0.047,
                },
            ),
            _ => return Err("Nieznana referencja".into()),
        };
        analyse_image(
            image::load_from_memory(bytes)
                .map_err(|e| e.to_string())?
                .to_rgba8(),
            roi,
            "timer".into(),
            now_ms(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    let show = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    let click = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyI);
    let panel = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
    tauri::Builder::default()
        .manage(Runtime::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &show {
                        if let Some(w) = app.get_webview_window("overlay") {
                            let visible = !w.is_visible().unwrap_or(false);
                            let _ = overlay_control(app.clone(), app.state(), Some(visible), None);
                        }
                    } else if shortcut == &click {
                        let new = !(*app.state::<Runtime>().click_through.lock().unwrap());
                        let _ = overlay_control(app.clone(), app.state(), None, Some(new));
                    } else if shortcut == &panel {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    } else if let Some(action) = ballistics_commands::quick_range_action(shortcut) {
                        let _ = app.emit("quick-range", action);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("overlay") {
                #[cfg(windows)]
                w.set_focusable(false)?;
                w.set_content_protected(true)?;
            }
            for shortcut in [show, click, panel] {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    eprintln!("Skrót niedostępny: {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            profiles,
            calculate,
            projectile_simulation,
            dynamic_solve,
            resources::resource_catalog,
            resources::sync_resources,
            marker_setup::capture_marker_setup,
            marker_setup::calibrate_visible_marker,
            recording::recording_start,
            recording::recording_stop,
            recording::recording_status,
            recording::recording_load,
            recording::analyse_recording,
            mtc_hud::read_weapon_panel,
            mtc_hud::read_weapon_panel_file,
            mtc_hud::read_mtc_map,
            mtc_hud::read_mtc_map_file,
            mtc_hud::mtc_map_point,
            mtc_hud::read_loadout_specs,
            mtc_hud::read_loadout_specs_file,
            mtc_hud::read_own_position,
            mtc_hud::read_own_position_file,
            map_hotkeys::configure_map_point_hotkeys,
            calibrate_map,
            publish,
            latest,
            overlay_control,
            load_settings,
            save_settings,
            backup_settings,
            import_image,
            image_fingerprint,
            import_profile,
            export_json,
            import_markers,
            capture_windows,
            analyse_capture,
            analyse_file,
            analyse_fixture,
            diagnostics,
            community_maps,
            community_map_scale,
            stop_capture,
            capture_preview,
            analyse_batch,
            analyse_batch_file,
            calibrate_map_view,
            analyse_batch_fixture,
            automation::load_automation_assets,
            automation::automation_assets_status,
            catalog_maps::prepare_catalog_map,
            automation::terrain_height,
            automation::fit_weapon,
            automation::resolve_weapon,
            automation::discover_hud_file,
            ballistics_commands::salvo_pattern,
            ballistics_commands::dynamic_intercept,
            ballistics_commands::range_card,
            ballistics_commands::lead_estimate,
            ballistics_commands::lead_iterated,
            ballistics_commands::quick_aim,
            ballistics_commands::fit_control_shots,
            ballistics_commands::air_to_ground_impact,
            ballistics_commands::air_to_ground_solve,
            ballistics_commands::quick_range_hotkeys
        ])
        .run(tauri::generate_context!())
        .expect("Nie udało się uruchomić aplikacji");
}

#[cfg(test)]
mod calculation_tests {
    use super::*;
    fn input(height: Option<f64>) -> CalculationInput {
        let mut profile = mtc_profiles::all_profiles()
            .into_iter()
            .find(|p| p.id == "tube")
            .unwrap();
        profile.speed = Some(100.0);
        profile.gravity = Some(10.0);
        profile.calibrated = true;
        profile.status = "beta".into();
        profile.source = "Syntetyczny pomiar do testu integracji".into();
        profile.game_version = "syntetyczna".into();
        serde_json::from_value(serde_json::json!({"calibration":{"unitsPerPixel":1.0,"northRotationDeg":0.0,"originPixel":{"x":0.0,"y":0.0},"originWorld":{"x":0.0,"y":0.0},"uncertaintyUnits":0.0},"own":{"x":0.0,"y":0.0},"target":{"x":100.0,"y":0.0},"profile":profile,"heightDelta":height,"targetVelocity":null})).unwrap()
    }
    #[test]
    fn missing_height_preserves_bearing_without_fabricating_a_trajectory() {
        let result = calculate_data(&Runtime::default(), input(None)).unwrap();
        assert_eq!(result.bearing.distance, 100.0);
        assert_eq!(result.bearing.azimuth_deg, 90.0);
        assert!(result.solution.is_none());
        assert!(result.ballistic_warning.unwrap().contains("wysokości"));
    }
    #[test]
    fn three_dimensional_motion_rejects_missing_height_and_platform_velocity() {
        for (height, inherit) in [(None, false), (Some(0.0), true)] {
            let mut request = input(height);
            request.target_velocity = Some([0.0, 0.0]);
            request.motion_3d = Some(Motion3d {
                target_vertical_velocity: 0.0,
                own_vertical_velocity: 0.0,
                inherit_own_velocity: inherit,
                max_time: 30.0,
            });
            assert!(calculate_data(&Runtime::default(), request).is_err());
        }
    }
    #[test]
    fn terrain_from_another_map_or_calibration_is_rejected() {
        let mut request = input(Some(0.0));
        request.current_map_id = Some("map-b".into());
        request.terrain_map_id = Some("map-a".into());
        assert!(calculate_data(&Runtime::default(), request)
            .err()
            .unwrap()
            .contains("bieżącej mapy"));
    }
    fn terrain_runtime(samples: &[Option<f64>]) -> Runtime {
        let state = Runtime::default();
        let calibration = MapCalibration {
            units_per_pixel: 1.0,
            north_rotation_deg: 0.0,
            origin_pixel: Point { x: 0.0, y: 0.0 },
            origin_world: Point { x: 0.0, y: 0.0 },
            uncertainty_units: 0.0,
        };
        let mut assets = state.assets.lock().unwrap();
        assets.maps.push(automation::StoredMap {
            sha256: "a".repeat(64),
            id: "map-a".into(),
            name: "Syntetyczna".into(),
            path: "C:/synthetic.png".into(),
            width: 101,
            height: 101,
            unit: "game_units".into(),
            source: "test".into(),
            confirmed: true,
            calibration,
        });
        assets.terrains.push(std::sync::Arc::new(
            mtc_maps::HeightMap::from_normalized(
                mtc_maps::HeightMapMetadata {
                    schema_version: 1,
                    map_id: "map-a".into(),
                    map_width: 101,
                    map_height: 101,
                    min_height: 0.0,
                    max_height: 100.0,
                    unit: "game_units".into(),
                    source: "test".into(),
                    confirmed: true,
                    coordinate_alignment_confirmed: true,
                },
                2,
                2,
                samples,
            )
            .unwrap(),
        ));
        drop(assets);
        state
    }
    fn terrain_request(uncertainty: f64, manual: Option<f64>) -> CalculationInput {
        let mut request = input(manual);
        request.calibration.uncertainty_units = uncertainty;
        request.current_map_id = Some("map-a".into());
        request.terrain_map_id = Some("map-a".into());
        request
    }
    #[test]
    fn terrain_height_is_used_even_when_position_uncertainty_differs() {
        // own (0,0) → 0; target (100,0) → 100 * 0.5.
        let state = terrain_runtime(&[Some(0.0), Some(0.5), Some(1.0), Some(1.0)]);
        let result = calculate_data(&state, terrain_request(15.0, None)).unwrap();
        assert_eq!(result.height_source, Some("terrain"));
        assert!((result.height_delta.unwrap() - 50.0).abs() < 1e-9);
        assert!(result.solution.is_some());
        assert_eq!(result.bearing.uncertainty_units, 30.0);
    }
    #[test]
    fn missing_terrain_sample_keeps_bearing_and_never_becomes_zero_height() {
        let state = terrain_runtime(&[Some(0.0), None, Some(1.0), Some(1.0)]);
        let result = calculate_data(&state, terrain_request(0.0, None)).unwrap();
        assert_eq!(result.bearing.distance, 100.0);
        assert!(result.height_delta.is_none() && result.solution.is_none());
        assert!(result.ballistic_warning.unwrap().contains("Mapa wysokości"));
        let manual = calculate_data(&state, terrain_request(0.0, Some(3.0))).unwrap();
        assert_eq!(manual.height_source, Some("manual"));
        assert_eq!(manual.height_delta, Some(3.0));
        assert!(manual.ballistic_warning.unwrap().contains("ręcznej"));
    }
    #[test]
    fn terrain_with_other_scale_is_still_rejected() {
        let state = terrain_runtime(&[Some(0.0), Some(0.5), Some(1.0), Some(1.0)]);
        let mut request = terrain_request(0.0, None);
        request.calibration.units_per_pixel = 2.0;
        assert!(calculate_data(&state, request)
            .err()
            .unwrap()
            .contains("Kalibracja"));
    }
    #[test]
    fn exif_orientation_is_applied_before_pixel_coordinates_are_used() {
        // 3×1 JPEG with EXIF orientation 6 (rotate 90° CW) must decode as 1×3.
        let mut jpeg = Vec::new();
        let source = image::RgbImage::from_pixel(3, 1, image::Rgb([200, 10, 10]));
        image::DynamicImage::ImageRgb8(source)
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let exif: &[u8] = &[
            0xFF, 0xE1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2A, 0, 8, 0, 0, 0,
            1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        let mut with_exif = jpeg[..2].to_vec();
        with_exif.extend_from_slice(exif);
        with_exif.extend_from_slice(&jpeg[2..]);
        let (image, oriented) = decode_oriented(with_exif).unwrap();
        assert!(oriented);
        assert_eq!(image.dimensions(), (1, 3));
        let (plain, oriented) = decode_oriented(jpeg).unwrap();
        assert!(!oriented);
        assert_eq!(plain.dimensions(), (3, 1));
    }
    #[test]
    fn stopped_capture_epoch_cannot_restore_measurement_history() {
        let state = Runtime::default();
        let captured_epoch = state.capture_epoch.load(Ordering::Acquire);
        state.capture_epoch.fetch_add(1, Ordering::AcqRel);
        assert!(require_epoch(&state, captured_epoch).is_err());
        assert!(require_epoch(&state, state.capture_epoch.load(Ordering::Acquire)).is_ok());
    }
    fn drag_input() -> CalculationInput {
        let mut request = input(Some(0.0));
        request.profile.dynamics = Some(mtc_profiles::ProjectileDynamics {
            model: mtc_profiles::DynamicsModel::Drag,
            unit: "game_units".into(),
            linear_drag: Some(0.0),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczne współczynniki do testu".into(),
        });
        request
    }
    #[test]
    fn locally_calibrated_drag_dispatches_numerical_solution() {
        let expected = calculate_data(&Runtime::default(), input(Some(0.0))).unwrap();
        let result = calculate_data(&Runtime::default(), drag_input()).unwrap();
        assert!(result.dynamic_solution.is_some());
        let expected = expected.solution.unwrap().arcs[0].elevation_deg;
        assert!((result.solution.unwrap().arcs[0].elevation_deg - expected).abs() < 0.01);
    }
    #[test]
    fn moving_target_never_receives_static_drag_trajectory_or_linear_lead() {
        let mut request = drag_input();
        request.target_velocity = Some([1.0, 0.0]);
        let result = calculate_data(&Runtime::default(), request).unwrap();
        assert!(result.dynamic_solution.is_none());
        assert!(result.solution.is_none());
        assert!(result.lead.is_none());
        assert!(result.ballistic_warning.unwrap().contains("nieruchomego"));
        assert!(result.lead_warning.unwrap().contains("oporu"));
    }
    #[test]
    fn drag_profile_with_3d_motion_keeps_bearing_and_warning_instead_of_failing() {
        let mut request = drag_input();
        request.target_velocity = Some([1.0, 0.0]);
        request.motion_3d = Some(Motion3d {
            target_vertical_velocity: 0.0,
            own_vertical_velocity: 0.0,
            inherit_own_velocity: false,
            max_time: 30.0,
        });
        let result = calculate_data(&Runtime::default(), request).unwrap();
        assert_eq!(result.bearing.distance, 100.0);
        assert!(result.intercept_3d.is_none());
        assert!(result.solution.is_none());
        assert!(result
            .ballistic_warning
            .unwrap()
            .contains("dynamic_intercept"));
    }
    #[test]
    fn source_acceptance_never_substitutes_for_drag_calibration() {
        let mut request = drag_input();
        request.profile.calibrated = false;
        request.profile.status = "experimental".into();
        let result = calculate_data(&Runtime::default(), request).unwrap();
        assert!(result.dynamic_solution.is_none());
        assert!(result.solution.is_none());
    }
    #[test]
    fn fractional_community_map_scales_survive_typed_and_value_json_roundtrips() {
        for scale in [
            0.9556135770234985_f64,
            0.9564459930313587,
            1.3180078124999999,
        ] {
            let calibration = MapCalibration {
                units_per_pixel: scale,
                north_rotation_deg: 0.0,
                origin_pixel: Point { x: 0.0, y: 0.0 },
                origin_world: Point { x: 0.0, y: 0.0 },
                uncertainty_units: 0.0,
            };
            let mut encoded = serde_json::to_string(&calibration).unwrap();
            for _ in 0..10 {
                let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
                encoded = serde_json::to_string(&value).unwrap();
                let decoded: MapCalibration = serde_json::from_str(&encoded).unwrap();
                assert_eq!(decoded.units_per_pixel.to_bits(), scale.to_bits());
            }
        }
    }
}
