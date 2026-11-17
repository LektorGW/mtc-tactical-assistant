//! Commands reading the MTC weapon-station panel and the full map (key M) from
//! the selected window or a local image. Visible pixels only; no game memory.
use crate::{capture_frame, now_ms, read_image};
use mtc_capture::CaptureBackend;
use mtc_vision::{MapCatalogEntry, MtcMapReading, WeaponPanelReading};
use serde::Serialize;
use std::sync::{Mutex, OnceLock};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponPanelAnalysis {
    pub panel: WeaponPanelReading,
    /// Catalog profile resolved from the explicit weapon/ammo labels, if any.
    pub profile: Option<mtc_profiles::WeaponResolution>,
    pub backend: String,
}

/// Previous live reading for two-frame stabilization (per window handle).
fn previous_panel() -> &'static Mutex<Option<(i64, WeaponPanelReading)>> {
    static PREVIOUS: OnceLock<Mutex<Option<(i64, WeaponPanelReading)>>> = OnceLock::new();
    PREVIOUS.get_or_init(|| Mutex::new(None))
}

fn known_weapon_names() -> &'static Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<String> = mtc_profiles::all_profiles()
            .iter()
            .map(mtc_profiles::profile_identity)
            .flat_map(|identity| std::iter::once(identity.vehicle).chain(identity.projectile))
            .chain(
                mtc_vision::OBSERVED_PANEL_NAMES
                    .iter()
                    .map(|s| s.to_string()),
            )
            // Curated in-game designations (e.g. "155 mm M185") so OCR snaps to
            // them instead of a wrong catalog vehicle; the resolver maps them.
            .chain(
                mtc_profiles::vehicle_aliases()
                    .into_iter()
                    .map(|a| a.observed),
            )
            .collect();
        names.sort();
        names.dedup();
        names
    })
}

pub fn map_catalog() -> Vec<MapCatalogEntry> {
    mtc_profiles::community_maps()
        .into_iter()
        .map(|map| MapCatalogEntry {
            id: map.id,
            name: map.name,
            size_game_meters: map.size_game_meters,
            height_game_meters: None,
        })
        .collect()
}

fn resolve_profile(
    panel: &WeaponPanelReading,
) -> Result<Option<mtc_profiles::WeaponResolution>, String> {
    let label = |text: &Option<mtc_vision::PanelText>| {
        text.as_ref().map(|t| mtc_profiles::OcrWeaponLabel {
            text: t.catalog_match.clone().unwrap_or_else(|| t.text.clone()),
            confidence: t.confidence.clamp(0.0, 1.0),
        })
    };
    let (vehicle, ammo) = (label(&panel.weapon), label(&panel.ammo));
    if vehicle.is_none() {
        return Ok(None);
    }
    // "Unloaded" is a state, not a projectile name; resolve by the weapon alone.
    let ammo = ammo.filter(|a| !a.text.eq_ignore_ascii_case("unloaded"));
    mtc_profiles::resolve_weapon_labels(
        &mtc_profiles::all_profiles(),
        vehicle.as_ref(),
        ammo.as_ref(),
    )
    .map(Some)
}

fn analyse_panel(
    image: &image::RgbaImage,
    timestamp: u64,
    source: Option<i64>,
    backend: &str,
) -> Result<WeaponPanelAnalysis, String> {
    let mut panel = mtc_vision::read_weapon_panel(image, timestamp, known_weapon_names());
    if let Some(handle) = source {
        let mut previous = previous_panel().lock().map_err(|e| e.to_string())?;
        let prior = previous
            .as_ref()
            .filter(|(h, _)| *h == handle)
            .map(|(_, p)| p);
        mtc_vision::stabilize_weapon_panel(&mut panel, prior);
        *previous = Some((handle, panel.clone()));
    }
    Ok(WeaponPanelAnalysis {
        profile: resolve_profile(&panel)?,
        panel,
        backend: backend.into(),
    })
}

fn backend_name(backend: CaptureBackend) -> &'static str {
    match backend {
        CaptureBackend::WindowsGraphicsCapture => "wgc",
        CaptureBackend::Gdi => "gdi",
    }
}

/// Live: reads the panel from the selected window (shares the persistent WGC session).
#[tauri::command]
pub async fn read_weapon_panel(
    app: tauri::AppHandle,
    handle: i64,
    backend: CaptureBackend,
) -> Result<WeaponPanelAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let frame = capture_frame(&app, handle, backend, std::time::Duration::from_millis(500))?;
        let analysis = analyse_panel(
            &frame.image,
            frame.captured_at_ms,
            Some(handle),
            backend_name(backend),
        )?;
        if now_ms().saturating_sub(frame.captured_at_ms) > 2000 {
            return Err("Odczyt panelu trwał zbyt długo; klatka jest nieaktualna".into());
        }
        Ok(analysis)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Offline: a saved frame or a recording frame (client-area image).
