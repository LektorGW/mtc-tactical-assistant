//! Direct fire (range card, lead) and unguided air-to-ground rockets.
//!
//! Numbers for the player only: no target detection, no tracking and no
//! on-screen lead markers. All trajectories use the profile's own model
//! (vacuum unless the profile carries locally calibrated drag/rocket data).
use crate::dynamics::{self, norm, Budget, Model, State};
use mtc_profiles::{DynamicsModel, WeaponProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectFireOptions {
    pub step_seconds: f64,
    pub max_time: f64,
    pub max_work_steps: u32,
}
impl Default for DirectFireOptions {
    fn default() -> Self {
        Self {
            step_seconds: 0.002,
            max_time: 60.0,
            max_work_steps: 5_000_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RangeCardInput {
    pub unit: String,
    /// Horizontal ranges, profile unit; 1–64 entries.
    pub ranges: Vec<f64>,
    /// Sight (line of sight) above the bore axis at the muzzle; default 0.
    #[serde(default)]
    pub sight_height: Option<f64>,
    /// Range at which the trajectory crosses the line of sight; none = bore
    /// parallel to the line of sight (no zeroing).
    #[serde(default)]
    pub zero_range: Option<f64>,
    #[serde(default)]
    pub options: DirectFireOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeCardRow {
    pub range: f64,
    pub reachable: bool,
    /// Flight time with the zeroed sight setting.
    pub time_of_flight: Option<f64>,
    /// Positive = trajectory below the line of sight (hold over), profile unit.
    pub drop_below_line_of_sight: Option<f64>,
    pub velocity_at_range: Option<f64>,
    /// Bore elevation needed to hit the line of sight at this range.
    pub elevation_mrad: Option<f64>,
    pub elevation_deg: Option<f64>,
    /// Required elevation minus the zero setting (aim higher when positive).
    pub hold_over_mrad: Option<f64>,
    pub hold_over_deg: Option<f64>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeCard {
    pub rows: Vec<RangeCardRow>,
    pub unit: String,
    pub model: String,
    pub status: String,
    pub sight_height: f64,
    pub zero_range: Option<f64>,
    pub zero_elevation_mrad: f64,
    pub warnings: Vec<String>,
    pub assumptions: Vec<String>,
    pub steps_used: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeadInput {
    pub range: f64,
    /// Target speed in the profile unit per second.
    pub target_speed: f64,
    /// 0° = moving directly away, 90° = crossing to the right, 180° = towards
    /// the shooter, 270° = crossing to the left.
    pub course_relative_deg: f64,
    pub time_of_flight: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeadProfileInput {
    pub range: f64,
    pub target_speed: f64,
    pub course_relative_deg: f64,
    pub unit: String,
    #[serde(default)]
    pub sight_height: Option<f64>,
    #[serde(default)]
    pub options: DirectFireOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadEstimate {
    /// Positive = aim to the right of the target, profile unit.
    pub lateral_offset: f64,
    /// Positive = target moves away during the flight.
    pub radial_offset: f64,
    pub future_range: f64,
    pub lead_mrad: f64,
    pub lead_deg: f64,
    pub time_of_flight: f64,
    /// Bore elevation for the future range (profile-based estimate only).
    pub elevation_mrad: Option<f64>,
    pub iterations: u32,
    pub converged: bool,
    pub assumptions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AirToGroundInput {
    /// Launcher height above the target point, profile unit (> 0).
    pub altitude_above_target: f64,
    /// Airspeed along the flight path, profile unit per second (≥ 0).
    pub airspeed: f64,
    /// Flight path below the horizon, degrees (negative = climbing).
    pub dive_angle_deg: f64,
    pub unit: String,
    #[serde(default)]
    pub options: DirectFireOptions,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AirToGroundSolveInput {
    pub altitude_above_target: f64,
    pub airspeed: f64,
    /// Horizontal distance from the launcher to the target, profile unit.
    pub horizontal_distance: f64,
    pub unit: String,
    #[serde(default)]
    pub options: DirectFireOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AirToGroundImpact {
    pub dive_angle_deg: f64,
    pub horizontal_distance: f64,
    pub slant_range: f64,
    pub time_of_flight: f64,
    pub impact_speed: f64,
    /// Impact path below the horizon, degrees.
    pub impact_angle_deg: f64,
    /// Where the launcher axis (line of sight) meets the target plane.
    pub boresight_ground_distance: Option<f64>,
    /// Impact minus boresight ground point; negative = rockets fall short.
    pub aim_offset_along_ground: Option<f64>,
    /// Angle of the impact point below the launcher axis: lower the aiming
    /// mark by this much and put the mark on the target.
    pub depression_below_boresight_mrad: f64,
    pub depression_below_boresight_deg: f64,
    pub model: String,
    pub status: String,
    pub warnings: Vec<String>,
    pub assumptions: Vec<String>,
}

fn status(profile: &WeaponProfile) -> String {
    if profile.calibrated {
        "beta"
    } else {
        "experimental"
    }
    .into()
}
fn model_name(profile: &WeaponProfile) -> String {
    match profile.dynamics_model() {
        DynamicsModel::Vacuum => "vacuum",
        DynamicsModel::Drag => "drag",
        DynamicsModel::Rocket => "rocket",
    }
    .into()
}
fn common_warnings(profile: &WeaponProfile) -> Vec<String> {
    let mut warnings = Vec::new();
    if profile.dynamics_model() == DynamicsModel::Vacuum {
        warnings.push("Model uproszczony: profil nie ma współczynników oporu ani napędu — obliczenie bez oporu powietrza. Pocisk (np. PG-7VR, PG-9) może mieć napęd rakietowy lub opór; wynik wymaga strzałów kontrolnych".into());
    }
    if let Some(observed) = &profile.observed_specs {
        warnings.push(format!(
            "Prędkość {} z {} ({}): {}",
            observed.muzzle_velocity,
            if observed.screen == "loadout" {
                "ekranu Loadout"
            } else {
                "wpisu ręcznego"
            },
            observed.observed_at,
            observed.velocity_unit_assumption
        ));
        warnings.push(format!("Grawitacja: {}", observed.gravity_source));
    }
    if profile.source_metadata.is_some() && !profile.calibrated {
        warnings.push("Dane społeczności bez lokalnej kalibracji".into());
    }
    warnings
}
fn check_options(options: &DirectFireOptions) -> Result<(), String> {
    dynamics::validate_time_and_step(options.max_time, options.step_seconds)?;
    if !(1..=20_000_000).contains(&options.max_work_steps) {
        return Err("Niepoprawny limit kroków RK4".into());
    }
    Ok(())
}

/// Flat-fire shooting in the vertical north/up plane from the muzzle.
struct Flat<'a> {
    model: Model,
    options: &'a DirectFireOptions,
    budget: Budget,
    limits: (f64, f64),
}

impl Flat<'_> {
    fn shot(&mut self, elevation: f64, range: f64) -> Result<Option<(f64, State)>, String> {
        let launch = [0.0, elevation.cos(), elevation.sin()];
        self.model.state_at_forward_range(
            launch,
            launch.map(|v| v * self.model.speed),
            range,
            self.options.max_time,
            self.options.step_seconds,
            &mut self.budget,
        )
    }
    fn height_error(&mut self, elevation: f64, range: f64, height: f64) -> Result<f64, String> {
        Ok(self
            .shot(elevation, range)?
            .map_or(f64::NEG_INFINITY, |(_, s)| s.position[2] - height))
    }
    /// Lowest (flat-fire) elevation that reaches `height` at `range`.
    fn required(&mut self, range: f64, height: f64) -> Result<Option<f64>, String> {
        let (v, g) = (self.model.speed, self.model.gravity);
        let disc = v.powi(4) - g * (g * range * range + 2.0 * height * v * v);
        let guess = if disc >= 0.0 {
            ((v * v - disc.sqrt()) / (g * range)).atan()
        } else {
            0.5
        };
        let (min, max) = (self.limits.0, self.limits.1.min(60_f64.to_radians()));
        let step = 1_f64.to_radians();
        let mut low = (guess - step).clamp(min, max);
        while self.height_error(low, range, height)? >= 0.0 {
            if low <= min {
                return Ok(None);
            }
            low = (low - 4.0 * step).max(min);
        }
        let mut high = low;
        loop {
            high = (high + step).min(max);
            if self.height_error(high, range, height)? >= 0.0 {
                break;
            }
            if high >= max {
                return Ok(None);
            }
            low = high;
        }
        for _ in 0..64 {
            let middle = 0.5 * (low + high);
            let error = self.height_error(middle, range, height)?;
            if error.abs() <= 1e-9 * range.max(1.0) {
                return Ok(Some(middle));
            }
            if error < 0.0 {
                low = middle;
            } else {
                high = middle;
            }
        }
        Ok(Some(0.5 * (low + high)))
    }
}

fn flat<'a>(
    profile: &WeaponProfile,
    unit: &str,
    options: &'a DirectFireOptions,
) -> Result<Flat<'a>, String> {
    let model = Model::from_profile(profile, unit)?;
    check_options(options)?;
    Ok(Flat {
        model,
        options,
        budget: Budget {
            used: 0,
            maximum: options.max_work_steps,
        },
        limits: (
            profile.min_elevation_deg.to_radians(),
            profile.max_elevation_deg.to_radians(),
        ),
    })
}

pub fn range_card(profile: &WeaponProfile, input: &RangeCardInput) -> Result<RangeCard, String> {
    let sight = input.sight_height.unwrap_or(0.0);
    if input.ranges.is_empty()
        || input.ranges.len() > 64
        || input
            .ranges
            .iter()
            .any(|r| !r.is_finite() || *r <= 0.0 || *r > 1e5)
        || !sight.is_finite()
        || sight.abs() > 100.0
        || input
            .zero_range
            .is_some_and(|z| !z.is_finite() || z <= 0.0 || z > 1e5)
    {
        return Err("Wymagane 1–64 dodatnie dystanse, wysokość przyrządów do 100 i dodatni dystans zerowania".into());
    }
    let mut solver = flat(profile, &input.unit, &input.options)?;
    let zero = match input.zero_range {
        Some(range) => solver
            .required(range, sight)?
            .ok_or("Dystans zerowania poza zasięgiem modelu w limitach elewacji")?,
        None => 0.0,
    };
    let mut rows = Vec::with_capacity(input.ranges.len());
    for &range in &input.ranges {
        let path = solver.shot(zero, range)?;
        let required = solver.required(range, sight)?;
        let to_mrad = |angle: f64| angle * 1000.0;
        rows.push(RangeCardRow {
            range,
            reachable: required.is_some(),
            time_of_flight: path.map(|(t, _)| t),
            drop_below_line_of_sight: path.map(|(_, s)| sight - s.position[2]),
            velocity_at_range: path.map(|(_, s)| norm(s.velocity)),
            elevation_mrad: required.map(to_mrad),
            elevation_deg: required.map(f64::to_degrees),
            hold_over_mrad: required.map(|r| to_mrad(r - zero)),
            hold_over_deg: required.map(|r| (r - zero).to_degrees()),
            note: required
                .is_none()
                .then(|| "Poza zasięgiem modelu w limitach elewacji profilu".into()),
        });
    }
    let mut assumptions = vec![
        "Ogień bezpośredni: cel na wysokości linii celowania, teren płaski; dystans z gry traktowany jako poziomy".to_string(),
        format!("Model {}; grawitacja {} {}/s²; prędkość {} {}/s", model_name(profile), solver.model.gravity, input.unit, solver.model.speed, input.unit),
        "mrad = 1/1000 radiana (≈ 1 jednostka na 1000 jednostek dystansu); poprawka dodatnia = celuj wyżej".to_string(),
        "Bez wiatru, ruchu strzelca i rozrzutu; tylko liczby dla gracza, bez śledzenia celów".to_string(),
    ];
    if input.zero_range.is_none() {
        assumptions.push("Brak zerowania: oś lufy równoległa do linii celowania".into());
    }
    Ok(RangeCard {
        rows,
        unit: input.unit.clone(),
        model: model_name(profile),
        status: status(profile),
        sight_height: sight,
        zero_range: input.zero_range,
        zero_elevation_mrad: zero * 1000.0,
        warnings: common_warnings(profile),
        assumptions,
        steps_used: solver.budget.used,
    })
}

fn validate_lead(range: f64, speed: f64, course: f64) -> Result<(), String> {
    if !range.is_finite()
        || range <= 0.0
        || range > 1e5
        || !speed.is_finite()
        || speed < 0.0
        || speed > 1e4
        || !course.is_finite()
        || course.abs() > 1e4
    {
        return Err(
            "Wymagany dodatni dystans, nieujemna prędkość celu i kurs względny w stopniach".into(),
        );
    }
    Ok(())
}
fn lead_geometry(range: f64, speed: f64, course_deg: f64, time: f64) -> (f64, f64, f64) {
    let course = course_deg.to_radians();
    let lateral = speed * course.sin() * time;
    let radial = speed * course.cos() * time;
    (lateral, radial, (range + radial).hypot(lateral))
}

/// First-order lead for a constant-velocity target using the given flight time.
pub fn lead(input: &LeadInput) -> Result<LeadEstimate, String> {
    validate_lead(input.range, input.target_speed, input.course_relative_deg)?;
    if !input.time_of_flight.is_finite()
        || input.time_of_flight <= 0.0
        || input.time_of_flight > 600.0
    {
        return Err("Wymagany dodatni czas lotu do 600 s".into());
    }
    let (lateral, radial, future) = lead_geometry(
        input.range,
        input.target_speed,
        input.course_relative_deg,
        input.time_of_flight,
    );
    let angle = lateral.atan2(input.range + radial);
    Ok(LeadEstimate {
        lateral_offset: lateral,
        radial_offset: radial,
        future_range: future,
        lead_mrad: angle * 1000.0,
        lead_deg: angle.to_degrees(),
        time_of_flight: input.time_of_flight,
        elevation_mrad: None,
        iterations: 0,
        converged: true,
        assumptions: vec![
            "Cel o stałej prędkości i kursie; czas lotu podany dla bieżącego dystansu (przybliżenie pierwszego rzędu)".into(),
            "Kurs względny: 0° oddala się, 90° w prawo, 180° zbliża się, 270° w lewo; dodatnie wyprzedzenie = celuj w prawo".into(),
        ],
    })
}

/// Lead with the flight time re-evaluated at the predicted range (fixed point).
pub fn lead_iterated(
    profile: &WeaponProfile,
    input: &LeadProfileInput,
) -> Result<LeadEstimate, String> {
    validate_lead(input.range, input.target_speed, input.course_relative_deg)?;
    let sight = input.sight_height.unwrap_or(0.0);
    if !sight.is_finite() || sight.abs() > 100.0 {
        return Err("Niepoprawna wysokość przyrządów".into());
    }
    let mut solver = flat(profile, &input.unit, &input.options)?;
    let flight = |solver: &mut Flat, range: f64| -> Result<(f64, f64), String> {
        let elevation = solver
            .required(range, sight)?
            .ok_or("Przewidywany dystans poza zasięgiem modelu")?;
        let (time, _) = solver
            .shot(elevation, range)?
            .ok_or("Pocisk nie osiąga przewidywanego dystansu")?;
        Ok((time, elevation))
    };
    let (mut time, mut elevation) = flight(&mut solver, input.range)?;
    let mut iterations = 0;
    let mut converged = false;
    while iterations < 30 {
        iterations += 1;
        let (_, _, future) = lead_geometry(
            input.range,
            input.target_speed,
            input.course_relative_deg,
            time,
        );
        let (next, next_elevation) = flight(&mut solver, future)?;
        let change = (next - time).abs();
        time = next;
        elevation = next_elevation;
        if change <= 1e-7 * time.max(1.0) {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err("Iteracja czasu lotu nie zbiega się (cel zbyt szybki względem pocisku)".into());
    }
    let mut estimate = lead(&LeadInput {
        range: input.range,
        target_speed: input.target_speed,
        course_relative_deg: input.course_relative_deg,
        time_of_flight: time,
    })?;
    estimate.elevation_mrad = Some(elevation * 1000.0);
    estimate.iterations = iterations;
    estimate.converged = true;
    estimate.assumptions[0] = format!(
        "Cel o stałej prędkości i kursie; czas lotu przeliczony dla przewidywanego dystansu ({iterations} iteracji, model {})",
        model_name(profile)
    );
    estimate.assumptions.extend(common_warnings(profile));
    Ok(estimate)
}

struct Air<'a> {
    model: Model,
    options: &'a DirectFireOptions,
    budget: Budget,
    altitude: f64,
    airspeed: f64,
}

impl Air<'_> {
    fn impact(&mut self, dive_deg: f64) -> Result<Option<(f64, State)>, String> {
        let dive = dive_deg.to_radians();
        let launch = [0.0, dive.cos(), -dive.sin()];
        let speed = self.model.speed + self.airspeed;
        let crossings = self.model.plane_crossings(
            launch,
            launch.map(|v| v * speed),
            -self.altitude,
            self.options.max_time,
            self.options.step_seconds,
            &mut self.budget,
        )?;
        Ok(crossings
            .into_iter()
            .find(|(_, state)| state.velocity[2] < 0.0))
    }
}

fn air<'a>(
    profile: &WeaponProfile,
    unit: &str,
    altitude: f64,
    airspeed: f64,
    options: &'a DirectFireOptions,
) -> Result<Air<'a>, String> {
    let model = Model::from_profile(profile, unit)?;
    check_options(options)?;
    if !altitude.is_finite()
        || altitude <= 0.0
        || altitude > 1e5
        || !airspeed.is_finite()
        || !(0.0..=2000.0).contains(&airspeed)
    {
        return Err(
            "Wymagana dodatnia wysokość nad celem i prędkość lotu od 0 do 2000 jednostek/s".into(),
        );
    }
    Ok(Air {
        model,
        options,
        budget: Budget {
            used: 0,
            maximum: options.max_work_steps,
        },
        altitude,
        airspeed,
    })
}

fn describe(
    profile: &WeaponProfile,
    air: &Air,
    dive_deg: f64,
    time: f64,
    state: State,
    unit: &str,
) -> AirToGroundImpact {
    let distance = state.position[1];
    let dive = dive_deg.to_radians();
    let boresight = (dive > 0.0).then(|| air.altitude / dive.tan());
    let depression = air.altitude.atan2(distance) - dive;
    let speed = norm(state.velocity);
    let mut assumptions = vec![
        "Rakieta startuje wzdłuż wektora prędkości lotu (bez kąta natarcia i ślizgu); prędkość lotu dodana do prędkości rakiety — dziedziczenie jest założeniem do weryfikacji".to_string(),
        format!("Wysokość nad celem {} {unit}, płaski teren między wyrzutnią a celem, bez wiatru", air.altitude),
        "Punkt celowania: obniż znak celowania o kąt depresji względem osi wyrzutni i nałóż go na cel; tylko liczby, bez śledzenia celów".to_string(),
    ];
    if profile.dynamics_model() == DynamicsModel::Rocket {
        assumptions.push("Napęd działa wzdłuż osi wyrzutni w chwili startu".into());
    }
    AirToGroundImpact {
        dive_angle_deg: dive_deg,
        horizontal_distance: distance,
        slant_range: distance.hypot(air.altitude),
        time_of_flight: time,
        impact_speed: speed,
        impact_angle_deg: (-state.velocity[2]).atan2(state.velocity[1]).to_degrees(),
        boresight_ground_distance: boresight,
        aim_offset_along_ground: boresight.map(|b| distance - b),
        depression_below_boresight_mrad: depression * 1000.0,
        depression_below_boresight_deg: depression.to_degrees(),
        model: model_name(profile),
        status: status(profile),
        warnings: common_warnings(profile),
        assumptions,
    }
}

fn check_dive(dive: f64) -> Result<(), String> {
    if !dive.is_finite() || !(-30.0..=89.0).contains(&dive) {
        return Err("Kąt nurkowania od −30° (wznoszenie) do 89°".into());
    }
    Ok(())
}

/// Forward problem: where do rockets fired at this dive angle hit the target plane?
pub fn air_to_ground_impact(
    profile: &WeaponProfile,
    input: &AirToGroundInput,
) -> Result<AirToGroundImpact, String> {
    check_dive(input.dive_angle_deg)?;
    let mut solver = air(
        profile,
        &input.unit,
        input.altitude_above_target,
        input.airspeed,
        &input.options,
    )?;
    let (time, state) = solver
        .impact(input.dive_angle_deg)?
        .ok_or("Rakieta nie osiąga wysokości celu w horyzoncie czasu")?;
    Ok(describe(
        profile,
        &solver,
        input.dive_angle_deg,
        time,
        state,
        &input.unit,
    ))
}

/// Inverse problem: dive angle that puts the impact at the given distance.
pub fn air_to_ground_solve(
    profile: &WeaponProfile,
    input: &AirToGroundSolveInput,
) -> Result<AirToGroundImpact, String> {
    if !input.horizontal_distance.is_finite()
        || input.horizontal_distance <= 0.0
        || input.horizontal_distance > 1e5
    {
        return Err("Wymagany dodatni dystans poziomy do celu".into());
    }
    let mut solver = air(
        profile,
        &input.unit,
        input.altitude_above_target,
        input.airspeed,
        &input.options,
    )?;
    let distance = |solver: &mut Air, dive: f64| -> Result<f64, String> {
        Ok(solver
            .impact(dive)?
            .map_or(f64::INFINITY, |(_, s)| s.position[1]))
    };
    let (mut steep, mut shallow) = (89.0, -30.0);
    let target = input.horizontal_distance;
    if distance(&mut solver, steep)? > target {
        return Err(
            "Cel zbyt blisko dla tej wysokości: nawet stromy lot przenosi rakiety dalej".into(),
        );
    }
    if distance(&mut solver, shallow)? < target {
        return Err("Cel poza zasięgiem w zakresie kątów −30°…89°".into());
    }
    for _ in 0..60 {
        let middle = 0.5 * (steep + shallow);
        let d = distance(&mut solver, middle)?;
        if (d - target).abs() <= 1e-6 * target.max(1.0) {
            steep = middle;
            shallow = middle;
            break;
        }
        if d > target {
            shallow = middle;
        } else {
            steep = middle;
        }
    }
    let dive = 0.5 * (steep + shallow);
    let (time, state) = solver
        .impact(dive)?
        .ok_or("Brak trafienia dla wyznaczonego kąta")?;
    let mut result = describe(profile, &solver, dive, time, state, &input.unit);
    result.assumptions.push(format!(
        "Kąt nurkowania wyznaczony bisekcją dla dystansu {} {}",
        input.horizontal_distance, input.unit
    ));
    Ok(result)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuickAimInput {
    pub unit: String,
    /// Assumed or rangefinder range; `None` = close mode (point-blank range).
    #[serde(default)]
    pub range: Option<f64>,
    #[serde(default)]
    pub sight_height: Option<f64>,
    #[serde(default)]
    pub zero_range: Option<f64>,
    /// |drop| below this (profile unit) means "aim at centre".
    pub center_threshold: f64,
    #[serde(default)]
    pub options: DirectFireOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReticleHold {
    pub mark: String,
    pub mark_range: f64,
    /// Remaining correction with that mark; positive = aim higher.
    pub residual_drop: f64,
    pub residual_mrad: f64,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAim {
    pub range: Option<f64>,
    /// Largest range with |drop| ≤ threshold for the zero setting (≤ 5000).
    pub point_blank_range: Option<f64>,
    pub center_aim: bool,
    pub reachable: bool,
    pub drop_below_line_of_sight: Option<f64>,
    pub hold_over_mrad: Option<f64>,
    pub elevation_mrad: Option<f64>,
    pub time_of_flight: Option<f64>,
    pub reticle: Option<ReticleHold>,
    pub model: String,
    pub status: String,
    pub warnings: Vec<String>,
}

impl Flat<'_> {
    /// One integration with fixed elevation, sampled at increasing ranges.
    fn sweep(&mut self, elevation: f64, ranges: &[f64]) -> Result<Vec<Option<f64>>, String> {
        let launch = [0.0, elevation.cos(), elevation.sin()];
        let mut state = State {
            position: [0.0; 3],
            velocity: launch.map(|v| v * self.model.speed),
        };
        let mut time = 0.0;
        let mut heights = Vec::with_capacity(ranges.len());
        let horizon = self.options.max_time;
        while heights.len() < ranges.len() && horizon - time > horizon.max(1.0) * 1e-12 {
            let plan =
                self.model
                    .step_size(state, launch, time, horizon, self.options.step_seconds)?;
            let next = self
                .model
                .step(state, launch, plan.h, plan.burning, &mut self.budget)?;
            while let Some(&range) = ranges.get(heights.len()) {
                if next.position[1] < range {
                    break;
                }
                // Linear position interpolation inside one short step is only
                // used to locate the point-blank bracket, then refined exactly.
                let span = next.position[1] - state.position[1];
                let w = if span > 0.0 {
                    (range - state.position[1]) / span
                } else {
                    1.0
                };
                heights.push(Some(
                    state.position[2] + w * (next.position[2] - state.position[2]),
                ));
            }
            if next.velocity[1] <= 0.0 {
                break;
            }
            state = next;
            time = plan.next_time;
        }
        heights.resize(ranges.len(), None);
        Ok(heights)
    }
}

fn drop_at(
    solver: &mut Flat,
    elevation: f64,
    range: f64,
    sight: f64,
) -> Result<Option<f64>, String> {
    Ok(solver
        .shot(elevation, range)?
        .map(|(_, s)| sight - s.position[2]))
}

/// Quick single-line aim for tanks and hand-held weapons: drop/hold at the
/// assumed range, or "aim at centre" inside the point-blank range computed
/// from the trajectory (threshold given by the player, not a fixed range).
pub fn quick_aim(profile: &WeaponProfile, input: &QuickAimInput) -> Result<QuickAim, String> {
    let sight = input.sight_height.unwrap_or(0.0);
    if !sight.is_finite()
        || sight.abs() > 100.0
        || !input.center_threshold.is_finite()
        || input.center_threshold <= 0.0
        || input.center_threshold > 100.0
        || input
            .range
            .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 1e5)
        || input
            .zero_range
            .is_some_and(|z| !z.is_finite() || z <= 0.0 || z > 1e5)
    {
        return Err("Niepoprawny dystans, próg celowania w środek lub nastawy przyrządów".into());
    }
    let mut solver = flat(profile, &input.unit, &input.options)?;
    let zero = match input.zero_range {
        Some(range) => solver
            .required(range, sight)?
            .ok_or("Dystans zerowania poza zasięgiem modelu")?,
        None => 0.0,
    };
    // Point-blank range: first sample (5-unit grid) where |drop| exceeds the
    // threshold, refined by bisection with exact RK4 events.
    let grid: Vec<f64> = (1..=1000).map(|i| 5.0 * f64::from(i)).collect();
    let heights = solver.sweep(zero, &grid)?;
    let exceeded = grid
        .iter()
        .zip(&heights)
        .position(|(_, h)| h.is_none_or(|h| (sight - h).abs() > input.center_threshold));
    let point_blank_range = match exceeded {
        None => Some(grid[grid.len() - 1]),
        Some(0) => None,
        Some(index) => {
            let (mut inside, mut outside) = (grid[index - 1], grid[index]);
            for _ in 0..40 {
                let middle = 0.5 * (inside + outside);
                match drop_at(&mut solver, zero, middle, sight)? {
                    Some(d) if d.abs() <= input.center_threshold => inside = middle,
                    _ => outside = middle,
                }
            }
            Some(inside)
        }
    };
    let mut result = QuickAim {
        range: input.range,
        point_blank_range,
        center_aim: input.range.is_none() && point_blank_range.is_some(),
        reachable: true,
        drop_below_line_of_sight: None,
        hold_over_mrad: None,
        elevation_mrad: None,
        time_of_flight: None,
        reticle: None,
        model: model_name(profile),
        status: status(profile),
        warnings: common_warnings(profile),
    };
    let Some(range) = input.range else {
        return Ok(result);
    };
    let path = solver.shot(zero, range)?;
    let required = solver.required(range, sight)?;
    result.reachable = required.is_some();
    result.time_of_flight = path.map(|(t, _)| t);
    result.drop_below_line_of_sight = path.map(|(_, s)| sight - s.position[2]);
    result.elevation_mrad = required.map(|r| r * 1000.0);
    result.hold_over_mrad = required.map(|r| (r - zero) * 1000.0);
    result.center_aim = result
        .drop_below_line_of_sight
        .is_some_and(|d| d.abs() <= input.center_threshold);
    if let (Some(reticle), true) = (&profile.reticle, result.reachable) {
        let mut best: Option<ReticleHold> = None;
        for mark in &reticle.marks {
            let Some(elevation) = solver.required(mark.range, sight)? else {
                continue;
            };
            let Some(residual) = drop_at(&mut solver, elevation, range, sight)? else {
                continue;
            };
            let residual_mrad = required.map_or(0.0, |r| (r - elevation) * 1000.0);
            if best
                .as_ref()
                .is_none_or(|b| residual.abs() < b.residual_drop.abs())
            {
                best = Some(ReticleHold {
                    mark: mark.mark.clone(),
                    mark_range: mark.range,
                    residual_drop: residual,
                    residual_mrad,
                    status: reticle.status.clone(),
                });
            }
        }
        if best.is_some() {
            result.warnings.push(format!(
                "Mapa kresek celownika ({}): {}; kreska oznacza dystans, dla którego model kompensuje opad",
                reticle.status, reticle.source
            ));
        }
        result.reticle = best;
    }
    Ok(result)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlShot {
    pub range: f64,
    /// Bore elevation actually used (as shown by the application), mrad.
    pub elevation_mrad: f64,
    /// Where the round passed the target: positive = above the aim point.
    pub miss_vertical: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlShotInput {
    pub unit: String,
    #[serde(default)]
    pub sight_height: Option<f64>,
    pub shots: Vec<ControlShot>,
    /// Largest accepted residual, profile unit (player's own tolerance).
    pub max_residual: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlShotFit {
    /// Vacuum speed that best explains the shots with the profile gravity.
    pub fitted_speed: Option<f64>,
    pub residuals: Vec<f64>,
    pub max_residual: Option<f64>,
    /// Model applicable (vacuum) and ≥ 2 shots within `max_residual`.
    pub fits: bool,
    pub model_applicable: bool,
    pub message: String,
    pub assumptions: Vec<String>,
}

/// Least-squares effective launch speed from control shots (vacuum, gravity
/// fixed to the profile value). With drag/rocket models the shots are only
/// logged: one vertical miss cannot identify several coefficients.
pub fn fit_control_shots(
    profile: &WeaponProfile,
    input: &ControlShotInput,
) -> Result<ControlShotFit, String> {
    Model::from_profile(profile, &input.unit)?;
    let sight = input.sight_height.unwrap_or(0.0);
    if input.shots.is_empty()
        || input.shots.len() > 100
        || !sight.is_finite()
        || !input.max_residual.is_finite()
        || input.max_residual <= 0.0
        || input.shots.iter().any(|s| {
            !s.range.is_finite()
                || s.range <= 0.0
                || s.range > 1e5
                || !s.elevation_mrad.is_finite()
                || s.elevation_mrad.abs() > 1500.0
                || !s.miss_vertical.is_finite()
                || s.miss_vertical.abs() > 1e4
        })
    {
        return Err(
            "Wymagane 1–100 strzałów z dystansem, użytą elewacją i odchyłką pionową".into(),
        );
    }
    let assumptions = vec![
        "Strzał kontrolny: cel na wysokości linii celowania; odchyłka mierzona pionowo przy celu"
            .to_string(),
        format!(
            "Grawitacja profilu {} przyjęta bez zmian; dopasowywana jest tylko efektywna prędkość",
            profile.gravity.unwrap_or(f64::NAN)
        ),
    ];
    if profile.dynamics_model() != DynamicsModel::Vacuum {
        return Ok(ControlShotFit {
            fitted_speed: None,
            residuals: Vec::new(),
            max_residual: None,
            fits: false,
            model_applicable: false,
            message: "Profil ma model oporu/napędu — strzały zapisano tylko w logu; jedna odchyłka nie wyznacza wielu współczynników".into(),
            assumptions,
        });
    }
    let g = profile.gravity.ok_or("Brak grawitacji")?;
    // height = r tanθ − b/v², b = g r² / (2 cos²θ): linear in u = 1/v².
    let terms: Vec<(f64, f64, f64)> = input
        .shots
        .iter()
        .map(|s| {
            let theta = s.elevation_mrad / 1000.0;
            let a = s.range * theta.tan();
            let b = g * s.range * s.range / (2.0 * theta.cos().powi(2));
            (a, b, sight + s.miss_vertical)
        })
        .collect();
    let numerator: f64 = terms.iter().map(|(a, b, h)| b * (a - h)).sum();
    let denominator: f64 = terms.iter().map(|(_, b, _)| b * b).sum();
    let u = numerator / denominator;
    if !u.is_finite() || u <= 0.0 {
        return Ok(ControlShotFit {
            fitted_speed: None,
            residuals: Vec::new(),
            max_residual: None,
            fits: false,
            model_applicable: true,
            message: "Odchyłki nie pasują do modelu bez oporu (wymagałyby ujemnego opadu); zapisano tylko w logu".into(),
            assumptions,
        });
    }
    let residuals: Vec<f64> = terms.iter().map(|(a, b, h)| a - b * u - h).collect();
    let max_residual = residuals.iter().map(|r| r.abs()).fold(0.0, f64::max);
    let fits = input.shots.len() >= 2 && max_residual <= input.max_residual;
    Ok(ControlShotFit {
        fitted_speed: Some(1.0 / u.sqrt()),
        residuals,
        max_residual: Some(max_residual),
        fits,
        model_applicable: true,
        message: if fits {
            "Strzały zgodne z modelem bez oporu w podanej tolerancji; można zastosować lokalną kalibrację beta".into()
        } else if input.shots.len() < 2 {
            "Jeden strzał wyznacza prędkość bez kontroli — dodaj strzał na innym dystansie".into()
        } else {
            "Reszty przekraczają tolerancję — model bez oporu nie opisuje strzałów; zapisano tylko w logu".into()
        },
        assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_profiles::{Category, ProjectileDynamics};

    fn direct(speed: f64, gravity: f64) -> WeaponProfile {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "id": "direct-test", "name": "Test / syntetyczny",
            "category": "direct_fire", "gameVersion": "syntetyczna", "status": "experimental",
            "source": "Test syntetyczny; nie MTC", "calibrated": false, "speed": speed,
            "gravity": gravity, "minElevationDeg": -89, "maxElevationDeg": 89,
            "dispersionSigma": null, "salvoSize": 1, "notes": "", "unit": "game_meters",
            "sourceAcknowledged": true,
            "observedSpecs": { "screen": "manual", "observedAt": "2026-10-16",
                "muzzleVelocity": speed, "velocityUnitAssumption": "test",
                "gravitySource": "test" }
        }))
        .unwrap()
    }
    fn card(p: &WeaponProfile, ranges: &[f64], sight: Option<f64>, zero: Option<f64>) -> RangeCard {
        range_card(
            p,
            &RangeCardInput {
                unit: "game_meters".into(),
                ranges: ranges.to_vec(),
                sight_height: sight,
                zero_range: zero,
                options: DirectFireOptions::default(),
            },
        )
        .unwrap()
    }
    fn near(a: f64, b: f64, tolerance: f64) {
        assert!((a - b).abs() <= tolerance, "{a} != {b} ± {tolerance}");
    }

    #[test]
    fn vacuum_range_card_matches_closed_form_drop_time_and_elevation() {
        let (v, g) = (300.0, 17.64);
        let p = direct(v, g);
        let result = card(&p, &[50.0, 200.0, 400.0, 800.0], None, None);
        assert_eq!(result.model, "vacuum");
        assert_eq!(result.status, "experimental");
        assert!(result.warnings.iter().any(|w| w.contains("bez oporu")));
        for row in &result.rows {
            let t = row.range / v;
            near(row.time_of_flight.unwrap(), t, 1e-9);
            near(row.drop_below_line_of_sight.unwrap(), 0.5 * g * t * t, 1e-7);
            let exact = 0.5 * (g * row.range / (v * v)).asin();
            near(row.elevation_mrad.unwrap(), exact * 1000.0, 1e-6);
            near(
                row.hold_over_mrad.unwrap(),
                row.elevation_mrad.unwrap(),
                1e-12,
            );
        }
    }

    #[test]
    fn zeroed_sight_crosses_line_of_sight_at_zero_range_and_holds_over_beyond() {
        let p = direct(250.0, 17.64);
        let result = card(&p, &[50.0, 300.0, 600.0], Some(0.1), Some(300.0));
        let at = |range: f64| result.rows.iter().find(|r| r.range == range).unwrap();
        near(at(300.0).drop_below_line_of_sight.unwrap(), 0.0, 1e-6);
        near(at(300.0).hold_over_mrad.unwrap(), 0.0, 1e-6);
        assert!(
            at(50.0).drop_below_line_of_sight.unwrap() < 0.0,
            "rises above LOS before zero"
        );
        assert!(at(600.0).drop_below_line_of_sight.unwrap() > 0.0);
        assert!(at(600.0).hold_over_mrad.unwrap() > 0.0);
        assert!(at(50.0).hold_over_mrad.unwrap() < 0.0);
        // Small-angle check: hold ≈ drop / range (within 1%).
        let row = at(600.0);
        let approximate = 1000.0 * row.drop_below_line_of_sight.unwrap() / 600.0;
        near(row.hold_over_mrad.unwrap(), approximate, 0.01 * approximate);
    }

    #[test]
    fn declared_drag_increases_drop_and_flight_time_versus_vacuum() {
        let vacuum = direct(250.0, 17.64);
        let mut drag = vacuum.clone();
        drag.calibrated = true;
        drag.status = "beta".into();
        drag.dynamics = Some(ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: "game_meters".into(),
            linear_drag: Some(0.0),
            quadratic_drag: Some(0.0005),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test; nie MTC".into(),
        });
        let a = card(&vacuum, &[400.0], None, None);
        let b = card(&drag, &[400.0], None, None);
        assert_eq!(b.model, "drag");
        assert_eq!(b.status, "beta");
        assert!(!b.warnings.iter().any(|w| w.contains("bez oporu")));
        assert!(b.rows[0].time_of_flight.unwrap() > a.rows[0].time_of_flight.unwrap() + 0.05);
        assert!(
            b.rows[0].drop_below_line_of_sight.unwrap()
                > a.rows[0].drop_below_line_of_sight.unwrap()
        );
        assert!(b.rows[0].elevation_mrad.unwrap() > a.rows[0].elevation_mrad.unwrap());
    }

    #[test]
    fn unreachable_rows_are_marked_instead_of_extrapolated() {
        let p = direct(60.0, 17.64);
        let result = card(&p, &[100.0, 5000.0], None, None);
        assert!(result.rows[0].reachable);
        assert!(!result.rows[1].reachable);
        assert!(result.rows[1].elevation_mrad.is_none());
        assert!(result.rows[1].note.is_some());
    }

    #[test]
    fn rejects_unacknowledged_guided_bad_units_and_invalid_ranges() {
        let mut p = direct(300.0, 17.64);
        p.source_acknowledged = false;
        let input = RangeCardInput {
            unit: "game_meters".into(),
            ranges: vec![100.0],
            sight_height: None,
            zero_range: None,
            options: DirectFireOptions::default(),
        };
        assert!(range_card(&p, &input).is_err());
        let p = direct(300.0, 17.64);
        let mut bad = input.clone();
        bad.unit = "game_units".into();
        assert!(range_card(&p, &bad).is_err());
        for ranges in [vec![], vec![-1.0], vec![f64::NAN], vec![1.0; 65]] {
            let mut bad = input.clone();
            bad.ranges = ranges;
            assert!(range_card(&p, &bad).is_err());
        }
        let mut guided = p.clone();
        guided.category = Category::GuidedMissile;
        assert!(range_card(&guided, &input).is_err());
    }

    #[test]
    fn crossing_target_lead_is_speed_times_time_and_course_signs_are_explicit() {
        let right = lead(&LeadInput {
            range: 200.0,
            target_speed: 10.0,
            course_relative_deg: 90.0,
            time_of_flight: 1.5,
        })
        .unwrap();
        near(right.lateral_offset, 15.0, 1e-12);
        near(right.radial_offset, 0.0, 1e-12);
        near(right.lead_mrad, (15.0_f64 / 200.0).atan() * 1000.0, 1e-9);
        let left = lead(&LeadInput {
            range: 200.0,
            target_speed: 10.0,
            course_relative_deg: 270.0,
            time_of_flight: 1.5,
        })
        .unwrap();
        assert!(left.lead_mrad < 0.0);
        let away = lead(&LeadInput {
            range: 200.0,
            target_speed: 10.0,
            course_relative_deg: 0.0,
            time_of_flight: 1.5,
        })
        .unwrap();
        near(away.lead_mrad, 0.0, 1e-12);
        near(away.future_range, 215.0, 1e-12);
        assert!(lead(&LeadInput {
            range: 200.0,
            target_speed: -1.0,
            course_relative_deg: 0.0,
            time_of_flight: 1.0
        })
        .is_err());
        assert!(lead(&LeadInput {
            range: 200.0,
            target_speed: 1.0,
            course_relative_deg: 0.0,
            time_of_flight: 0.0
        })
        .is_err());
    }

    #[test]
    fn iterated_lead_uses_flight_time_at_predicted_range() {
        let p = direct(200.0, 17.64);
        let input = |course: f64| LeadProfileInput {
            range: 300.0,
            target_speed: 15.0,
            course_relative_deg: course,
            unit: "game_meters".into(),
            sight_height: None,
            options: DirectFireOptions::default(),
        };
        let away = lead_iterated(&p, &input(0.0)).unwrap();
        let towards = lead_iterated(&p, &input(180.0)).unwrap();
        assert!(away.converged && towards.converged);
        assert!(away.time_of_flight > towards.time_of_flight);
        // Fixed point: flight time equals flat-fire time to the future range.
        let check = card(&p, &[away.future_range], None, None);
        let vacuum_time = {
            let theta = check.rows[0].elevation_mrad.unwrap() / 1000.0;
            away.future_range / (200.0 * theta.cos())
        };
        near(away.time_of_flight, vacuum_time, 1e-6);
        let crossing = lead_iterated(&p, &input(90.0)).unwrap();
        assert!(crossing.lead_mrad > 0.0);
        assert!(crossing.elevation_mrad.unwrap() > 0.0);
    }

    fn ub32() -> WeaponProfile {
        let mut p = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.id == "community-ub32-1")
            .unwrap();
        p.source_acknowledged = true;
        p
    }

    #[test]
    fn s5k1_level_and_dive_impacts_match_vacuum_with_inherited_airspeed() {
        let p = ub32();
        assert_eq!(p.category, Category::HelicopterRocket);
        let (v, g, h, airspeed) = (650.0, 17.64, 300.0, 50.0);
        for dive in [0.0_f64, 10.0, 30.0] {
            let result = air_to_ground_impact(
                &p,
                &AirToGroundInput {
                    altitude_above_target: h,
                    airspeed,
                    dive_angle_deg: dive,
                    unit: "game_meters".into(),
                    options: DirectFireOptions::default(),
                },
            )
            .unwrap();
            let u = v + airspeed;
            let (c, s) = (dive.to_radians().cos(), dive.to_radians().sin());
            // h = u s t + g t²/2
            let t = (-u * s + ((u * s).powi(2) + 2.0 * g * h).sqrt()) / g;
            near(result.time_of_flight, t, 1e-7);
            near(result.horizontal_distance, u * c * t, 1e-5);
            assert!(result.depression_below_boresight_mrad > 0.0);
            if dive > 0.0 {
                assert!(
                    result.aim_offset_along_ground.unwrap() < 0.0,
                    "rockets fall short of boresight"
                );
            } else {
                assert!(result.boresight_ground_distance.is_none());
            }
            assert!(result
                .assumptions
                .iter()
                .any(|a| a.contains("dziedziczenie")));
            assert_eq!(result.status, "experimental");
        }
    }

    #[test]
    fn inverse_dive_angle_reproduces_requested_distance() {
        let p = ub32();
        let input = AirToGroundSolveInput {
            altitude_above_target: 250.0,
            airspeed: 40.0,
            horizontal_distance: 900.0,
            unit: "game_meters".into(),
            options: DirectFireOptions::default(),
        };
        let solved = air_to_ground_solve(&p, &input).unwrap();
        near(solved.horizontal_distance, 900.0, 1e-3);
        let forward = air_to_ground_impact(
            &p,
            &AirToGroundInput {
                altitude_above_target: 250.0,
                airspeed: 40.0,
                dive_angle_deg: solved.dive_angle_deg,
                unit: "game_meters".into(),
                options: DirectFireOptions::default(),
            },
        )
        .unwrap();
        near(forward.horizontal_distance, 900.0, 1e-3);
        let mut too_close = input.clone();
        too_close.horizontal_distance = 1.0;
        assert!(air_to_ground_solve(&p, &too_close).is_err());
        let mut unacknowledged = ub32();
        unacknowledged.source_acknowledged = false;
        assert!(air_to_ground_solve(&unacknowledged, &input).is_err());
        let mut bad = input;
        bad.altitude_above_target = 0.0;
        assert!(air_to_ground_solve(&p, &bad).is_err());
    }

    fn quick(p: &WeaponProfile, range: Option<f64>, zero: Option<f64>, threshold: f64) -> QuickAim {
        quick_aim(
            p,
            &QuickAimInput {
                unit: "game_meters".into(),
                range,
                sight_height: Some(0.1),
                zero_range: zero,
                center_threshold: threshold,
                options: DirectFireOptions::default(),
            },
        )
        .unwrap()
    }

    #[test]
    fn point_blank_range_comes_from_trajectory_and_threshold_not_a_fixed_distance() {
        let fast = direct(1500.0, 17.64);
        let slow = direct(300.0, 17.64);
        let close_fast = quick(&fast, None, Some(500.0), 1.0);
        let close_slow = quick(&slow, None, Some(200.0), 1.0);
        assert!(close_fast.center_aim && close_slow.center_aim);
        let pbr_fast = close_fast.point_blank_range.unwrap();
        let pbr_slow = close_slow.point_blank_range.unwrap();
        assert!(pbr_fast > pbr_slow + 200.0, "{pbr_fast} vs {pbr_slow}");
        // At the boundary the drop equals the threshold; inside it is smaller.
        let edge = quick(&fast, Some(pbr_fast), Some(500.0), 1.0);
        near(edge.drop_below_line_of_sight.unwrap().abs(), 1.0, 1e-3);
        assert!(quick(&fast, Some(pbr_fast * 0.9), Some(500.0), 1.0).center_aim);
        let far = quick(&fast, Some(pbr_fast + 300.0), Some(500.0), 1.0);
        assert!(!far.center_aim);
        assert!(far.hold_over_mrad.unwrap() > 0.0);
        assert!(far.drop_below_line_of_sight.unwrap() > 1.0);
        // A looser threshold gives a longer point-blank range.
        assert!(
            quick(&fast, None, Some(500.0), 2.0)
                .point_blank_range
                .unwrap()
                > pbr_fast
        );
    }

    #[test]
    fn reticle_map_picks_nearest_mark_and_reports_remaining_correction() {
        let mut p = direct(120.0, 17.64);
        p.reticle = Some(mtc_profiles::ReticleMap {
            marks: [("2", 200.0), ("3", 300.0), ("4", 400.0), ("5", 500.0)]
                .iter()
                .map(|(mark, range)| mtc_profiles::ReticleMark {
                    mark: (*mark).into(),
                    range: *range,
                })
                .collect(),
            status: "unverified".into(),
            source: "Założenie gracza".into(),
        });
        let at_mark = quick(&p, Some(300.0), None, 0.5);
        let hold = at_mark.reticle.unwrap();
        assert_eq!(hold.mark, "3");
        near(hold.residual_drop, 0.0, 1e-6);
        assert_eq!(hold.status, "unverified");
        let between = quick(&p, Some(330.0), None, 0.5).reticle.unwrap();
        assert_eq!(between.mark, "3");
        assert!(
            between.residual_drop > 0.0 && between.residual_mrad > 0.0,
            "beyond mark: aim higher"
        );
        assert!(quick(&direct(120.0, 17.64), Some(330.0), None, 0.5)
            .reticle
            .is_none());
    }

    #[test]
    fn control_shots_recover_vacuum_speed_and_drag_profiles_are_log_only() {
        let truth = direct(260.0, 17.64);
        let assumed = direct(240.0, 17.64);
        // Shots fired with the elevation the assumed profile recommended.
        let shots: Vec<ControlShot> = [150.0, 300.0, 450.0]
            .iter()
            .map(|&range| {
                let e = card(&assumed, &[range], Some(0.1), None).rows[0]
                    .elevation_mrad
                    .unwrap();
                let actual = card(&truth, &[range], Some(0.1), None);
                // Height of the true trajectory at that elevation, relative to LOS.
                let theta = e / 1000.0;
                let height = range * theta.tan()
                    - 17.64 * range * range / (2.0 * 260.0_f64.powi(2) * theta.cos().powi(2));
                assert!(actual.rows[0].reachable);
                ControlShot {
                    range,
                    elevation_mrad: e,
                    miss_vertical: height - 0.1,
                }
            })
            .collect();
        assert!(
            shots.iter().all(|s| s.miss_vertical > 0.0),
            "faster round lands high"
        );
        let input = ControlShotInput {
            unit: "game_meters".into(),
            sight_height: Some(0.1),
            shots,
            max_residual: 0.05,
        };
        let fit = fit_control_shots(&assumed, &input).unwrap();
        assert!(fit.fits && fit.model_applicable);
        near(fit.fitted_speed.unwrap(), 260.0, 1e-6);
        let single = ControlShotInput {
            shots: input.shots[..1].to_vec(),
            ..input.clone()
        };
        assert!(!fit_control_shots(&assumed, &single).unwrap().fits);
        let mut drag = assumed.clone();
        drag.calibrated = true;
        drag.status = "beta".into();
        drag.dynamics = Some(ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: "game_meters".into(),
            linear_drag: Some(0.01),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test".into(),
        });
        let logged = fit_control_shots(&drag, &input).unwrap();
        assert!(!logged.model_applicable && logged.fitted_speed.is_none());
        let mut nonsense = input;
        nonsense.shots[0].miss_vertical = 1e3;
        nonsense.shots[1].miss_vertical = 1e3;
        nonsense.shots[2].miss_vertical = 1e3;
        assert!(!fit_control_shots(&assumed, &nonsense).unwrap().fits);
    }
}
