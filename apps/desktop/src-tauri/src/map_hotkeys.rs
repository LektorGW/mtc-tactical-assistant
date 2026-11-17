//! Global hotkeys "point at the in-game map": the current mouse cursor position
//! over the open MTC map (key M) becomes the target or own position. Only the
//! cursor position and the visible window pixels are read; nothing is clicked,
//! typed or sent to the game.
use crate::{capture_frame, now_ms};
use mtc_capture::CaptureBackend;
use mtc_maps::Point;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const MAP_POINT_EVENT: &str = "mtc-map-point-picked";
/// Shortcuts owned by the main window/overlay; map hotkeys must not reuse them.
const RESERVED: [&str; 3] = ["ctrl+shift+o", "ctrl+shift+i", "ctrl+shift+m"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapPointRole {
    Target,
    Own,
}

#[derive(Default)]
struct HotkeyState {
    handle: Option<i64>,
    backend: Option<CaptureBackend>,
    registered: Vec<(Shortcut, MapPointRole)>,
}

fn state() -> &'static Mutex<HotkeyState> {
    static STATE: OnceLock<Mutex<HotkeyState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HotkeyState::default()))
}

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapHotkeyRequest {
    pub enabled: bool,
    pub handle: Option<i64>,
    pub backend: CaptureBackend,
    /// e.g. "Ctrl+Shift+T"
    pub target_shortcut: String,
    /// e.g. "Ctrl+Shift+G"
    pub own_shortcut: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapHotkeyStatus {
    pub enabled: bool,
    pub target_shortcut: String,
    pub own_shortcut: String,
    pub target_registered: bool,
    pub own_registered: bool,
    pub errors: Vec<String>,
}

/// Payload of `mtc-map-point-picked`. Without a usable point `ok` is false and
/// `error` explains why; fraction/world/sector are then absent.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapPointPicked {
    pub role: MapPointRole,
    pub ok: bool,
    pub error: Option<String>,
    /// 0–1 from the west (x) and north (y) grid edge.
    pub fraction: Option<Point>,
    /// Meters east of the west edge and north of the south edge (identified map only).
    pub world: Option<Point>,
    pub sector: Option<String>,
    pub map_id: Option<String>,
    /// `ready` | `ambiguous` | `unidentified` | `blocked`.
    pub map_status: Option<mtc_vision::MtcMapStatus>,
    pub candidates: Vec<String>,
    pub grid_size_meters: Option<f64>,
    /// Source time of the analysed frame (Unix ms).
    pub timestamp_ms: u64,
    /// Cursor in physical client pixels of the game window.
    pub cursor: Option<Point>,
}

impl MapPointPicked {
    fn failure(role: MapPointRole, error: impl Into<String>, timestamp_ms: u64) -> Self {
        Self {
            role,
            ok: false,
            error: Some(error.into()),
            fraction: None,
            world: None,
            sector: None,
            map_id: None,
            map_status: None,
            candidates: Vec::new(),
            grid_size_meters: None,
            timestamp_ms,
            cursor: None,
        }
    }
}

/// Pure part: map reading + cursor pixel -> event payload.
pub fn map_point_from_reading(
    role: MapPointRole,
    reading: &mtc_vision::MtcMapReading,
    cursor: Point,
) -> MapPointPicked {
    let mut picked = MapPointPicked::failure(role, "", reading.timestamp_ms);
    picked.cursor = Some(cursor);
    picked.map_status = Some(reading.status);
    picked.map_id = reading.map_id.clone();
    picked.candidates = reading.candidates.iter().map(|c| c.id.clone()).collect();
    picked.grid_size_meters = reading.grid_size_meters;
    let Some(grid) = reading.grid else {
        picked.error =
            Some("Nie widać mapy z siatką 9 × 9 — otwórz mapę w grze klawiszem M".into());
        return picked;
    };
    let size = reading.map_width_meters.zip(reading.map_height_meters);
    match mtc_vision::map_grid_point(&grid, size, cursor) {
        Some(point) => {
            picked.ok = true;
            picked.error = None;
            picked.fraction = Some(point.fraction);
            picked.world = point.world;
            picked.sector = Some(point.sector);
        }
        None => picked.error = Some("Kursor jest poza siatką mapy".into()),
    }
    picked
}

fn pick(app: &tauri::AppHandle, role: MapPointRole) -> MapPointPicked {
    let (handle, backend) = {
        let state = state().lock().unwrap_or_else(|e| e.into_inner());
        (state.handle, state.backend)
    };
    let (Some(handle), Some(backend)) = (handle, backend) else {
        return MapPointPicked::failure(role, "Nie wybrano okna gry w aplikacji", now_ms());
    };
    let (x, y, inside) = match mtc_capture::cursor_client_position(handle) {
        Ok(position) => position,
        Err(error) => return MapPointPicked::failure(role, error.to_string(), now_ms()),
    };
    if !inside {
        return MapPointPicked::failure(role, "Kursor jest poza oknem gry", now_ms());
    }
    let frame = match capture_frame(app, handle, backend, std::time::Duration::from_millis(250)) {
        Ok(frame) => frame,
        Err(error) => return MapPointPicked::failure(role, error, now_ms()),
    };
    let reading = mtc_vision::read_mtc_full_map(
        &frame.image,
        frame.captured_at_ms,
        &crate::mtc_hud::map_catalog(),
    );
    map_point_from_reading(
        role,
        &reading,
        Point {
            x: f64::from(x),
            y: f64::from(y),
        },
    )
}