#[tauri::command]
pub async fn read_weapon_panel_file(path: String) -> Result<WeaponPanelAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        analyse_panel(&read_image(&path)?, now_ms(), None, "file")
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_mtc_map(
    app: tauri::AppHandle,
    handle: i64,
    backend: CaptureBackend,
) -> Result<MtcMapReading, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let frame = capture_frame(&app, handle, backend, std::time::Duration::from_millis(500))?;
        Ok(mtc_vision::read_mtc_full_map(
            &frame.image,
            frame.captured_at_ms,
            &map_catalog(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_mtc_map_file(path: String) -> Result<MtcMapReading, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(mtc_vision::read_mtc_full_map(
            &read_image(&path)?,
            now_ms(),
            &map_catalog(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Loadout screen ("Choose Weapon" / Specifications) of the selected window.
/// Muzzle velocity read here has the status "odczytane z UI gry".
#[tauri::command]
pub async fn read_loadout_specs(
    app: tauri::AppHandle,
    handle: i64,
    backend: CaptureBackend,
) -> Result<mtc_vision::LoadoutSpecs, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let frame = capture_frame(&app, handle, backend, std::time::Duration::from_millis(500))?;
        mtc_vision::read_loadout_specs(&frame.image, frame.captured_at_ms)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_loadout_specs_file(path: String) -> Result<mtc_vision::LoadoutSpecs, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mtc_vision::read_loadout_specs(&read_image(&path)?, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Previous own-position reading for two-frame stabilization (per window handle).
fn previous_own() -> &'static Mutex<Option<(i64, mtc_vision::OwnPositionReading)>> {
    static PREVIOUS: OnceLock<Mutex<Option<(i64, mtc_vision::OwnPositionReading)>>> =
        OnceLock::new();
    PREVIOUS.get_or_init(|| Mutex::new(None))
}

fn own_map_context(map_id: Option<&str>) -> Option<mtc_vision::OwnMapContext> {
    let id = map_id?;
    map_catalog()
        .into_iter()
        .find(|m| m.id == id)
        .map(|m| mtc_vision::OwnMapContext {
            map_id: m.id,
            width_meters: m.size_game_meters,
            height_meters: m.height_game_meters.unwrap_or(m.size_game_meters),
        })
}

/// Own position from the white player arrow on the minimap or the open map.
/// `mapId` is the map chosen in the app (the minimap has no "Grid size" label).
#[tauri::command]
pub async fn read_own_position(
    app: tauri::AppHandle,
    handle: i64,
    backend: CaptureBackend,
    map_id: Option<String>,
) -> Result<mtc_vision::OwnPositionReading, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let frame = capture_frame(&app, handle, backend, std::time::Duration::from_millis(500))?;
        let context = own_map_context(map_id.as_deref());
        let mut reading =
            mtc_vision::read_own_position(&frame.image, frame.captured_at_ms, context.as_ref());
        if map_id.is_some() && context.is_none() {
            reading
                .warnings
                .push("Wybrana mapa nie występuje w katalogu — brak współrzędnych świata".into());
        }
        let mut previous = previous_own().lock().map_err(|e| e.to_string())?;
        let prior = previous
            .as_ref()
            .filter(|(h, _)| *h == handle)
            .map(|(_, p)| p);
        mtc_vision::stabilize_own_position(&mut reading, prior);
        *previous = Some((handle, reading.clone()));
        Ok(reading)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Offline variant for saved frames or recordings (no stabilization).
#[tauri::command]
pub async fn read_own_position_file(
    path: String,
    map_id: Option<String>,
) -> Result<mtc_vision::OwnPositionReading, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let context = own_map_context(map_id.as_deref());
        Ok(mtc_vision::read_own_position(
            &read_image(&path)?,
            now_ms(),
            context.as_ref(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pixel of the analysed frame -> map fraction, sector and world meters.
#[tauri::command]
pub fn mtc_map_point(
    reading: MtcMapReading,
    point: mtc_maps::Point,
) -> Result<mtc_vision::MapGridPoint, String> {
    let grid = reading.grid.ok_or("Brak wykrytej siatki mapy")?;
    let size = reading.map_width_meters.zip(reading.map_height_meters);
    mtc_vision::map_grid_point(&grid, size, point)
        .ok_or_else(|| "Punkt leży poza siatką mapy".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_map_context_uses_catalog_size_and_rejects_unknown_maps() {
        let rohkstov = own_map_context(Some("rohkstov")).unwrap();
        assert!((rohkstov.width_meters - 5398.56).abs() < 0.01);
        assert!(own_map_context(Some("no_such_map")).is_none());
        assert!(own_map_context(None).is_none());
    }

    #[test]
    fn catalog_contains_zone_11_and_weapon_names_include_ub32() {
        assert!(map_catalog().iter().any(|m| m.id == "zone_11"));
        assert!(known_weapon_names().iter().any(|n| n == "UB-32"));
        assert!(known_weapon_names().iter().any(|n| n == "S-5K1"));
    }
}
