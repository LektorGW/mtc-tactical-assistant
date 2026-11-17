use mtc_profiles::WeaponProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterceptInput3d {
    /// East, north, up. Up is observed target height minus observed muzzle height.
    pub relative: [f64; 3],
    pub target_velocity: [f64; 3],
    pub own_velocity: [f64; 3],
    /// No nonzero platform-motion compensation without an explicit inheritance model.
    pub inherit_own_velocity: Option<bool>,
    pub unit: String,
    pub max_time: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptArc3d {
    pub flight_time: f64,
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    /// Muzzle-relative launch velocity; magnitude equals the calibrated speed.
    pub launch_velocity: [f64; 3],
    pub world_initial_velocity: [f64; 3],
    pub target_position_at_impact: [f64; 3],
    /// Relative velocity lead only; gravity correction is in aim_point/angles.
    pub lead: [f64; 3],
    pub aim_point: [f64; 3],
    pub miss_distance: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptSolution3d {
    pub arcs: Vec<InterceptArc3d>,
    pub unit: String,
    pub assumptions: Vec<String>,
}

fn norm(vector: [f64; 3]) -> f64 {
    vector[0].hypot(vector[1]).hypot(vector[2])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn evaluate(coefficients: &[f64], x: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |sum, c| sum * x + c)
}

/// Isolate all roots using derivative extrema, including tangent/double roots.
/// The normalized search variable is time/max_time within [0,1]. No unbounded
/// simulation loop or change-sign-only sampling that misses maximum-range shots.
fn roots_interval(coefficients: &[f64], low: f64, high: f64) -> Vec<f64> {
    let mut end = coefficients.len();
    while end > 1 && coefficients[end - 1] == 0.0 {
        end -= 1;
    }
    let coefficients = &coefficients[..end];
    if end < 2 {
        return Vec::new();
    }
    if end == 2 {
        let root = -coefficients[0] / coefficients[1];
        return if root.is_finite() && root >= low && root <= high {
            vec![root]
        } else {
            Vec::new()
        };
    }
    let derivative: Vec<f64> = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(degree, value)| *value * degree as f64)
        .collect();
    let mut points = vec![low];
    points.extend(roots_interval(&derivative, low, high));
    points.push(high);
    points.sort_by(f64::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let tolerance = 1e-12
        * coefficients
            .iter()
            .map(|c| c.abs())
            .sum::<f64>()
            .max(f64::MIN_POSITIVE);
    let mut roots: Vec<f64> = points
        .iter()
        .copied()
        .filter(|point| evaluate(coefficients, *point).abs() <= tolerance)
        .collect();
    for interval in points.windows(2) {
        let (mut a, mut b) = (interval[0], interval[1]);
        let mut fa = evaluate(coefficients, a);
        let fb = evaluate(coefficients, b);
        if !fa.is_finite()
            || !fb.is_finite()
            || fa == 0.0
            || fb == 0.0
            || fa.signum() == fb.signum()
        {
            continue;
        }
        for _ in 0..90 {
            let middle = a + (b - a) * 0.5;
            if middle == a || middle == b {
                break;
            }
            let fm = evaluate(coefficients, middle);
            if fm == 0.0 {
                a = middle;
                b = middle;
                break;
            }
            if fm.signum() == fa.signum() {
                a = middle;
                fa = fm;
            } else {
                b = middle;
            }
        }
        roots.push(a + (b - a) * 0.5);
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    roots
}

/// Constant game gravity, straight constant target velocity, known platform
/// inheritance. All three spatial/velocity coordinates must be observed. This
/// function cannot recover unknown altitude from a 2D screenshot/minimap.
pub fn solve_intercept_3d(
    profile: &WeaponProfile,
    input: &InterceptInput3d,
) -> Result<InterceptSolution3d, String> {
    profile.can_calculate()?;
    profile.require_vacuum()?;
    if input.unit != profile.unit {
        return Err("Jednostki pozycji/ruchu i profilu pocisku są różne".into());
    }
    if input
        .relative
        .iter()
        .chain(input.target_velocity.iter())
        .chain(input.own_velocity.iter())
        .any(|value| !value.is_finite())
        || !input.max_time.is_finite()
        || input.max_time <= 0.0
        || norm(input.relative) <= 1e-9
    {
        return Err("Wymagane pełne 3D pozycji/wysokości i prędkości oraz dodatni horyzont".into());
    }
    let inherit = match input.inherit_own_velocity {
        Some(value) => value,
        None if norm(input.own_velocity) <= 1e-9 => false,
        None => return Err("Brak potwierdzonego modelu dziedziczenia prędkości stanowiska".into()),
    };
    let inherited = input
        .own_velocity
        .map(|value| if inherit { value } else { 0.0 });
    let velocity =
        std::array::from_fn::<_, 3, _>(|index| input.target_velocity[index] - inherited[index]);
    let speed = profile.speed.ok_or("Brak kalibracji prędkości pocisku")?;
    let gravity = profile.gravity.ok_or("Brak kalibracji grawitacji gry")?;
    let raw = [
        dot(input.relative, input.relative),
        2.0 * dot(input.relative, velocity),
        dot(velocity, velocity) + gravity * input.relative[2] - speed * speed,
        gravity * velocity[2],
        0.25 * gravity * gravity,
    ];
    let scaled: Vec<f64> = raw
        .iter()
        .enumerate()
        .map(|(index, coefficient)| coefficient * input.max_time.powi(index as i32))
        .collect();
    if scaled.iter().any(|coefficient| !coefficient.is_finite()) {
        return Err("Horyzont lub parametry 3D poza zakresem numerycznym".into());
    }
    let scale = scaled
        .iter()
        .map(|coefficient| coefficient.abs())
        .max_by(f64::total_cmp)
        .unwrap();
    let coefficients: Vec<f64> = scaled
        .iter()
        .map(|coefficient| coefficient / scale)
        .collect();
    let mut arcs = Vec::new();
    for normalized_time in roots_interval(&coefficients, 0.0, 1.0) {
        let time = normalized_time * input.max_time;
        if time <= 1e-8 || time > input.max_time {
            continue;
        }
        let lead = velocity.map(|value| value * time);
        let mut aim_point =
            std::array::from_fn::<_, 3, _>(|index| input.relative[index] + lead[index]);
        aim_point[2] += 0.5 * gravity * time * time;
        let required = aim_point.map(|value| value / time);
        let required_speed = norm(required);
        if !required_speed.is_finite() || required_speed <= 0.0 {
            continue;
        }
        let launch_velocity = required.map(|value| value * speed / required_speed);
        let elevation_deg = launch_velocity[2]
            .atan2(launch_velocity[0].hypot(launch_velocity[1]))
            .to_degrees();
        if elevation_deg < profile.min_elevation_deg || elevation_deg > profile.max_elevation_deg {
            continue;
        }
        let world_initial_velocity =
            std::array::from_fn::<_, 3, _>(|index| launch_velocity[index] + inherited[index]);
        let target_position_at_impact = std::array::from_fn::<_, 3, _>(|index| {
            input.relative[index] + input.target_velocity[index] * time
        });
        let mut projectile = world_initial_velocity.map(|value| value * time);
        projectile[2] -= 0.5 * gravity * time * time;
        let miss_distance = norm(std::array::from_fn(|index| {
            projectile[index] - target_position_at_impact[index]
        }));
        let tolerance = 1e-7
            * norm(input.relative)
                .max(norm(target_position_at_impact))
                .max(speed * time)
                .max(1.0);
        if !miss_distance.is_finite() || miss_distance > tolerance {
            continue;
        }
        arcs.push(InterceptArc3d {
            flight_time: time,
            elevation_deg,
            azimuth_deg: launch_velocity[0]
                .atan2(launch_velocity[1])
                .to_degrees()
                .rem_euclid(360.0),
            launch_velocity,
            world_initial_velocity,
            target_position_at_impact,
            lead,
            aim_point,
            miss_distance,
        });
    }
    if arcs.is_empty() {
        return Err("Brak przechwycenia 3D w horyzoncie i limitach elewacji; nie zastępujemy go prostym wyprzedzeniem".into());
    }
    let mut assumptions = vec![
        "Model vacuum: stała grawitacja gry, brak oporu i napędu".into(),
        "Cel utrzymuje obserwowaną prędkość 3D przez podany horyzont".into(),
        format!(
            "Dziedziczenie prędkości stanowiska: {inherit}; jednostka: {}",
            input.unit
        ),
        "Wysokość i składowa pionowa ruchu muszą pochodzić z pomiaru, nie z 2D minimapy".into(),
    ];
    if !profile.calibrated {
        assumptions.push(
            "Eksperymentalne źródło community zaakceptowane; brak lokalnej kalibracji".into(),
        );
    }
    Ok(InterceptSolution3d {
        arcs,
        unit: input.unit.clone(),
        assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> WeaponProfile {
        let mut p = mtc_profiles::built_in().remove(6);
        p.speed = Some(100.0);
        p.gravity = Some(10.0);
        p.calibrated = true;
        p.min_elevation_deg = -20.0;
        p.max_elevation_deg = 89.0;
        p
    }
    fn input() -> InterceptInput3d {
        InterceptInput3d {
            relative: [400.0, 100.0, 70.0],
            target_velocity: [5.0, -2.0, 3.0],
            own_velocity: [10.0, 1.0, -1.0],
            inherit_own_velocity: Some(true),
            unit: "game_units".into(),
            max_time: 30.0,
        }
    }
    #[test]
    fn reconstructs_moving_air_target_and_platform_with_gravity_for_every_arc() {
        let p = profile();
        let i = input();
        let solution = solve_intercept_3d(&p, &i).unwrap();
        assert_eq!(solution.arcs.len(), 2);
        for arc in solution.arcs {
            assert!((norm(arc.launch_velocity) - 100.0).abs() < 1e-8);
            let t = arc.flight_time;
            let mut shot = arc.world_initial_velocity.map(|value| value * t);
            shot[2] -= 5.0 * t * t;
            for (index, position) in shot.iter().enumerate() {
                assert!(
                    (*position - (i.relative[index] + i.target_velocity[index] * t)).abs() < 1e-5
                );
            }
            assert!(arc.miss_distance < 1e-5);
        }
    }
    #[test]
    fn static_target_matches_analytic_arcs_and_finds_tangent_maximum_range() {
        let p = profile();
        let mut i = input();
        i.relative = [500.0, 0.0, 20.0];
        i.target_velocity = [0.0; 3];
        i.own_velocity = [0.0; 3];
        let analytic = crate::solve(&p, 500.0, 20.0).unwrap();
        let actual = solve_intercept_3d(&p, &i).unwrap();
        assert_eq!(actual.arcs.len(), 2);
        for (expected, actual) in analytic.arcs.iter().zip(actual.arcs) {
            assert!((expected.elevation_deg - actual.elevation_deg).abs() < 1e-7);
            assert!((expected.flight_time - actual.flight_time).abs() < 1e-7);
        }
        i.relative = [1000.0, 0.0, 0.0];
        let tangent = solve_intercept_3d(&p, &i).unwrap();
        assert_eq!(tangent.arcs.len(), 1);
        assert!((tangent.arcs[0].elevation_deg - 45.0).abs() < 1e-5);
    }
    #[test]
    fn blocks_missing_height_unknown_platform_model_bad_units_and_horizon() {
        let p = profile();
        let mut i = input();
        i.relative[2] = f64::NAN;
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        i.inherit_own_velocity = None;
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        i.unit = "game_meters".into();
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        i.max_time = 0.1;
        assert!(solve_intercept_3d(&p, &i).is_err());
    }
    #[test]
    fn rejects_escaping_target_guidance_and_honors_elevation_limits() {
        let mut p = profile();
        let mut i = input();
        i.target_velocity = [200.0, 0.0, 0.0];
        i.own_velocity = [0.0; 3];
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        p.max_elevation_deg = 0.0;
        assert!(solve_intercept_3d(&p, &i).is_err());
        let guided = mtc_profiles::built_in().remove(5);
        assert!(solve_intercept_3d(&guided, &input()).is_err());
    }
    #[test]
    fn inheritance_false_does_not_subtract_platform_motion() {
        let p = profile();
        let mut i = input();
        i.inherit_own_velocity = Some(false);
        let with_moving_platform = solve_intercept_3d(&p, &i).unwrap();
        i.own_velocity = [0.0; 3];
        let stationary = solve_intercept_3d(&p, &i).unwrap();
        assert!(
            (with_moving_platform.arcs[0].flight_time - stationary.arcs[0].flight_time).abs()
                < 1e-8
        );
    }
    #[test]
    fn roots_outside_horizon_and_numeric_overflow_are_rejected() {
        let p = profile();
        let mut i = input();
        i.max_time = f64::MAX;
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        i.relative[0] = f64::MAX;
        assert!(solve_intercept_3d(&p, &i).is_err());
        i = input();
        i.relative = [1000.01, 0.0, 0.0];
        i.target_velocity = [0.0; 3];
        i.own_velocity = [0.0; 3];
        assert!(solve_intercept_3d(&p, &i).is_err());
    }
}
