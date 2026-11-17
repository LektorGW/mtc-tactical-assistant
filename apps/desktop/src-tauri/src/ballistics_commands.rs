//! Tauri commands for the salvo forecast and the beta moving-target intercept
//! with drag/rocket models. Pure wrappers: all validation lives in Rust crates.
use mtc_profiles::WeaponProfile;

#[tauri::command]
pub async fn salvo_pattern(
    profile: WeaponProfile,
    input: mtc_ballistics::SalvoInput,
) -> Result<mtc_ballistics::SalvoPattern, String> {
    tauri::async_runtime::spawn_blocking(move || mtc_ballistics::salvo_pattern(&profile, &input))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn dynamic_intercept(
    profile: WeaponProfile,
    input: mtc_ballistics::DynamicInterceptInput,
) -> Result<mtc_ballistics::DynamicInterceptSolution, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mtc_ballistics::solve_dynamic_intercept(&profile, &input)
    })
    .await
    .map_err(|e| e.to_string())?
}

macro_rules! blocking {
    ($body:expr) => {
        tauri::async_runtime::spawn_blocking(move || $body)
            .await
            .map_err(|e| e.to_string())?
    };
}

#[tauri::command]
pub async fn range_card(
    profile: WeaponProfile,
    input: mtc_ballistics::RangeCardInput,
) -> Result<mtc_ballistics::RangeCard, String> {
    blocking!(mtc_ballistics::range_card(&profile, &input))
}

#[tauri::command]
pub fn lead_estimate(
    input: mtc_ballistics::LeadInput,
) -> Result<mtc_ballistics::LeadEstimate, String> {
    mtc_ballistics::lead(&input)
}

#[tauri::command]
pub async fn lead_iterated(
    profile: WeaponProfile,
    input: mtc_ballistics::LeadProfileInput,
) -> Result<mtc_ballistics::LeadEstimate, String> {
    blocking!(mtc_ballistics::lead_iterated(&profile, &input))
}

#[tauri::command]
pub async fn quick_aim(
    profile: WeaponProfile,
    input: mtc_ballistics::QuickAimInput,
) -> Result<mtc_ballistics::QuickAim, String> {
    blocking!(mtc_ballistics::quick_aim(&profile, &input))
}

#[tauri::command]
pub async fn fit_control_shots(
    profile: WeaponProfile,
    input: mtc_ballistics::ControlShotInput,
) -> Result<mtc_ballistics::ControlShotFit, String> {
    blocking!(mtc_ballistics::fit_control_shots(&profile, &input))
}

#[tauri::command]
pub async fn air_to_ground_impact(
    profile: WeaponProfile,
    input: mtc_ballistics::AirToGroundInput,
) -> Result<mtc_ballistics::AirToGroundImpact, String> {
    blocking!(mtc_ballistics::air_to_ground_impact(&profile, &input))
}

#[tauri::command]
pub async fn air_to_ground_solve(
    profile: WeaponProfile,
    input: mtc_ballistics::AirToGroundSolveInput,
) -> Result<mtc_ballistics::AirToGroundImpact, String> {
    blocking!(mtc_ballistics::air_to_ground_solve(&profile, &input))
}

/// Quick range hotkeys: Numpad + / − / 0. Registered only while the quick
/// mode is enabled, so the game keeps these keys otherwise. They only change
/// the assumed range inside the application; nothing is sent to the game.
pub fn quick_range_shortcuts() -> [(tauri_plugin_global_shortcut::Shortcut, &'static str); 3] {
    use tauri_plugin_global_shortcut::{Code, Shortcut};
    [
        (Shortcut::new(None, Code::NumpadAdd), "next"),
        (Shortcut::new(None, Code::NumpadSubtract), "previous"),
        (Shortcut::new(None, Code::Numpad0), "close"),
    ]
}

pub fn quick_range_action(
    shortcut: &tauri_plugin_global_shortcut::Shortcut,
) -> Option<&'static str> {
    quick_range_shortcuts()
        .into_iter()
        .find(|(candidate, _)| candidate == shortcut)
        .map(|(_, action)| action)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub enabled: bool,
    pub registered: Vec<String>,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn quick_range_hotkeys(app: tauri::AppHandle, enabled: bool) -> HotkeyStatus {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let manager = app.global_shortcut();
    let mut status = HotkeyStatus {
        enabled,
        registered: Vec::new(),
        errors: Vec::new(),
    };
    for (shortcut, action) in quick_range_shortcuts() {
        let label = match action {
            "next" => "Numpad +",
            "previous" => "Numpad −",
            _ => "Numpad 0",
        };
        let registered = manager.is_registered(shortcut);
        let outcome = match (enabled, registered) {
            (true, false) => manager.register(shortcut),
            (false, true) => manager.unregister(shortcut),
            _ => Ok(()),
        };
        match outcome {
            Err(error) => status.errors.push(format!("{label}: {error}")),
            Ok(()) if enabled => status.registered.push(label.into()),
            Ok(()) => {}
        }
    }
    status
}
