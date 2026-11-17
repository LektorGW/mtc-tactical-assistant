//! Salvo forecast: mean point of impact and dispersion ellipses for N
//! projectiles, propagated from explicit launch dispersion through the same
//! RK4 model as the trajectory. Without explicit dispersion data there is no
//! forecast; no default spread is ever assumed.
use crate::dynamics::{self, norm, Budget, Model};
use mtc_profiles::WeaponProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SalvoOptions {
    pub step_seconds: f64,
    pub max_work_steps: u32,
    /// Deterministic quasi-random samples of one projectile's impact.
    pub coverage_samples: u32,
    /// Grid resolution across the area diameter for the coverage average.
    pub coverage_grid: u32,
}
impl Default for SalvoOptions {
    fn default() -> Self {
        Self {
            step_seconds: 0.01,
            max_work_steps: 2_000_000,
            coverage_samples: 2048,
            coverage_grid: 25,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SalvoInput {
    /// Static target: horizontal distance, height above muzzle, azimuth.
    pub distance: f64,
    pub height_delta: f64,
    pub azimuth_deg: f64,
    /// Aim of the solved arc (from `solve` or `solve_dynamic`).
    pub elevation_deg: f64,
    pub flight_time: f64,
    pub unit: String,
    /// Explicitly observed projectile count; defaults to `profile.salvoSize`.
    #[serde(default)]
    pub salvo_size: Option<u32>,
    /// Effect radius of one projectile from the user's observation, if any.
    #[serde(default)]
    pub effect_radius: Option<f64>,
    /// Radius of the area around the aim point whose coverage is estimated.
    #[serde(default)]
    pub area_radius: Option<f64>,
    #[serde(default)]
    pub options: SalvoOptions,
}

/// Ellipse in the target-centred east/north frame of the profile unit.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispersionEllipse {
    pub probability: f64,
    pub center_offset: [f64; 2],
    pub semi_major: f64,
    pub semi_minor: f64,
    /// Azimuth of the major axis, degrees clockwise from north, in [0, 180).
    pub major_axis_azimuth_deg: f64,
    /// 72 outline points, east/north offsets from the target.
    pub outline: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvoCoverage {
    pub effect_radius: f64,
    /// P(at least one of N impacts within effect radius of the aim point).
    pub probability_any_within_effect: f64,
    pub expected_projectiles_within_effect: f64,
    pub area_radius: Option<f64>,
    /// Expected fraction of the area within effect radius of ≥1 impact.
    pub expected_area_fraction: Option<f64>,
    pub samples: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvoTiming {
    pub ripple_interval: f64,
    pub first_impact: f64,
    pub last_impact: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalvoPattern {
    pub unit: String,
    /// `experimental` or `beta`; never `verified`.
    pub status: String,
    pub salvo_size: u32,
    /// `launch_dispersion` (profile.salvo) or `impact_sigma` (dispersionSigma).
    pub dispersion_source: String,
    /// East/north of the target from the muzzle.
    pub target_position: [f64; 2],
    /// Forecast mean point of impact relative to the target (second order).
    pub center_offset: [f64; 2],
    /// 2×2 covariance of one impact, east/north, unit².
    pub covariance: [[f64; 2]; 2],
    pub range_sigma: f64,
    pub cross_range_sigma: f64,
    /// 95% region of a single projectile.
    pub single_projectile: DispersionEllipse,
    /// Region containing all N impacts with probability 95%.
    pub salvo_envelope: DispersionEllipse,
    /// 95% region of the mean point of impact of N independent projectiles.
    pub mean_point_of_impact: DispersionEllipse,
    /// max |second-order term| / |first-order term|; >0.2 marks a poor ellipse.
    pub nonlinearity: f64,
    pub coverage: Option<SalvoCoverage>,
    pub timing: Option<SalvoTiming>,
    pub missing_components: Vec<String>,
    pub assumptions: Vec<String>,
}

fn chi2_two_dof_radius(probability: f64) -> f64 {
    (-2.0 * (1.0 - probability).ln()).sqrt()
}

struct Impact<'a> {
    model: Model,
    input: &'a SalvoInput,
    budget: Budget,
}

impl Impact<'_> {
    /// Impact east/north when the projectile crosses the target height nearest
    /// the nominal flight time, for launch-direction/speed perturbations.
    fn point(
        &mut self,
        elevation_offset: f64,
        cross_offset: f64,
        speed_offset: f64,
    ) -> Result<[f64; 2], String> {
        let theta = self.input.elevation_deg.to_radians();
        let phi = self.input.azimuth_deg.rem_euclid(360.0).to_radians();
        let nominal = dynamics::launch_direction(self.input.elevation_deg, self.input.azimuth_deg);
        let up_axis = [
            -theta.sin() * phi.sin(),
            -theta.sin() * phi.cos(),
            theta.cos(),
        ];
        let side_axis = [phi.cos(), -phi.sin(), 0.0];
        let raw: [f64; 3] = std::array::from_fn(|i| {
            nominal[i] + elevation_offset.tan() * up_axis[i] + cross_offset.tan() * side_axis[i]
        });
        let length = norm(raw);
        let launch = raw.map(|v| v / length);
        let speed = self.model.speed + speed_offset;
        if !speed.is_finite() || speed <= 0.0 {
            return Err(
                "Rozrzut prędkości prowadzi do niedodatniej prędkości; linearyzacja nieważna"
                    .into(),
            );
        }
        let horizon = (1.5 * self.input.flight_time + 2.0).min(600.0);
        let crossings = self.model.plane_crossings(
            launch,
            launch.map(|v| v * speed),
            self.input.height_delta,
            horizon,
            self.input.options.step_seconds,
            &mut self.budget,
        )?;
        crossings
            .iter()
            .min_by(|a, b| {
                (a.0 - self.input.flight_time)
                    .abs()
                    .total_cmp(&(b.0 - self.input.flight_time).abs())
            })
            .map(|(_, state)| [state.position[0], state.position[1]])
            .ok_or_else(|| "Rozrzut nieokreślony: tor nie przecina wysokości celu (np. przy maksymalnym zasięgu); linearyzacja nieważna".into())
    }
}

fn ellipse(
    covariance: [[f64; 2]; 2],
    center: [f64; 2],
    probability: f64,
    scale: f64,
) -> DispersionEllipse {
    let (a, b, c) = (
        covariance[0][0] * scale,
        covariance[0][1] * scale,
        covariance[1][1] * scale,
    );
    let mean = 0.5 * (a + c);
    let spread = (0.25 * (a - c).powi(2) + b * b).sqrt();
    let major = (mean + spread).max(0.0);
    let minor = (mean - spread).max(0.0);
    // Angle of the major axis from east towards north.
    let angle = if spread <= 1e-300 {
        0.0
    } else {
        0.5 * (2.0 * b).atan2(a - c)
    };
    let k = chi2_two_dof_radius(probability);
    let (major_axis, minor_axis) = ([angle.cos(), angle.sin()], [-angle.sin(), angle.cos()]);
    let (semi_major, semi_minor) = (k * major.sqrt(), k * minor.sqrt());
    let outline = (0..72)
        .map(|index| {
            let t = std::f64::consts::TAU * f64::from(index) / 72.0;
            std::array::from_fn(|axis| {
                center[axis]
                    + semi_major * t.cos() * major_axis[axis]
                    + semi_minor * t.sin() * minor_axis[axis]
            })
        })
        .collect();
    DispersionEllipse {
        probability,
        center_offset: center,
        semi_major,
        semi_minor,
        major_axis_azimuth_deg: (90.0 - angle.to_degrees()).rem_euclid(180.0),
        outline,
    }
}

fn halton(mut index: u32, base: u32) -> f64 {
    let mut fraction = 1.0;
    let mut result = 0.0;
    while index > 0 {
        fraction /= f64::from(base);
        result += fraction * f64::from(index % base);
        index /= base;
    }
    result
}

/// Deterministic quasi-Monte-Carlo impact samples (target-centred offsets).
/// Works for degenerate (rank-1/zero) covariances, unlike a density integral.
fn samples(covariance: [[f64; 2]; 2], center: [f64; 2], count: u32) -> Vec<[f64; 2]> {
    let (a, b, c) = (covariance[0][0], covariance[0][1], covariance[1][1]);
    // Cholesky of a PSD 2×2 with a tolerant zero pivot.
    let l11 = a.max(0.0).sqrt();
    let l21 = if l11 > 0.0 { b / l11 } else { 0.0 };
    let l22 = (c - l21 * l21).max(0.0).sqrt();
    (1..=count)
        .map(|index| {
            let u1 = halton(index, 2).max(1e-12);
            let u2 = halton(index, 3);
            let radius = (-2.0 * u1.ln()).sqrt();
            let (z1, z2) = (
                radius * (std::f64::consts::TAU * u2).cos(),
                radius * (std::f64::consts::TAU * u2).sin(),
            );
            [center[0] + l11 * z1, center[1] + l21 * z1 + l22 * z2]
        })
        .collect()
}

fn coverage_probability(points: &[[f64; 2]], at: [f64; 2], radius: f64) -> f64 {
    let inside = points
        .iter()
        .filter(|p| (p[0] - at[0]).hypot(p[1] - at[1]) <= radius)
        .count();
    inside as f64 / points.len() as f64
}

pub fn salvo_pattern(profile: &WeaponProfile, input: &SalvoInput) -> Result<SalvoPattern, String> {
    let model = Model::from_profile(profile, &input.unit)?;
    let options = &input.options;
    dynamics::validate_time_and_step(input.flight_time, options.step_seconds)?;
    let salvo_size = input.salvo_size.unwrap_or(profile.salvo_size);
    if !input.distance.is_finite()
        || input.distance <= 0.0
        || input.distance > 1e9
        || !input.height_delta.is_finite()
        || input.height_delta.abs() > 1e9
        || !input.azimuth_deg.is_finite()
        || input.azimuth_deg.abs() > 1e6
        || !input.elevation_deg.is_finite()
        || input.elevation_deg < profile.min_elevation_deg
        || input.elevation_deg > profile.max_elevation_deg
        || !(1..=1000).contains(&salvo_size)
        || !(1..=10_000_000).contains(&options.max_work_steps)
        || !(64..=65_536).contains(&options.coverage_samples)
        || !(3..=101).contains(&options.coverage_grid)
        || input
            .effect_radius
            .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 1e6)
        || input
            .area_radius
            .is_some_and(|r| !r.is_finite() || r <= 0.0 || r > 1e6)
        || (input.area_radius.is_some() && input.effect_radius.is_none())
    {
        return Err(
            "Niepoprawny cel, łuk, liczba pocisków, promień efektu lub limity prognozy salwy"
                .into(),
        );
    }
    let launch = profile
        .salvo
        .as_ref()
        .filter(|s| s.require_parameters().is_ok());
    let mut missing = Vec::new();
    let mut assumptions = vec![
        "Prognoza salwy: cel i wyrzutnia nieruchome; ten sam nominalny łuk dla każdego pocisku".to_string(),
        "Błędy pocisków niezależne i o średniej zero; brak wspólnego błędu celowania, wiatru i rozstawu prowadnic".to_string(),
        "Elipsy z rozkładu normalnego: linearyzacja różnicami centralnymi o kroku 1σ (z poprawką drugiego rzędu środka)".to_string(),
        format!("Liczba pocisków: {salvo_size}{}", if input.salvo_size.is_some() { " (podana jawnie dla tej prognozy)" } else { " (salvoSize profilu)" }),
        format!("Osie wschód/północ względem celu; jednostka {}", input.unit),
    ];
    let source;
    let status;
    let mut sigmas: Vec<(&str, [f64; 3])> = Vec::new();
    if let Some(block) = launch {
        if profile.dispersion_sigma.is_some() {
            return Err("Niejednoznaczne źródło rozrzutu: profil ma jednocześnie rozrzut startowy salwy i dispersionSigma trafień; usuń jedno z nich".into());
        }
        source = "launch_dispersion";
        status = if profile.calibrated && block.status == "beta" {
            "beta"
        } else {
            "experimental"
        };
        match block.angular_sigma_mrad {
            Some(mrad) => {
                let radians = mrad * 1e-3;
                sigmas.push(("elewacja", [radians, 0.0, 0.0]));
                sigmas.push(("kierunek poprzeczny", [0.0, radians, 0.0]));
                assumptions.push(format!("Rozrzut kątowy 1σ = {mrad} mrad na każdą z dwóch osi poprzecznych (interpretacja aplikacji); źródło: {}", block.source));
            }
            None => missing.push(
                "Rozrzut kątowy: brak danych — nieuwzględniony; elipsa może być zaniżona"
                    .to_string(),
            ),
        }
        match block.speed_sigma {
            Some(sigma) => {
                sigmas.push(("prędkość", [0.0, 0.0, sigma]));
                assumptions.push(format!("Rozrzut prędkości 1σ = {sigma} {}/s", input.unit));
            }
            None => missing.push("Rozrzut prędkości początkowej: brak danych — nieuwzględniony; elipsa może być zaniżona".to_string()),
        }
        assumptions.push(format!("Status parametrów rozrzutu: {}", block.status));
    } else if let Some(sigma) = profile.dispersion_sigma {
        source = "impact_sigma";
        status = if profile.calibrated {
            "beta"
        } else {
            "experimental"
        };
        assumptions.push(format!(
            "Izotropowy rozrzut trafień σ = {sigma} {} z kalibracji profilu (dispersionSigma)",
            input.unit
        ));
    } else {
        return Err("Brak danych rozrzutu: profil nie ma jawnego rozrzutu startowego salwy ani dispersionSigma — prognoza salwy niedostępna".into());
    }
    let mut impact = Impact {
        model,
        input,
        budget: Budget {
            used: 0,
            maximum: options.max_work_steps,
        },
    };
    let phi = input.azimuth_deg.rem_euclid(360.0).to_radians();
    let target = [input.distance * phi.sin(), input.distance * phi.cos()];
    let nominal = impact.point(0.0, 0.0, 0.0)?;
    let nominal_miss = (nominal[0] - target[0]).hypot(nominal[1] - target[1]);
    if nominal_miss > (0.002 * input.distance).max(0.1) {
        return Err(format!("Podany łuk nie trafia w cel w tym modelu (błąd {nominal_miss:.3} {}); przelicz rozwiązanie", input.unit));
    }
    let mut covariance = [[0.0; 2]; 2];
    let mut center = [nominal[0] - target[0], nominal[1] - target[1]];
    let mut nonlinearity: f64 = 0.0;
    for (_, sigma) in sigmas.iter().filter(|(_, s)| s.iter().any(|v| *v > 0.0)) {
        let plus = impact.point(sigma[0], sigma[1], sigma[2])?;
        let minus = impact.point(-sigma[0], -sigma[1], -sigma[2])?;
        let first: [f64; 2] = std::array::from_fn(|i| 0.5 * (plus[i] - minus[i]));
        let second: [f64; 2] =
            std::array::from_fn(|i| 0.5 * (plus[i] + minus[i] - 2.0 * nominal[i]));
        for (row, cov_row) in covariance.iter_mut().enumerate() {
            for (col, value) in cov_row.iter_mut().enumerate() {
                *value += first[row] * first[col];
            }
        }
        for (axis, value) in center.iter_mut().enumerate() {
            *value += second[axis];
        }
        let first_size = first[0].hypot(first[1]);
        if first_size > 0.0 {
            nonlinearity = nonlinearity.max(second[0].hypot(second[1]) / first_size);
        }
    }
    if source == "impact_sigma" {
        let sigma = profile.dispersion_sigma.unwrap_or(0.0);
        covariance = [[sigma * sigma, 0.0], [0.0, sigma * sigma]];
    }
    if nonlinearity > 0.2 {
        assumptions.push(format!("Silna nieliniowość (wskaźnik {nonlinearity:.2}): rzeczywisty rozkład nie jest eliptyczny; elipsa przybliżona"));
    }
    let n = f64::from(salvo_size);
    let single = ellipse(covariance, center, 0.95, 1.0);
    // Per-projectile probability p with p^N = 0.95: all N inside together.
    let envelope_probability = 0.95_f64.powf(1.0 / n);
    let mut salvo_envelope = ellipse(covariance, center, envelope_probability, 1.0);
    salvo_envelope.probability = 0.95;
    let mean_point_of_impact = ellipse(covariance, center, 0.95, 1.0 / n);
    let range_axis = [phi.sin(), phi.cos()];
    let cross_axis = [phi.cos(), -phi.sin()];
    let project = |axis: [f64; 2]| {
        (axis[0] * axis[0] * covariance[0][0]
            + 2.0 * axis[0] * axis[1] * covariance[0][1]
            + axis[1] * axis[1] * covariance[1][1])
            .max(0.0)
            .sqrt()
    };
    let coverage = input.effect_radius.map(|radius| {
        let points = samples(covariance, center, options.coverage_samples);
        let single_hit = coverage_probability(&points, [0.0, 0.0], radius);
        let any = |p: f64| 1.0 - (1.0 - p).powf(n);
        let area_fraction = input.area_radius.map(|area| {
            let grid = options.coverage_grid;
            let mut total = 0.0;
            let mut count = 0u32;
            for i in 0..grid {
                for j in 0..grid {
                    let x = -area + 2.0 * area * (f64::from(i) + 0.5) / f64::from(grid);
                    let y = -area + 2.0 * area * (f64::from(j) + 0.5) / f64::from(grid);
                    if x.hypot(y) <= area {
                        total += any(coverage_probability(&points, [x, y], radius));
                        count += 1;
                    }
                }
            }
            total / f64::from(count.max(1))
        });
        SalvoCoverage {
            effect_radius: radius,
            probability_any_within_effect: any(single_hit),
            expected_projectiles_within_effect: n * single_hit,
            area_radius: input.area_radius,
            expected_area_fraction: area_fraction,
            samples: options.coverage_samples,
        }
    });
    if coverage.is_some() {
        assumptions.push("Pokrycie: promień efektu podany przez użytkownika; deterministyczne próbki quasi-losowe; nie jest to model obrażeń".into());
    }
    let timing = launch
        .and_then(|block| block.ripple_interval)
        .filter(|_| salvo_size > 1)
        .map(|interval| SalvoTiming {
            ripple_interval: interval,
            first_impact: input.flight_time,
            last_impact: input.flight_time + interval * (n - 1.0),
        });
    if !profile.calibrated {
        assumptions.push(
            "Dane eksperymentalne bez lokalnej kalibracji; porównaj z obserwowanymi trafieniami"
                .into(),
        );
    }
    assumptions.push(format!(
        "Kroki RK4: {} / {}",
        impact.budget.used, options.max_work_steps
    ));
    Ok(SalvoPattern {
        unit: input.unit.clone(),
        status: status.into(),
        salvo_size,
        dispersion_source: source.into(),
        target_position: target,
        center_offset: center,
        covariance,
        range_sigma: project(range_axis),
        cross_range_sigma: project(cross_axis),
        single_projectile: single,
        salvo_envelope,
        mean_point_of_impact,
        nonlinearity,
        coverage,
        timing,
        missing_components: missing,
        assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_profiles::SalvoDispersion;

    fn grad() -> WeaponProfile {
        let mut p = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.id == "community-bm_21_grad-1")
            .unwrap();
        p.source_acknowledged = true;
        p
    }
    fn input_for(p: &WeaponProfile, distance: f64, height: f64, high: bool) -> SalvoInput {
        let solution = crate::solve(p, distance, height).unwrap();
        let arc = solution.arcs.iter().find(|a| a.high_arc == high).unwrap();
        SalvoInput {
            distance,
            height_delta: height,
            azimuth_deg: 30.0,
            elevation_deg: arc.elevation_deg,
            flight_time: arc.flight_time,
            unit: p.unit.clone(),
            salvo_size: None,
            effect_radius: None,
            area_radius: None,
            options: SalvoOptions::default(),
        }
    }

    #[test]
    fn grad_wiki_angular_sigma_matches_analytic_vacuum_sensitivity() {
        let p = grad();
        let input = input_for(&p, 1200.0, 0.0, false);
        let pattern = salvo_pattern(&p, &input).unwrap();
        let (v, g) = (180.0, 17.64);
        let theta = input.elevation_deg.to_radians();
        let sigma = 1.75e-3;
        // Flat ground: dR/dθ = 2 v² cos 2θ / g; cross-range = R × angle / cos θ … here
        // the cross perturbation tilts the plane by tan(σ)/cos θ in azimuth.
        let range_expected = (2.0 * v * v * (2.0 * theta).cos() / g).abs() * sigma;
        let cross_expected = 1200.0 * sigma / theta.cos();
        assert!(
            (pattern.range_sigma - range_expected).abs() / range_expected < 0.01,
            "{} {}",
            pattern.range_sigma,
            range_expected
        );
        assert!(
            (pattern.cross_range_sigma - cross_expected).abs() / cross_expected < 0.01,
            "{} {}",
            pattern.cross_range_sigma,
            cross_expected
        );
        assert_eq!(pattern.status, "experimental");
        assert_eq!(pattern.dispersion_source, "launch_dispersion");
        assert_eq!(pattern.salvo_size, 1);
        assert!(pattern
            .missing_components
            .iter()
            .any(|m| m.contains("prędkości")));
        assert!(pattern.center_offset[0].hypot(pattern.center_offset[1]) < 0.5);
        assert!(pattern
            .assumptions
            .iter()
            .any(|a| a.contains("multicrew.wiki")));
    }

    #[test]
    fn salvo_envelope_grows_and_mean_point_shrinks_with_explicit_count() {
        let p = grad();
        let mut input = input_for(&p, 1500.0, 20.0, true);
        input.salvo_size = Some(40);
        let pattern = salvo_pattern(&p, &input).unwrap();
        let single = &pattern.single_projectile;
        let k95 = chi2_two_dof_radius(0.95);
        let k_all = chi2_two_dof_radius(0.95_f64.powf(1.0 / 40.0));
        assert!((pattern.salvo_envelope.semi_major / single.semi_major - k_all / k95).abs() < 1e-9);
        assert!(
            (pattern.mean_point_of_impact.semi_major * 40.0_f64.sqrt() - single.semi_major).abs()
                < 1e-9
        );
        assert_eq!(single.outline.len(), 72);
        // Every outline point lies on the ellipse boundary.
        for point in &single.outline {
            let d = [
                point[0] - single.center_offset[0],
                point[1] - single.center_offset[1],
            ];
            let a = (90.0 - single.major_axis_azimuth_deg).to_radians();
            let (u, w) = (
                d[0] * a.cos() + d[1] * a.sin(),
                -d[0] * a.sin() + d[1] * a.cos(),
            );
            let r = (u / single.semi_major).powi(2) + (w / single.semi_minor).powi(2);
            assert!((r - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn coverage_is_bounded_monotonic_and_matches_closed_form_for_isotropic_sigma() {
        let mut p = grad();
        p.salvo = None;
        p.dispersion_sigma = Some(10.0);
        let mut input = input_for(&p, 1000.0, 0.0, false);
        input.effect_radius = Some(10.0);
        input.area_radius = Some(30.0);
        input.salvo_size = Some(1);
        let one = salvo_pattern(&p, &input).unwrap();
        let c = one.coverage.as_ref().unwrap();
        // Rayleigh: P(|X| ≤ r) = 1 − exp(−r²/2σ²) = 0.3935 for r = σ.
        assert!((c.probability_any_within_effect - (1.0 - (-0.5_f64).exp())).abs() < 0.01);
        assert_eq!(one.dispersion_source, "impact_sigma");
        input.salvo_size = Some(12);
        let many = salvo_pattern(&p, &input).unwrap();
        let m = many.coverage.as_ref().unwrap();
        assert!(m.probability_any_within_effect > c.probability_any_within_effect);
        assert!(m.expected_area_fraction.unwrap() > c.expected_area_fraction.unwrap());
        assert!((0.0..=1.0).contains(&m.expected_area_fraction.unwrap()));
        assert!(
            (m.expected_projectiles_within_effect - 12.0 * (1.0 - (-0.5_f64).exp())).abs() < 0.15
        );
    }

    #[test]
    fn missing_dispersion_data_blocks_forecast_instead_of_assuming_spread() {
        let mut p = grad();
        p.salvo = None;
        let input = input_for(&p, 1000.0, 0.0, false);
        assert!(salvo_pattern(&p, &input)
            .unwrap_err()
            .contains("Brak danych rozrzutu"));
        p.salvo = Some(SalvoDispersion {
            unit: p.unit.clone(),
            angular_sigma_mrad: None,
            speed_sigma: None,
            ripple_interval: None,
            calibrated: false,
            status: "no_data".into(),
            source: String::new(),
        });
        assert!(salvo_pattern(&p, &input)
            .unwrap_err()
            .contains("Brak danych rozrzutu"));
        let mut both = grad();
        both.dispersion_sigma = Some(3.0);
        assert!(salvo_pattern(&both, &input)
            .unwrap_err()
            .contains("Niejednoznaczne"));
    }

    #[test]
    fn rejects_unacknowledged_guided_mismatched_arc_and_invalid_inputs() {
        let mut p = grad();
        let input = input_for(&p, 1000.0, 0.0, false);
        p.source_acknowledged = false;
        assert!(salvo_pattern(&p, &input).is_err());
        let p = grad();
        let mut wrong = input.clone();
        wrong.elevation_deg += 1.0;
        assert!(salvo_pattern(&p, &wrong)
            .unwrap_err()
            .contains("nie trafia"));
        let patches: [fn(&mut SalvoInput); 5] = [
            |i: &mut SalvoInput| i.salvo_size = Some(0),
            |i: &mut SalvoInput| i.effect_radius = Some(-1.0),
            |i: &mut SalvoInput| i.area_radius = Some(5.0),
            |i: &mut SalvoInput| i.unit = "game_units".into(),
            |i: &mut SalvoInput| i.flight_time = f64::NAN,
        ];
        for patch in patches {
            let mut bad = input.clone();
            patch(&mut bad);
            assert!(salvo_pattern(&p, &bad).is_err());
        }
        let mut guided = mtc_profiles::community_profiles()
            .into_iter()
            .find(|p| p.category == mtc_profiles::Category::GuidedMissile)
            .unwrap();
        guided.source_acknowledged = true;
        assert!(salvo_pattern(&guided, &input).is_err());
    }

    #[test]
    fn drag_model_dispersion_uses_the_numerical_trajectory_and_ripple_timing() {
        let mut p = grad();
        p.calibrated = true;
        p.status = "beta".into();
        p.dynamics = Some(mtc_profiles::ProjectileDynamics {
            model: mtc_profiles::DynamicsModel::Drag,
            unit: p.unit.clone(),
            linear_drag: Some(0.01),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test; nie MTC".into(),
        });
        let salvo = p.salvo.as_mut().unwrap();
        salvo.speed_sigma = Some(1.0);
        salvo.ripple_interval = Some(0.5);
        let solution = crate::solve_dynamic(
            &p,
            &crate::DynamicSolveInput {
                distance: 1000.0,
                height_delta: 0.0,
                azimuth_deg: 30.0,
                unit: p.unit.clone(),
                options: crate::DynamicSolveOptions::default(),
            },
        )
        .unwrap();
        let arc = &solution.arcs[0];
        let input = SalvoInput {
            distance: 1000.0,
            height_delta: 0.0,
            azimuth_deg: 30.0,
            elevation_deg: arc.elevation_deg,
            flight_time: arc.flight_time,
            unit: p.unit.clone(),
            salvo_size: Some(10),
            effect_radius: None,
            area_radius: None,
            options: SalvoOptions::default(),
        };
        let pattern = salvo_pattern(&p, &input).unwrap();
        assert!(pattern.missing_components.is_empty());
        assert_eq!(pattern.status, "experimental");
        let timing = pattern.timing.unwrap();
        assert!((timing.last_impact - arc.flight_time - 4.5).abs() < 1e-12);
        let vacuum = salvo_pattern(&grad(), &input_for(&grad(), 1000.0, 0.0, false)).unwrap();
        assert!((pattern.range_sigma - vacuum.range_sigma).abs() > 0.05);
    }
}
