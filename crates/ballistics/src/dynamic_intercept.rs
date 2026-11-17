//! Moving-target intercept for the numerical drag/rocket models (status beta).
//!
//! Unknowns: elevation, azimuth and flight time. Residual: projectile position
//! (RK4, world frame, air at rest) minus constant-velocity target position at
//! that time. Damped Newton iterations start from the static numerical
//! solution to the current target position. Every iteration count, residual and
//! failure is reported; a non-converged branch is never returned as a result.
use crate::dynamics::{self, norm, Budget, Model, State};
use crate::{solve_dynamic, DynamicSolveInput, DynamicSolveOptions};
use mtc_profiles::WeaponProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicInterceptOptions {
    pub step_seconds: f64,
    /// Shared RK4 budget: half for the static seed search, the rest for Newton.
    pub max_work_steps: u32,
    pub max_evaluations: u32,
    pub angle_samples: u32,
    /// Numerical positional tolerance in the profile unit; not game accuracy.
    pub hit_tolerance: f64,
    pub max_iterations: u32,
}
impl Default for DynamicInterceptOptions {
    fn default() -> Self {
        Self {
            step_seconds: 0.02,
            max_work_steps: 3_000_000,
            max_evaluations: 512,
            angle_samples: 65,
            hit_tolerance: 0.05,
            max_iterations: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicInterceptInput {
    /// East, north, up of the target relative to the muzzle.
    pub relative: [f64; 3],
    pub target_velocity: [f64; 3],
    pub own_velocity: [f64; 3],
    /// No nonzero platform-motion compensation without an explicit choice.
    pub inherit_own_velocity: Option<bool>,
    pub unit: String,
    /// Horizon of the observed constant target motion and of the flight.
    pub max_time: f64,
    #[serde(default)]
    pub options: DynamicInterceptOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicInterceptArc {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    pub flight_time: f64,
    /// Muzzle-relative launch velocity; magnitude equals the profile speed.
    pub launch_velocity: [f64; 3],
    pub world_initial_velocity: [f64; 3],
    pub target_position_at_impact: [f64; 3],
    pub projectile_position_at_impact: [f64; 3],
    /// Relative target motion during the flight, (target − inherited) × time.
    pub lead: [f64; 3],
    pub miss_distance: f64,
    pub iterations: u32,
    pub high_arc: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptBranchReport {
    pub seed_elevation_deg: f64,
    pub seed_flight_time: f64,
    pub converged: bool,
    pub iterations: u32,
    /// Last evaluated |miss| in the profile unit, when one was computed.
    pub final_miss: Option<f64>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicInterceptSolution {
    pub arcs: Vec<DynamicInterceptArc>,
    pub branches: Vec<InterceptBranchReport>,
    /// Result status: `beta` for a locally calibrated profile, else `experimental`.
    pub status: String,
    pub unit: String,
    pub steps_used: u32,
    pub assumptions: Vec<String>,
}

fn direction(elevation: f64, azimuth: f64) -> [f64; 3] {
    [
        elevation.cos() * azimuth.sin(),
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
    ]
}

/// Gaussian elimination with partial pivoting; `None` for a singular system.
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    let scale = a.iter().flatten().map(|v| v.abs()).fold(0.0_f64, f64::max);
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    for column in 0..3 {
        let pivot =
            (column..3).max_by(|&i, &j| a[i][column].abs().total_cmp(&a[j][column].abs()))?;
        if a[pivot][column].abs() <= 1e-12 * scale {
            return None;
        }
        a.swap(column, pivot);
        b.swap(column, pivot);
        let pivot_row = a[column];
        for row in column + 1..3 {
            let factor = a[row][column] / pivot_row[column];
            for (value, pivot_value) in a[row].iter_mut().zip(pivot_row).skip(column) {
                *value -= factor * pivot_value;
            }
            b[row] -= factor * b[column];
        }
    }
    let mut x = [0.0; 3];
    for row in (0..3).rev() {
        let tail: f64 = (row + 1..3).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Converged unknowns, iterations, miss and terminal state.
type Converged = ([f64; 3], u32, f64, State);
/// Diagnostic message, iterations and the last evaluated miss.
type Diverged = (String, u32, Option<f64>);

struct Problem<'a> {
    model: Model,
    input: &'a DynamicInterceptInput,
    inherited: [f64; 3],
    budget: Budget,
}

impl Problem<'_> {
    fn residual(&mut self, x: [f64; 3]) -> Result<([f64; 3], State), String> {
        let launch = direction(x[0], x[1]);
        let initial = std::array::from_fn(|i| launch[i] * self.model.speed + self.inherited[i]);
        let state = self.model.propagate(
            launch,
            initial,
            x[2],
            self.input.options.step_seconds,
            &mut self.budget,
        )?;
        let residual = std::array::from_fn(|i| {
            state.position[i] - (self.input.relative[i] + self.input.target_velocity[i] * x[2])
        });
        Ok((residual, state))
    }

    /// Damped Newton on (elevation, azimuth, time). Returns the converged
    /// unknowns and iteration count, or a diagnostic message with last miss.
    fn newton(&mut self, seed: [f64; 3], limits: (f64, f64)) -> Result<Converged, Diverged> {
        let input = self.input;
        let options = &input.options;
        let max_time = input.max_time;
        let clamp = |x: [f64; 3]| {
            [
                x[0].clamp(-89.5_f64.to_radians(), 89.5_f64.to_radians()),
                x[1],
                x[2].clamp(1e-3_f64.min(max_time), max_time),
            ]
        };
        let mut x = clamp(seed);
        let (mut f, mut state) = self.residual(x).map_err(|e| (e, 0, None))?;
        let mut miss = norm(f);
        for iteration in 0..=options.max_iterations {
            if miss <= options.hit_tolerance {
                let elevation = x[0].to_degrees();
                if elevation < limits.0 || elevation > limits.1 {
                    return Err((
                        format!("Zbieżność poza limitami elewacji profilu ({elevation:.4}°)"),
                        iteration,
                        Some(miss),
                    ));
                }
                if x[2] > max_time {
                    return Err(("Czas lotu poza horyzontem".into(), iteration, Some(miss)));
                }
                return Ok((x, iteration, miss, state));
            }
            if iteration == options.max_iterations {
                break;
            }
            let epsilon = 1e-6;
            let column = |problem: &mut Self, index: usize| {
                let mut shifted = x;
                shifted[index] += epsilon;
                problem
                    .residual(shifted)
                    .map(|(g, _)| std::array::from_fn::<f64, 3, _>(|i| (g[i] - f[i]) / epsilon))
            };
            let d_elevation = column(self, 0).map_err(|e| (e, iteration, Some(miss)))?;
            let d_azimuth = column(self, 1).map_err(|e| (e, iteration, Some(miss)))?;
            let d_time: [f64; 3] =
                std::array::from_fn(|i| state.velocity[i] - input.target_velocity[i]);
            let jacobian =
                std::array::from_fn(|row| [d_elevation[row], d_azimuth[row], d_time[row]]);
            let Some(mut delta) = solve3(jacobian, f.map(|v| -v)) else {
                return Err((
                    "Osobliwa macierz Jacobiego (np. styczny maksymalny zasięg); brak kierunku iteracji".into(),
                    iteration,
                    Some(miss),
                ));
            };
            let limit = [0.2, 0.5, 0.5 * x[2] + 1.0];
            let ratio = (0..3)
                .map(|i| delta[i].abs() / limit[i])
                .fold(1.0_f64, f64::max);
            delta = delta.map(|v| v / ratio);
            let mut accepted = false;
            let mut alpha = 1.0;
            for _ in 0..12 {
                let candidate = clamp(std::array::from_fn(|i| x[i] + alpha * delta[i]));
                match self.residual(candidate) {
                    Ok((g, next)) if norm(g) < miss * (1.0 - 1e-4 * alpha) => {
                        x = candidate;
                        f = g;
                        state = next;
                        miss = norm(g);
                        accepted = true;
                        break;
                    }
                    Ok(_) => alpha *= 0.5,
                    Err(e) if e.contains("limit kroków") => {
                        return Err((e, iteration + 1, Some(miss)));
                    }
                    Err(_) => alpha *= 0.5,
                }
            }
            if !accepted {
                return Err((
                    "Iteracja nie zmniejsza błędu trafienia; brak zbieżności".into(),
                    iteration + 1,
                    Some(miss),
                ));
            }
        }
        Err((
            format!(
                "Brak zbieżności w limicie {} iteracji",
                options.max_iterations
            ),
            options.max_iterations,
            Some(miss),
        ))
    }
}

/// Constant target velocity, stationary air, declared platform inheritance.
/// Guided weapons remain blocked by `can_calculate`; no guidance law exists.
pub fn solve_dynamic_intercept(
    profile: &WeaponProfile,
    input: &DynamicInterceptInput,
) -> Result<DynamicInterceptSolution, String> {
    let model = Model::from_profile(profile, &input.unit)?;
    let options = &input.options;
    dynamics::validate_time_and_step(input.max_time, options.step_seconds)?;
    if input
        .relative
        .iter()
        .chain(input.target_velocity.iter())
        .chain(input.own_velocity.iter())
        .any(|v| !v.is_finite() || v.abs() > 1e12)
        || !(1..=200).contains(&options.max_iterations)
        || !(2..=10_000_000).contains(&options.max_work_steps)
    {
        return Err("Wymagane pełne 3D pozycji i prędkości oraz poprawne limity iteracji".into());
    }
    let horizontal = input.relative[0].hypot(input.relative[1]);
    if horizontal <= 1e-9 {
        return Err("Cel bez odległości poziomej — azymut nieokreślony".into());
    }
    let inherit = match input.inherit_own_velocity {
        Some(value) => value,
        None if norm(input.own_velocity) <= 1e-9 => false,
        None => return Err("Brak potwierdzonego modelu dziedziczenia prędkości stanowiska".into()),
    };
    let inherited = input.own_velocity.map(|v| if inherit { v } else { 0.0 });
    let seed_budget = options.max_work_steps / 2;
    let seeds = solve_dynamic(
        profile,
        &DynamicSolveInput {
            distance: horizontal,
            height_delta: input.relative[2],
            azimuth_deg: input.relative[0].atan2(input.relative[1]).to_degrees(),
            unit: input.unit.clone(),
            options: DynamicSolveOptions {
                max_time: input.max_time,
                step_seconds: options.step_seconds,
                max_work_steps: seed_budget,
                max_evaluations: options.max_evaluations,
                angle_samples: options.angle_samples,
                hit_tolerance: options.hit_tolerance,
            },
        },
    )
    .map_err(|e| {
        format!(
            "Brak rozwiązania statycznego do bieżącej pozycji celu (punkt startowy iteracji): {e}"
        )
    })?;
    let mut problem = Problem {
        model,
        input,
        inherited,
        budget: Budget {
            used: 0,
            maximum: options.max_work_steps - seeds.steps_used.min(seed_budget),
        },
    };
    let mut arcs: Vec<DynamicInterceptArc> = Vec::new();
    let mut branches = Vec::new();
    for seed in &seeds.arcs {
        let start = [
            seed.elevation_deg.to_radians(),
            seed.azimuth_deg.to_radians(),
            seed.flight_time,
        ];
        let limits = (profile.min_elevation_deg, profile.max_elevation_deg);
        match problem.newton(start, limits) {
            Ok((x, iterations, miss, state)) => {
                let launch = direction(x[0], x[1]);
                let launch_velocity = launch.map(|v| v * model.speed);
                let target: [f64; 3] =
                    std::array::from_fn(|i| input.relative[i] + input.target_velocity[i] * x[2]);
                branches.push(InterceptBranchReport {
                    seed_elevation_deg: seed.elevation_deg,
                    seed_flight_time: seed.flight_time,
                    converged: true,
                    iterations,
                    final_miss: Some(miss),
                    message: "Zbieżność w tolerancji numerycznej".into(),
                });
                let arc = DynamicInterceptArc {
                    elevation_deg: x[0].to_degrees(),
                    azimuth_deg: x[1].to_degrees().rem_euclid(360.0),
                    flight_time: x[2],
                    launch_velocity,
                    world_initial_velocity: std::array::from_fn(|i| {
                        launch_velocity[i] + inherited[i]
                    }),
                    target_position_at_impact: target,
                    projectile_position_at_impact: state.position,
                    lead: std::array::from_fn(|i| (input.target_velocity[i] - inherited[i]) * x[2]),
                    miss_distance: miss,
                    iterations,
                    high_arc: false,
                };
                let duplicate = arcs.iter().any(|a| {
                    (a.elevation_deg - arc.elevation_deg).abs() < 1e-4
                        && (a.flight_time - arc.flight_time).abs() < 1e-3
                });
                if !duplicate {
                    arcs.push(arc);
                }
            }
            Err((message, iterations, final_miss)) => branches.push(InterceptBranchReport {
                seed_elevation_deg: seed.elevation_deg,
                seed_flight_time: seed.flight_time,
                converged: false,
                iterations,
                final_miss,
                message,
            }),
        }
    }
    if arcs.is_empty() {
        let details: Vec<String> = branches
            .iter()
            .map(|b| {
                format!(
                    "start {:.3}°: {} (iteracje {}, błąd {})",
                    b.seed_elevation_deg,
                    b.message,
                    b.iterations,
                    b.final_miss
                        .map_or("brak".into(), |m| format!("{m:.4} {}", input.unit))
                )
            })
            .collect();
        return Err(format!(
            "Brak zbieżnego przechwycenia z oporem/napędem; nie zastępujemy go modelem vacuum ani wyprzedzeniem 2D. {}",
            details.join("; ")
        ));
    }
    arcs.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    if arcs.len() > 1 {
        if let Some(last) = arcs.last_mut() {
            last.high_arc = true;
        }
    }
    let mut assumptions = dynamics::assumptions(profile);
    for assumption in &mut assumptions {
        *assumption = assumption.replace("nieruchome stanowisko; ", "");
    }
    assumptions.extend([
        "Status beta: iteracyjne przechwycenie (Newton na elewacji, azymucie i czasie lotu z RK4), start z rozwiązania statycznego do bieżącej pozycji celu".to_string(),
        "Cel utrzymuje stałą obserwowaną prędkość 3D przez cały lot; brak przyspieszeń i manewrów".to_string(),
        format!("Dziedziczenie prędkości stanowiska: {inherit}; opór liczony względem nieruchomego powietrza w układzie świata"),
        "Napęd rakiety działa wzdłuż kierunku lufy w chwili startu; naprowadzanie nie jest modelowane".to_string(),
        format!("Tolerancja numeryczna {} {}; zbieżność nie dowodzi zgodności modelu z grą", options.hit_tolerance, input.unit),
        "Iteracja może pominąć tor nieobecny w rozwiązaniu statycznym; brak wyniku nie dowodzi braku trajektorii w grze".to_string(),
    ]);
    Ok(DynamicInterceptSolution {
        arcs,
        branches,
        status: if profile.calibrated {
            "beta"
        } else {
            "experimental"
        }
        .into(),
        unit: input.unit.clone(),
        steps_used: seeds.steps_used + problem.budget.used,
        assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_profiles::{Category, DynamicsModel, ProjectileDynamics};

    fn vacuum() -> WeaponProfile {
        let mut p = mtc_profiles::built_in()
            .into_iter()
            .find(|p| p.category == Category::TubeArtillery)
            .unwrap();
        p.speed = Some(100.0);
        p.gravity = Some(10.0);
        p.min_elevation_deg = -20.0;
        p.max_elevation_deg = 89.0;
        p.status = "beta".into();
        p.calibrated = true;
        p.source = "Syntetyczny test; brak pomiarów rzeczywistej gry".into();
        p
    }
    fn with_dynamics(model: DynamicsModel, linear: f64, quadratic: f64) -> WeaponProfile {
        let mut p = vacuum();
        let rocket = model == DynamicsModel::Rocket;
        p.dynamics = Some(ProjectileDynamics {
            model,
            unit: p.unit.clone(),
            linear_drag: Some(linear),
            quadratic_drag: Some(quadratic),
            rocket_acceleration: rocket.then_some(15.0),
            burn_time: rocket.then_some(1.5),
            thrust_direction_fixed_launch: rocket.then_some(true),
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczne współczynniki; nie MTC".into(),
        });
        p
    }
    fn input() -> DynamicInterceptInput {
        DynamicInterceptInput {
            relative: [300.0, 120.0, 15.0],
            target_velocity: [-6.0, 4.0, 0.0],
            own_velocity: [0.0; 3],
            inherit_own_velocity: Some(false),
            unit: "game_units".into(),
            max_time: 40.0,
            options: DynamicInterceptOptions::default(),
        }
    }
    /// Independent reconstruction: separate RK4 run with the reported launch.
    fn reconstruct(p: &WeaponProfile, i: &DynamicInterceptInput, arc: &DynamicInterceptArc) {
        let model = Model::from_profile(p, &i.unit).unwrap();
        let launch = arc.launch_velocity.map(|v| v / model.speed);
        let mut budget = Budget {
            used: 0,
            maximum: 1_000_000,
        };
        let state = model
            .propagate(
                launch,
                arc.world_initial_velocity,
                arc.flight_time,
                0.005,
                &mut budget,
            )
            .unwrap();
        let target: [f64; 3] =
            std::array::from_fn(|k| i.relative[k] + i.target_velocity[k] * arc.flight_time);
        let miss = norm(std::array::from_fn(|k| state.position[k] - target[k]));
        assert!(
            miss < i.options.hit_tolerance * 1.5,
            "reconstructed miss {miss}"
        );
        assert!((norm(arc.launch_velocity) - model.speed).abs() < 1e-9);
    }

    #[test]
    fn vacuum_numeric_intercept_matches_closed_form_quartic_for_moving_target() {
        let p = vacuum();
        let i = input();
        let numeric = solve_dynamic_intercept(&p, &i).unwrap();
        let analytic = crate::solve_intercept_3d(
            &p,
            &crate::InterceptInput3d {
                relative: i.relative,
                target_velocity: i.target_velocity,
                own_velocity: i.own_velocity,
                inherit_own_velocity: Some(false),
                unit: i.unit.clone(),
                max_time: i.max_time,
            },
        )
        .unwrap();
        assert_eq!(numeric.arcs.len(), 2);
        assert_eq!(analytic.arcs.len(), 2);
        let mut expected = analytic.arcs.clone();
        expected.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
        for (n, a) in numeric.arcs.iter().zip(&expected) {
            assert!(
                (n.elevation_deg - a.elevation_deg).abs() < 0.01,
                "{} {}",
                n.elevation_deg,
                a.elevation_deg
            );
            assert!((n.azimuth_deg - a.azimuth_deg).abs() < 0.01);
            assert!((n.flight_time - a.flight_time).abs() < 0.01);
            reconstruct(&p, &i, n);
        }
        assert!(numeric.branches.iter().all(|b| b.converged));
    }

    #[test]
    fn drag_intercept_converges_and_differs_from_static_and_vacuum_aim() {
        let p = with_dynamics(DynamicsModel::Drag, 0.02, 0.0001);
        let i = input();
        let solution = solve_dynamic_intercept(&p, &i).unwrap();
        assert!(!solution.arcs.is_empty());
        assert_eq!(solution.status, "beta");
        let vacuum = solve_dynamic_intercept(&vacuum(), &i).unwrap();
        for arc in &solution.arcs {
            reconstruct(&p, &i, arc);
            assert!(arc.iterations >= 1);
            assert!(arc.miss_distance <= i.options.hit_tolerance);
            let closest = vacuum
                .arcs
                .iter()
                .map(|v| (v.elevation_deg - arc.elevation_deg).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(closest > 0.2, "drag changed aim by {closest}°");
        }
        let still = DynamicInterceptInput {
            target_velocity: [0.0; 3],
            ..input()
        };
        let static_aim = solve_dynamic_intercept(&p, &still).unwrap();
        assert!((static_aim.arcs[0].azimuth_deg - solution.arcs[0].azimuth_deg).abs() > 0.3);
    }

    #[test]
    fn rocket_intercept_with_inherited_platform_velocity_hits_in_world_frame() {
        let p = with_dynamics(DynamicsModel::Rocket, 0.01, 0.0);
        let mut i = input();
        i.own_velocity = [3.0, -2.0, 0.5];
        i.inherit_own_velocity = Some(true);
        let solution = solve_dynamic_intercept(&p, &i).unwrap();
        for arc in &solution.arcs {
            reconstruct(&p, &i, arc);
            for k in 0..3 {
                assert!(
                    (arc.world_initial_velocity[k] - arc.launch_velocity[k] - i.own_velocity[k])
                        .abs()
                        < 1e-12
                );
            }
        }
        assert!(solution
            .assumptions
            .iter()
            .any(|a| a.contains("Dziedziczenie prędkości stanowiska: true")));
    }

    #[test]
    fn reports_non_convergence_instead_of_partial_result() {
        let p = with_dynamics(DynamicsModel::Drag, 0.02, 0.0);
        let mut i = input();
        i.options.max_iterations = 1;
        i.target_velocity = [-25.0, 18.0, 0.0];
        match solve_dynamic_intercept(&p, &i) {
            Err(error) => assert!(error.contains("Brak zbieżnego"), "{error}"),
            Ok(solution) => {
                assert!(solution.branches.iter().any(|b| !b.converged));
                for arc in solution.arcs {
                    assert!(arc.miss_distance <= i.options.hit_tolerance);
                }
            }
        }
        // A target escaping faster than the projectile has no intercept.
        let mut escaping = input();
        escaping.target_velocity = [400.0, 0.0, 0.0];
        assert!(solve_dynamic_intercept(&p, &escaping).is_err());
    }

    #[test]
    fn blocks_guided_unit_mismatch_unknown_inheritance_and_bad_limits() {
        let p = with_dynamics(DynamicsModel::Drag, 0.02, 0.0);
        let mut guided = p.clone();
        guided.category = Category::GuidedMissile;
        assert!(solve_dynamic_intercept(&guided, &input()).is_err());
        let mut i = input();
        i.unit = "game_meters".into();
        assert!(solve_dynamic_intercept(&p, &i).is_err());
        let mut i = input();
        i.own_velocity = [1.0, 0.0, 0.0];
        i.inherit_own_velocity = None;
        assert!(solve_dynamic_intercept(&p, &i).is_err());
        let mut i = input();
        i.relative[2] = f64::NAN;
        assert!(solve_dynamic_intercept(&p, &i).is_err());
        let mut i = input();
        i.options.max_iterations = 0;
        assert!(solve_dynamic_intercept(&p, &i).is_err());
        let mut i = input();
        i.options.max_work_steps = 10;
        assert!(solve_dynamic_intercept(&p, &i).is_err());
        let mut uncalibrated = p.clone();
        uncalibrated.dynamics.as_mut().unwrap().calibrated = false;
        assert!(solve_dynamic_intercept(&uncalibrated, &input()).is_err());
    }

    #[test]
    fn solves_small_linear_systems_and_rejects_singular_jacobians() {
        let x = solve3(
            [[2.0, 1.0, 0.0], [0.0, 3.0, 1.0], [1.0, 0.0, 4.0]],
            [3.0, 4.0, 5.0],
        )
        .unwrap();
        for v in x {
            assert!((v - 1.0).abs() < 1e-12);
        }
        assert!(solve3(
            [[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 1.0, 1.0]],
            [1.0, 2.0, 3.0]
        )
        .is_none());
    }
}
