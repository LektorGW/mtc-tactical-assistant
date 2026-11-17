use mtc_profiles::WeaponProfile;
use serde::Serialize;
mod dynamics;
pub use dynamics::{
    simulate, solve_dynamic, DynamicArc, DynamicSolution, DynamicSolveInput, DynamicSolveOptions,
    SimulationInput, SimulationOptions, TrajectoryPoint, TrajectorySimulation,
};
mod calibration;
pub use calibration::{
    fit_vacuum, CalibrationFit, CalibrationOptions, CalibrationResidual, CalibrationSample,
};
mod intercept3d;
pub use intercept3d::{solve_intercept_3d, InterceptArc3d, InterceptInput3d, InterceptSolution3d};
mod dynamic_intercept;
pub use dynamic_intercept::{
    solve_dynamic_intercept, DynamicInterceptArc, DynamicInterceptInput, DynamicInterceptOptions,
    DynamicInterceptSolution, InterceptBranchReport,
};
mod direct_fire;
pub use direct_fire::{
    air_to_ground_impact, air_to_ground_solve, fit_control_shots, lead, lead_iterated, quick_aim,
    range_card, AirToGroundImpact, AirToGroundInput, AirToGroundSolveInput, ControlShot,
    ControlShotFit, ControlShotInput, DirectFireOptions, LeadEstimate, LeadInput, LeadProfileInput,
    QuickAim, QuickAimInput, RangeCard, RangeCardInput, RangeCardRow, ReticleHold,
};
mod salvo;
pub use salvo::{
    salvo_pattern, DispersionEllipse, SalvoCoverage, SalvoInput, SalvoOptions, SalvoPattern,
    SalvoTiming,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcSolution {
    pub elevation_deg: f64,
    pub flight_time: f64,
    pub high_arc: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Solution {
    pub arcs: Vec<ArcSolution>,
    pub dispersion_radius_95: Option<f64>,
    pub assumptions: Vec<String>,
}

/// Constant speed at launch, constant game gravity, zero drag. Never real-world weapon data.
pub fn solve(
    profile: &WeaponProfile,
    distance: f64,
    height_delta: f64,
) -> Result<Solution, String> {
    profile.can_calculate()?;
    profile.require_vacuum()?;
    if !distance.is_finite() || distance <= 0.0 || !height_delta.is_finite() {
        return Err("Niepoprawny dystans lub różnica wysokości".into());
    }
    let v = profile.speed.ok_or("Brak prędkości pocisku")?;
    let g = profile.gravity.ok_or("Brak grawitacji gry")?;
    let v2 = v * v;
    let disc = v2 * v2 - g * (g * distance * distance + 2.0 * height_delta * v2);
    if !disc.is_finite() || disc < 0.0 {
        return Err("Cel poza zasięgiem wybranego modelu".into());
    }
    let mut arcs = Vec::new();
    for high in [false, true] {
        let theta = ((v2 + if high { disc.sqrt() } else { -disc.sqrt() }) / (g * distance)).atan();
        let angle = theta.to_degrees();
        let time = distance / (v * theta.cos());
        if angle >= profile.min_elevation_deg
            && angle <= profile.max_elevation_deg
            && time.is_finite()
        {
            if high
                && arcs
                    .first()
                    .is_some_and(|a: &ArcSolution| (a.elevation_deg - angle).abs() < 1e-8)
            {
                continue;
            }
            arcs.push(ArcSolution {
                elevation_deg: angle,
                flight_time: time,
                high_arc: high,
            });
        }
    }
    if arcs.is_empty() {
        return Err("Brak trajektorii w limitach elewacji profilu".into());
    }
    let mut assumptions = vec![
        "Model bez oporu powietrza i bez przyspieszania rakiet".into(),
        "Stała grawitacja gry; zgodność parametrów wymaga testów w MTC".into(),
        format!(
            "Jednostka profilu: {}; dystans i wysokość muszą być zgodne",
            profile.unit
        ),
    ];
    if !profile.calibrated {
        assumptions.push("Eksperymentalne dane społeczności zaakceptowane dla tego pocisku; brak lokalnej kalibracji".into());
    }
    if let Some(source) = &profile.source_metadata {
        assumptions.push(format!(
            "Źródło: {}@{}; experience {}; pobrano {}",
            source.repository, source.commit, source.experience_id, source.fetched_at
        ));
    }
    Ok(Solution {
        arcs,
        dispersion_radius_95: profile
            .dispersion_sigma
            .map(|s| s * (-2.0 * 0.05_f64.ln()).sqrt()),
        assumptions,
    })
}

/// Interception under straight-line constant-speed motion. Used by tanks/AA as an explicit approximation.
pub fn intercept(
    relative: [f64; 2],
    velocity: [f64; 2],
    projectile_speed: f64,
) -> Result<(f64, [f64; 2]), String> {
    if relative
        .iter()
        .chain(velocity.iter())
        .any(|v| !v.is_finite())
        || !projectile_speed.is_finite()
        || projectile_speed <= 0.0
    {
        return Err("Brak poprawnej obserwowanej prędkości celu lub pocisku".into());
    }
    let a = velocity[0].powi(2) + velocity[1].powi(2) - projectile_speed.powi(2);
    let b = 2.0 * (relative[0] * velocity[0] + relative[1] * velocity[1]);
    let c = relative[0].powi(2) + relative[1].powi(2);
    if c <= 1e-12 {
        return Err("Cel pokrywa się z pozycją strzelca".into());
    }
    let mut candidates = Vec::new();
    if a.abs() < 1e-9 {
        if b.abs() > 1e-9 {
            candidates.push(-c / b);
        }
    } else {
        let d = b * b - 4.0 * a * c;
        if d >= 0.0 {
            candidates.extend([(-b - d.sqrt()) / (2.0 * a), (-b + d.sqrt()) / (2.0 * a)]);
        }
    }
    let t = candidates
        .into_iter()
        .filter(|t| *t > 0.0 && t.is_finite())
        .min_by(f64::total_cmp)
        .ok_or("Brak rozwiązania wyprzedzenia")?;
    Ok((t, [velocity[0] * t, velocity[1] * t]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_profiles::Category;
    fn calibrated() -> WeaponProfile {
        let mut p = mtc_profiles::built_in().remove(0);
        p.speed = Some(100.0);
        p.gravity = Some(10.0);
        p.calibrated = true;
        p.min_elevation_deg = -10.0;
        p.max_elevation_deg = 89.0;
        p
    }
    #[test]
    fn refuses_unverified_weapon_data() {
        assert!(solve(&mtc_profiles::built_in()[0], 500.0, 0.0).is_err());
    }
    #[test]
    fn solves_both_arcs_and_reconstructs_target() {
        let p = calibrated();
        let s = solve(&p, 500.0, 20.0).unwrap();
        assert_eq!(s.arcs.len(), 2);
        for arc in s.arcs {
            let a = arc.elevation_deg.to_radians();
            let t = arc.flight_time;
            assert!((100.0 * a.cos() * t - 500.0).abs() < 1e-6);
            assert!((100.0 * a.sin() * t - 5.0 * t * t - 20.0).abs() < 1e-6);
        }
    }
    #[test]
    fn rejects_out_of_range_and_invalid_distance() {
        assert!(solve(&calibrated(), 1001.0, 0.0).is_err());
        assert!(solve(&calibrated(), f64::NAN, 0.0).is_err());
    }
    #[test]
    fn deduplicates_maximum_range_arc() {
        assert_eq!(solve(&calibrated(), 1000.0, 0.0).unwrap().arcs.len(), 1);
    }
    #[test]
    fn honors_elevation_limits() {
        let mut p = calibrated();
        p.max_elevation_deg = 20.0;
        assert_eq!(solve(&p, 500.0, 0.0).unwrap().arcs.len(), 1);
    }
    #[test]
    fn computes_intercept_and_rejects_escaping_target() {
        let (t, lead) = intercept([100.0, 0.0], [0.0, 10.0], 100.0).unwrap();
        assert!((t - 1.0050378).abs() < 1e-6);
        assert!((lead[1] - 10.050378).abs() < 1e-5);
        assert!(intercept([100.0, 0.0], [200.0, 0.0], 100.0).is_err());
    }
    #[test]
    fn community_bm21_matches_source_vacuum_model_only_after_acknowledgement() {
        let mut p = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.id == "community-bm_21_grad-1")
            .unwrap();
        assert_eq!(p.speed, Some(180.0));
        assert_eq!(p.gravity, Some(17.64));
        assert!(solve(&p, 500.0, 0.0).is_err());
        p.source_acknowledged = true;
        let result = solve(&p, 500.0, 0.0).unwrap();
        assert_eq!(result.arcs.len(), 2);
        for arc in result.arcs {
            let angle = arc.elevation_deg.to_radians();
            assert!((180.0 * angle.cos() * arc.flight_time - 500.0).abs() < 1e-7);
            assert!(
                (180.0 * angle.sin() * arc.flight_time - 8.82 * arc.flight_time.powi(2)).abs()
                    < 1e-7
            );
        }
        assert!(result
            .assumptions
            .iter()
            .any(|a| a.contains("brak lokalnej kalibracji")));
        assert!(!p.calibrated);
    }
    /// Transcription of grand-hawk `utils/math.ts` at the pinned commit:
    /// `G = 9.8 * 1.8`, `calculateLow/HighElevation`, `calculateTimeOfFlight`.
    mod source {
        pub const G: f64 = 9.8 * 1.8;
        pub fn elevation(d: f64, v: f64, h: f64, high: bool) -> f64 {
            let root = (v.powi(4) - G * (G * d.powi(2) + 2.0 * h * v.powi(2))).sqrt();
            let sign = if high { 1.0 } else { -1.0 };
            ((v.powi(2) + sign * root) / (G * d)).atan() * (180.0 / std::f64::consts::PI)
        }
        pub fn time_of_flight(e: f64, v: f64, d: f64) -> f64 {
            d / (v * (e * std::f64::consts::PI / 180.0).cos())
        }
    }
    #[test]
    fn transcribed_formulas_still_match_the_pinned_source_file() {
        let math = include_str!("../../../data/weapons/community/source/math.ts");
        for fragment in [
            "const G = 9.8 * 1.8;",
            "return s / (5 / 1.8);",
            "Math.sqrt(v ** 4 - G * (G * d ** 2 + 2 * h * v ** 2))",
            "Math.atan((v ** 2 - squareRoot) / (G * d))",
            "Math.atan((v ** 2 + squareRoot) / (G * d))",
            "return d / (v * Math.cos(radians));",
        ] {
            assert!(math.contains(fragment), "math.ts changed: {fragment}");
        }
    }
    #[test]
    fn agrees_with_grand_hawk_calculator_for_catalog_profiles() {
        let ids = [
            "community-bm_21_grad-1",
            "community-m270_mlrs-5",
            "community-d30-4",
            "community-mortar-2",
            "community-vasilek-2",
            "community-pounder-1",
            "community-m109-1",
        ];
        let profiles = mtc_profiles::community_profiles();
        let mut compared = 0;
        let mut worst_elevation: f64 = 0.0;
        let mut worst_time: f64 = 0.0;
        for id in ids {
            let mut p = profiles.iter().find(|p| p.id == id).unwrap().clone();
            assert_eq!(p.gravity.unwrap().to_bits(), source::G.to_bits(), "{id}");
            p.source_acknowledged = true;
            let v = p.speed.unwrap();
            for d in [150.0, 400.0, 900.0, 1500.0, 2500.0, 6000.0] {
                for h in [-40.0, 0.0, 35.0] {
                    let ours = solve(&p, d, h);
                    let low = source::elevation(d, v, h, false);
                    let high = source::elevation(d, v, h, true);
                    if low.is_nan() {
                        // Source shows "N/A"; our solver must refuse, not guess.
                        assert!(ours.is_err(), "{id} {d} {h}");
                        continue;
                    }
                    let ours = ours.unwrap();
                    for arc in &ours.arcs {
                        let expected = if arc.high_arc { high } else { low };
                        worst_elevation = worst_elevation.max((arc.elevation_deg - expected).abs());
                        // Horizontal distance in the source ToF formula; equal when h = 0.
                        let tof = source::time_of_flight(expected, v, d);
                        worst_time = worst_time.max((arc.flight_time - tof).abs() / tof);
                        compared += 1;
                    }
                }
            }
        }
        eprintln!("compared {compared} arcs; worst elevation {worst_elevation:e} deg; worst relative ToF {worst_time:e}");
        assert!(compared > 150, "{compared}");
        assert!(worst_elevation < 1e-9, "{worst_elevation}");
        assert!(worst_time < 1e-12, "{worst_time}");
    }
    #[test]
    fn source_ui_slant_range_time_of_flight_is_not_copied_for_height_differences() {
        // grand-hawk TimeOfFlight.tsx passes useDistanceWithHeight (3D slant
        // range) to calculateTimeOfFlight; the trajectory needs horizontal range.
        let mut p = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.id == "community-bm_21_grad-1")
            .unwrap();
        p.source_acknowledged = true;
        let (d, h, v, g) = (1500.0, 35.0, 180.0, source::G);
        let arc = solve(&p, d, h).unwrap().arcs.remove(0);
        let theta = arc.elevation_deg.to_radians();
        let height = |t: f64| v * theta.sin() * t - 0.5 * g * t * t;
        assert!((height(arc.flight_time) - h).abs() < 1e-8);
        let slant = source::time_of_flight(arc.elevation_deg, v, d.hypot(h));
        assert!((slant - arc.flight_time).abs() > 1e-3);
        assert!((height(slant) - h).abs() > 0.1);
    }
    #[test]
    fn community_guided_weapon_remains_disabled_even_when_source_is_acknowledged() {
        let mut p = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.category == Category::GuidedMissile)
            .unwrap();
        p.source_acknowledged = true;
        assert!(solve(&p, 500.0, 0.0).is_err());
    }
}