fn on_hotkey(app: &tauri::AppHandle, role: MapPointRole) {
    // One analysis at a time; repeated presses during OCR are ignored.
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let picked = pick(&app, role);
        BUSY.store(false, Ordering::Release);
        let _ = app.emit(MAP_POINT_EVENT, picked);
    });
}

fn parse_shortcut(text: &str) -> Result<Shortcut, String> {
    let normalized = text.trim().to_lowercase().replace(' ', "");
    if RESERVED.contains(&normalized.as_str()) {
        return Err(format!("Skrót {text} jest zajęty przez nakładkę/panel"));
    }
    normalized
        .parse::<Shortcut>()
        .map_err(|e| format!("Niepoprawny skrót {text}: {e}"))
}

/// (Re)registers both hotkeys and remembers the selected window. Disabling
/// unregisters them. Call again whenever the window or backend changes.
#[tauri::command]
pub fn configure_map_point_hotkeys(
    app: tauri::AppHandle,
    request: MapHotkeyRequest,
) -> Result<MapHotkeyStatus, String> {
    let mut state = state().lock().map_err(|e| e.to_string())?;
    for (shortcut, _) in state.registered.drain(..) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
    state.handle = request.handle;
    state.backend = Some(request.backend);
    let mut status = MapHotkeyStatus {
        enabled: request.enabled,
        target_shortcut: request.target_shortcut.clone(),
        own_shortcut: request.own_shortcut.clone(),
        target_registered: false,
        own_registered: false,
        errors: Vec::new(),
    };
    if !request.enabled {
        return Ok(status);
    }
    let target = parse_shortcut(&request.target_shortcut);
    let own = parse_shortcut(&request.own_shortcut);
    if let (Ok(a), Ok(b)) = (&target, &own) {
        if a == b {
            status
                .errors
                .push("Skróty celu i własnej pozycji muszą się różnić".into());
            return Ok(status);
        }
    }
    for (parsed, role) in [(target, MapPointRole::Target), (own, MapPointRole::Own)] {
        let shortcut = match parsed {
            Ok(shortcut) => shortcut,
            Err(error) => {
                status.errors.push(error);
                continue;
            }
        };
        let result = app
            .global_shortcut()
            .on_shortcut(shortcut, move |app, _, event| {
                if event.state() == ShortcutState::Pressed {
                    on_hotkey(app, role);
                }
            });
        match result {
            Ok(()) => {
                state.registered.push((shortcut, role));
                match role {
                    MapPointRole::Target => status.target_registered = true,
                    MapPointRole::Own => status.own_registered = true,
                }
            }
            Err(error) => status.errors.push(format!("Skrót niedostępny: {error}")),
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_vision::{MapGrid, MtcMapReading, MtcMapStatus};

    fn reading(grid: Option<MapGrid>) -> MtcMapReading {
        MtcMapReading {
            schema_version: 1,
            status: if grid.is_some() {
                MtcMapStatus::Ready
            } else {
                MtcMapStatus::Blocked
            },
            timestamp_ms: 77,
            frame_width: 1920,
            frame_height: 1080,
            grid,
            grid_size_meters: Some(324.0),
            grid_size_ocr: vec![],
            map_id: Some("zone_11".into()),
            candidates: vec![],
            map_width_meters: Some(2918.52),
            map_height_meters: Some(2918.52),
            calibration: None,
            warnings: vec![],
        }
    }

    fn grid() -> MapGrid {
        MapGrid {
            left: 672.0,
            top: 252.0,
            right: 1248.0,
            bottom: 828.0,
            cell_width: 64.0,
            cell_height: 64.0,
            columns: 9,
            rows: 9,
            line_strength: 0.8,
            frame_width: 1920,
            frame_height: 1080,
        }
    }

    #[test]
    fn cursor_over_grid_gives_fraction_sector_and_world() {
        let picked = map_point_from_reading(
            MapPointRole::Target,
            &reading(Some(grid())),
            Point { x: 832.0, y: 668.0 },
        );
        assert!(picked.ok);
        assert_eq!(picked.sector.as_deref(), Some("C7"));
        assert_eq!(
            picked.fraction,
            Some(Point {
                x: 0.2777777777777778,
                y: 0.7222222222222222
            })
        );
        let world = picked.world.unwrap();
        assert!((world.x - 2918.52 * 160.0 / 576.0).abs() < 1e-6);
        assert_eq!(picked.map_id.as_deref(), Some("zone_11"));
        assert_eq!(picked.timestamp_ms, 77);
    }

    #[test]
    fn closed_map_or_cursor_outside_grid_never_produces_a_point() {
        let closed = map_point_from_reading(
            MapPointRole::Own,
            &reading(None),
            Point { x: 900.0, y: 500.0 },
        );
        assert!(
            !closed.ok
                && closed.fraction.is_none()
                && closed.error.unwrap().contains("klawiszem M")
        );
        let outside = map_point_from_reading(
            MapPointRole::Own,
            &reading(Some(grid())),
            Point { x: 100.0, y: 100.0 },
        );
        assert!(!outside.ok && outside.sector.is_none());
    }

    #[test]
    fn shortcut_parsing_rejects_reserved_and_invalid_keys() {
        assert!(parse_shortcut("Ctrl+Shift+T").is_ok());
        assert!(parse_shortcut("Ctrl+Shift+G").is_ok());
        assert!(parse_shortcut("Ctrl+Shift+M").is_err());
        assert!(parse_shortcut("Ctrl+Shift+NotAKey").is_err());
    }
}
