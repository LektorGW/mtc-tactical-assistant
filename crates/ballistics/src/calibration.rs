use mtc_profiles::{Category, WeaponProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationSample {
    pub range: f64,
    pub elevation_deg: f64,
    pub flight_time: f64,
    pub height_delta: f64,
    pub unit: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationOptions {
    pub unit: String,
    pub max_residual_units: f64,
    pub max_relative_residual: f64,
    pub min_samples: usize,
    pub source: String,
    #[serde(default)]
    pub game_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationResidual {
    pub sample_index: usize,
    pub range_error: f64,
    pub height_error: f64,
    pub residual_units: f64,
    pub relative_residual: f64,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationFit {
    pub fitted_profile: WeaponProfile,
    pub rms_residual_units: f64,
    pub max_residual_units: f64,
    pub max_relative_residual: f64,
    /// Condition number of the column-normalized normal matrix; dimensionless.
    pub condition_number: f64,
    pub observed_range_min: f64,
    pub observed_range_max: f64,
    pub residuals: Vec<CalibrationResidual>,
    pub assumptions: Vec<String>,
}

/// Least squares for the explicitly chosen game vacuum model:
/// range = v*cos(e)*t; height = v*sin(e)*t - g*t²/2.
/// Both unknowns are fitted to actual observations supplied by the caller.
/// No drag/thrust/default projectile parameters or explosive data enter this fit.
pub fn fit_vacuum(
    profile: &WeaponProfile,
    samples: &[CalibrationSample],
    options: &CalibrationOptions,
) -> Result<CalibrationFit, String> {
    profile.validate()?;
    profile.require_vacuum()?;
    if profile.category == Category::GuidedMissile {
        return Err(
            "lack_model: nie można kalibrować nieznanej mechaniki naprowadzania modelem vacuum"
                .into(),
        );
    }
    if !["game_units", "game_meters"].contains(&options.unit.as_str())
        || options.min_samples < 3
        || options.min_samples > 1000
        || samples.len() < options.min_samples
        || samples.len() > 1000
        || options.source.trim().is_empty()
        || options.source.len() > 2048
        || options
            .game_version
            .as_ref()
            .is_some_and(|version| version.trim().is_empty())
        || !options.max_residual_units.is_finite()
        || options.max_residual_units <= 0.0
        || !options.max_relative_residual.is_finite()
        || options.max_relative_residual <= 0.0
        || options.max_relative_residual > 1.0
    {
        return Err("Wymagane ≥3 pomiary, jawne źródło, jednostka i dodatnie limity błędu".into());
    }
    let mut a = 0.0;
    let mut b = 0.0;
    let mut c = 0.0;
    let mut d = 0.0;
    let mut e = 0.0;
    for sample in samples {
        if !sample.range.is_finite()
            || sample.range <= 0.0
            || !sample.flight_time.is_finite()
            || sample.flight_time <= 0.0
            || !sample.height_delta.is_finite()
            || !sample.elevation_deg.is_finite()
            || sample.elevation_deg < profile.min_elevation_deg
            || sample.elevation_deg > profile.max_elevation_deg
            || sample.unit != options.unit
            || sample.source.trim().is_empty()
            || sample.source.len() > 1024
        {
            return Err(
                "Niepoprawny pomiar: wymagane range/kąt/czas/wysokość, wspólna jednostka i źródło"
                    .into(),
            );
        }
        let theta = sample.elevation_deg.to_radians();
        let horizontal = theta.cos() * sample.flight_time;
        let vertical = theta.sin() * sample.flight_time;
        let gravity = -0.5 * sample.flight_time.powi(2);
        a += horizontal * horizontal + vertical * vertical;
        b += vertical * gravity;
        c += gravity * gravity;
        d += horizontal * sample.range + vertical * sample.height_delta;
        e += gravity * sample.height_delta;
    }
    if ![a, b, c, d, e].iter().all(|value| value.is_finite()) || a <= 0.0 || c <= 0.0 {
        return Err("Pomiary prowadzą do przepełnienia lub nieokreślonego dopasowania".into());
    }
    let changes = samples.iter().skip(1).any(|sample| {
        (sample.flight_time - samples[0].flight_time).abs() > 1e-8 * samples[0].flight_time
            || (sample.elevation_deg - samples[0].elevation_deg).abs() > 1e-8
    });
    if !changes {
        return Err("Brak różnych warunków pomiaru: zmień czas lotu lub elewację".into());
    }
    // Scale the two columns to avoid unit-dependent conditioning and determinants.
    let norm_v = a.sqrt();
    let norm_g = c.sqrt();
    let rho = b / norm_v / norm_g;
    let determinant = 1.0 - rho * rho;
    let condition_number = (1.0 + rho.abs()) / (1.0 - rho.abs());
    if !condition_number.is_finite() || determinant <= 1e-8 || condition_number > 1e6 {
        return Err(
            "Nieidentyfikowalne parametry: pomiary prędkości i grawitacji są źle uwarunkowane"
                .into(),
        );
    }
    let dv = d / norm_v;
    let eg = e / norm_g;
    let speed = (dv - rho * eg) / determinant / norm_v;
    let gravity = (eg - rho * dv) / determinant / norm_g;
    if !speed.is_finite() || !gravity.is_finite() || speed <= 0.0 || gravity <= 0.0 {
        return Err("Pomiary nie pasują do dodatniej prędkości i grawitacji modelu vacuum".into());
    }
    let mut residuals = Vec::with_capacity(samples.len());
    let mut sum_squared = 0.0;
    let mut max_residual: f64 = 0.0;
    let mut max_relative: f64 = 0.0;
    for (sample_index, sample) in samples.iter().enumerate() {
        let theta = sample.elevation_deg.to_radians();
        let range_error = speed * theta.cos() * sample.flight_time - sample.range;
        let height_error = speed * theta.sin() * sample.flight_time
            - 0.5 * gravity * sample.flight_time.powi(2)
            - sample.height_delta;
        let residual_units = range_error.hypot(height_error);
        let relative_residual = residual_units / sample.range.hypot(sample.height_delta);
        sum_squared += residual_units.powi(2);
        max_residual = max_residual.max(residual_units);
        max_relative = max_relative.max(relative_residual);
        residuals.push(CalibrationResidual {
            sample_index,
            range_error,
            height_error,
            residual_units,
            relative_residual,
            source: sample.source.clone(),
        });
    }
    if !sum_squared.is_finite()
        || max_residual > options.max_residual_units
        || max_relative > options.max_relative_residual
    {
        return Err(format!("Odrzucono model vacuum: maks. reszta {max_residual:.6} {}, względna {max_relative:.6}; sprawdź jednostki, czas i mechanikę", options.unit));
    }
    let observed_range_min = samples
        .iter()
        .map(|sample| sample.range)
        .min_by(f64::total_cmp)
        .unwrap();
    let observed_range_max = samples
        .iter()
        .map(|sample| sample.range)
        .max_by(f64::total_cmp)
        .unwrap();
    let mut fitted_profile = profile.clone();
    fitted_profile.speed = Some(speed);
    fitted_profile.gravity = Some(gravity);
    fitted_profile.unit = options.unit.clone();
    fitted_profile.calibrated = true;
    fitted_profile.status = "beta".into();
    fitted_profile.source = format!(
        "Lokalna kalibracja vacuum: {}; {} pomiarów; szczegółowe źródła w raporcie dopasowania",
        options.source,
        samples.len()
    );
    if let Some(version) = &options.game_version {
        fitted_profile.game_version = version.clone();
    }
    fitted_profile.notes = format!("{} Dopasowanie lokalne v={speed:.6}, g={gravity:.6}; zakres pomiarów {observed_range_min:.6}–{observed_range_max:.6} {}; brak walidacji oporu/napędu lub innych zakresów.", fitted_profile.notes, options.unit);
    fitted_profile.validate()?;
    Ok(CalibrationFit {
        fitted_profile, rms_residual_units: (sum_squared / samples.len() as f64).sqrt(),
        max_residual_units: max_residual, max_relative_residual: max_relative, condition_number,
        observed_range_min, observed_range_max, residuals,
        assumptions: vec![
            "Dopasowano stałą prędkość początkową i grawitację gry bez oporu i napędu".into(),
            "Próbki muszą dotyczyć tego samego pocisku, wersji gry i układu jednostek".into(),
            "Status beta: dopasowanie nie dowodzi zgodności mechaniki poza zakresem pomiarów".into(),
            "Nie odrzucamy po cichu odstających próbek ani nie potwierdzamy automatycznie źródła społeczności".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn samples() -> Vec<CalibrationSample> {
        [(12.0_f64, 2.0_f64), (30.0, 4.0), (55.0, 7.0), (65.0, 9.0)]
            .iter()
            .map(|(angle, time)| CalibrationSample {
                range: 100.0 * angle.to_radians().cos() * time,
                elevation_deg: *angle,
                flight_time: *time,
                height_delta: 100.0 * angle.to_radians().sin() * time - 5.0 * time * time,
                unit: "game_units".into(),
                source: "Deterministyczny test syntetyczny, nie pomiar MTC".into(),
            })
            .collect()
    }
    fn options() -> CalibrationOptions {
        CalibrationOptions {
            unit: "game_units".into(),
            max_residual_units: 0.01,
            max_relative_residual: 0.0001,
            min_samples: 3,
            source: "Dane testowe".into(),
            game_version: Some("syntetyczna".into()),
        }
    }
    #[test]
    fn reconstructs_both_parameters_and_preserves_provenance_without_community_ack() {
        let mut p = mtc_profiles::community_profiles().remove(0);
        p.min_elevation_deg = 0.0;
        p.max_elevation_deg = 89.0;
        let fit = fit_vacuum(&p, &samples(), &options()).unwrap();
        assert!((fit.fitted_profile.speed.unwrap() - 100.0).abs() < 1e-8);
        assert!((fit.fitted_profile.gravity.unwrap() - 10.0).abs() < 1e-8);
        assert_eq!(fit.fitted_profile.status, "beta");
        assert_eq!(fit.fitted_profile.game_version, "syntetyczna");
        assert!(!fit.fitted_profile.source_acknowledged);
        assert!(fit.fitted_profile.source_metadata.is_some());
        assert!(fit.rms_residual_units < 1e-8);
    }
    #[test]
    fn rejects_wrong_units_outliers_missing_evidence_and_repeated_conditions() {
        let p = mtc_profiles::built_in().remove(1);
        let mut observations = samples();
        observations[1].unit = "game_meters".into();
        assert!(fit_vacuum(&p, &observations, &options()).is_err());
        observations = samples();
        observations[1].range += 25.0;
        assert!(fit_vacuum(&p, &observations, &options()).is_err());
        observations = samples();
        observations[1].source.clear();
        assert!(fit_vacuum(&p, &observations, &options()).is_err());
        assert!(fit_vacuum(&p, &vec![samples()[0].clone(); 4], &options()).is_err());
    }
    #[test]
    fn rejects_unobserved_guidance_nonfinite_and_insufficient_samples() {
        let p = mtc_profiles::built_in().remove(5);
        assert!(fit_vacuum(&p, &samples(), &options())
            .unwrap_err()
            .contains("lack_model"));
        let p = mtc_profiles::built_in().remove(1);
        let mut observations = samples();
        observations[0].flight_time = f64::NAN;
        assert!(fit_vacuum(&p, &observations, &options()).is_err());
        assert!(fit_vacuum(&p, &samples()[..2], &options()).is_err());
    }
    #[test]
    fn small_observation_noise_is_reported_and_negative_gravity_is_not_invented() {
        let p = mtc_profiles::built_in().remove(1);
        let mut noisy = samples();
        noisy[0].range += 0.03;
        noisy[2].height_delta -= 0.02;
        let mut limits = options();
        limits.max_residual_units = 0.1;
        limits.max_relative_residual = 0.01;
        let fit = fit_vacuum(&p, &noisy, &limits).unwrap();
        assert!(fit.rms_residual_units > 0.0);
        assert!((fit.fitted_profile.speed.unwrap() - 100.0).abs() < 0.01);
        assert!((fit.fitted_profile.gravity.unwrap() - 10.0).abs() < 0.01);
        for sample in &mut noisy {
            sample.height_delta =
                100.0 * sample.elevation_deg.to_radians().sin() * sample.flight_time
                    + 5.0 * sample.flight_time.powi(2);
        }
        assert!(fit_vacuum(&p, &noisy, &limits).is_err());
    }
    #[test]
    fn extreme_measurements_and_out_of_limit_angles_are_rejected() {
        let p = mtc_profiles::built_in().remove(1);
        let mut huge = samples();
        huge[0].flight_time = f64::MAX;
        assert!(fit_vacuum(&p, &huge, &options()).is_err());
        let mut invalid = samples();
        invalid[0].elevation_deg = 90.0;
        assert!(fit_vacuum(&p, &invalid, &options()).is_err());
    }
}
