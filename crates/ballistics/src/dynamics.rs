//! Bounded numerical trajectories for explicit, locally calibrated game models.
use mtc_profiles::{DynamicsModel, WeaponProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationOptions {
    pub step_seconds: f64,
    pub max_steps: u32,
}
impl Default for SimulationOptions {
    fn default() -> Self {
        Self {
            step_seconds: 0.02,
            max_steps: 50_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationInput {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    pub unit: String,
    pub duration: f64,
    #[serde(default)]
    pub options: SimulationOptions,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryPoint {
    pub time: f64,
    /// East, north, up, relative to a stationary muzzle at time zero.
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrajectorySimulation {
    pub points: Vec<TrajectoryPoint>,
    pub unit: String,
    pub steps_used: u32,
    pub assumptions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicSolveOptions {
    pub max_time: f64,
    pub step_seconds: f64,
    /// One shared budget for angle probes, root refinement and RK4 event lookup.
    pub max_work_steps: u32,
    pub max_evaluations: u32,
    pub angle_samples: u32,
    /// Numerical positional tolerance in the selected game unit; not game accuracy.
    pub hit_tolerance: f64,
}
impl Default for DynamicSolveOptions {
    fn default() -> Self {
        Self {
            max_time: 60.0,
            step_seconds: 0.02,
            max_work_steps: 1_000_000,
            max_evaluations: 512,
            angle_samples: 65,
            hit_tolerance: 0.05,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicSolveInput {
    pub distance: f64,
    pub height_delta: f64,
    pub azimuth_deg: f64,
    pub unit: String,
    #[serde(default)]
    pub options: DynamicSolveOptions,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicArc {
    pub elevation_deg: f64,
    pub azimuth_deg: f64,
    pub flight_time: f64,
    pub impact_position: [f64; 3],
    pub miss_distance: f64,
    /// Higher of at least two returned roots, not an inferred parabola class.
    pub high_arc: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicSolution {
    pub arcs: Vec<DynamicArc>,
    pub unit: String,
    pub minimum_residual_units: f64,
    pub steps_used: u32,
    pub evaluations: u32,
    pub assumptions: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct State {
    pub(crate) position: [f64; 3],
    pub(crate) velocity: [f64; 3],
}
pub(crate) fn norm(vector: [f64; 3]) -> f64 {
    vector[0].hypot(vector[1]).hypot(vector[2])
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Clone, Copy)]
pub(crate) struct Model {
    pub(crate) speed: f64,
    pub(crate) gravity: f64,
    linear: f64,
    quadratic: f64,
    acceleration: f64,
    burn_time: f64,
}
impl Model {
    pub(crate) fn from_profile(profile: &WeaponProfile, unit: &str) -> Result<Self, String> {
        profile.can_calculate()?;
        if unit != profile.unit {
            return Err("Jednostka trajektorii musi odpowiadać jednostce profilu gry".into());
        }
        let mut model = Self {
            speed: profile.speed.ok_or("Brak prędkości początkowej")?,
            gravity: profile.gravity.ok_or("Brak grawitacji gry")?,
            linear: 0.0,
            quadratic: 0.0,
            acceleration: 0.0,
            burn_time: 0.0,
        };
        if profile.dynamics_model() != DynamicsModel::Vacuum {
            let d = profile
                .dynamics
                .as_ref()
                .ok_or("Brak parametrów dynamics")?;
            model.linear = d.linear_drag.ok_or("Brak linearDrag")?;
            model.quadratic = d.quadratic_drag.ok_or("Brak quadraticDrag")?;
            if d.model == DynamicsModel::Rocket {
                model.acceleration = d.rocket_acceleration.ok_or("Brak rocketAcceleration")?;
                model.burn_time = d.burn_time.ok_or("Brak burnTime")?;
            }
        }
        if model.speed > 1e9 || model.gravity > 1e12 || model.acceleration > 1e12 {
            return Err("Parametry przekraczają numeryczny limit prędkości lub przyspieszenia; brak wiarygodnej symulacji".into());
        }
        Ok(model)
    }

    fn acceleration(
        self,
        velocity: [f64; 3],
        launch: [f64; 3],
        burning: bool,
    ) -> Result<[f64; 3], String> {
        let speed = norm(velocity);
        let drag = self.linear + self.quadratic * speed;
        let thrust = if burning { self.acceleration } else { 0.0 };
        let mut acceleration = std::array::from_fn(|i| -drag * velocity[i] + thrust * launch[i]);
        acceleration[2] -= self.gravity;
        if !drag.is_finite() || !norm(acceleration).is_finite() || norm(acceleration) > 1e12 {
            return Err("Przepełnienie lub zbyt duże przyspieszenie w modelu dynamics".into());
        }
        Ok(acceleration)
    }

    fn derivative(self, state: State, launch: [f64; 3], burning: bool) -> Result<State, String> {
        check_state(state)?;
        Ok(State {
            position: state.velocity,
            velocity: self.acceleration(state.velocity, launch, burning)?,
        })
    }

    pub(crate) fn step(
        self,
        state: State,
        launch: [f64; 3],
        h: f64,
        burning: bool,
        budget: &mut Budget,
    ) -> Result<State, String> {
        budget.take()?;
        let k1 = self.derivative(state, launch, burning)?;
        let k2 = self.derivative(add_scaled(state, k1, h * 0.5), launch, burning)?;
        let k3 = self.derivative(add_scaled(state, k2, h * 0.5), launch, burning)?;
        let k4 = self.derivative(add_scaled(state, k3, h), launch, burning)?;
        let next = State {
            position: std::array::from_fn(|i| {
                state.position[i]
                    + h / 6.0
                        * (k1.position[i]
                            + 2.0 * k2.position[i]
                            + 2.0 * k3.position[i]
                            + k4.position[i])
            }),
            velocity: std::array::from_fn(|i| {
                state.velocity[i]
                    + h / 6.0
                        * (k1.velocity[i]
                            + 2.0 * k2.velocity[i]
                            + 2.0 * k3.velocity[i]
                            + k4.velocity[i])
            }),
        };
        check_state(next)?;
        Ok(next)
    }

    /// Limit drag stiffness and velocity changes; split exactly at burnout.
    /// Coefficients are never clamped or replaced by guessed values.
    ///
    /// The returned `next_time` is snapped exactly onto burnout or the end of
    /// the horizon. Accumulating `time += h` could otherwise stop one ulp short
    /// of the boundary and demand a sub-1e-10 s step (spurious error).
    pub(crate) fn step_size(
        self,
        state: State,
        launch: [f64; 3],
        time: f64,
        end: f64,
        maximum: f64,
    ) -> Result<StepPlan, String> {
        let burning = time < self.burn_time;
        let boundary = if burning {
            end.min(self.burn_time)
        } else {
            end
        };
        let remaining = boundary - time;
        let mut h = maximum.min(remaining);
        let speed = norm(state.velocity);
        let rate = self.linear + 2.0 * self.quadratic * speed;
        if !rate.is_finite() {
            return Err("Zbyt sztywny model oporu dla ograniczonej integracji".into());
        }
        if rate > 0.0 {
            h = h.min(0.25 / rate);
        }
        let acceleration = norm(self.acceleration(state.velocity, launch, burning)?);
        if acceleration > 0.0 {
            h = h.min(0.25 * speed.max(1.0) / acceleration);
        }
        // Absorb a negligible remainder instead of leaving a sliver step.
        let snap = remaining - h <= 1e-9 * boundary.abs().max(1.0);
        if snap {
            h = remaining;
        }
        if h < 1e-10 || !h.is_finite() || time + h == time {
            return Err(
                "Wymagany krok dynamics jest zbyt mały; brak wiarygodnego wyniku w limicie pracy"
                    .into(),
            );
        }
        Ok(StepPlan {
            h,
            burning,
            next_time: if snap { boundary } else { time + h },
        })
    }

    /// Integrate exactly to `duration` from the muzzle with an explicit initial
    /// world velocity (launch speed plus any declared platform inheritance).
    pub(crate) fn propagate(
        self,
        launch: [f64; 3],
        initial_velocity: [f64; 3],
        duration: f64,
        step_seconds: f64,
        budget: &mut Budget,
    ) -> Result<State, String> {
        let mut state = State {
            position: [0.0; 3],
            velocity: initial_velocity,
        };
        let mut time = 0.0;
        while duration - time > duration.max(1.0) * 1e-12 {
            let plan = self.step_size(state, launch, time, duration, step_seconds)?;
            state = self.step(state, launch, plan.h, plan.burning, budget)?;
            time = plan.next_time;
        }
        Ok(state)
    }

    /// All crossings of the horizontal plane `up = height` within `horizon`,
    /// each located with RK4 substeps (never by linear interpolation).
    pub(crate) fn plane_crossings(
        self,
        launch: [f64; 3],
        initial_velocity: [f64; 3],
        height: f64,
        horizon: f64,
        step_seconds: f64,
        budget: &mut Budget,
    ) -> Result<Vec<(f64, State)>, String> {
        let mut state = State {
            position: [0.0; 3],
            velocity: initial_velocity,
        };
        let mut time = 0.0;
        let mut crossings = Vec::new();
        while horizon - time > horizon.max(1.0) * 1e-12 {
            let plan = self.step_size(state, launch, time, horizon, step_seconds)?;
            let next = self.step(state, launch, plan.h, plan.burning, budget)?;
            let before = state.position[2] - height;
            let after = next.position[2] - height;
            if after == 0.0 || (before != 0.0 && before.signum() != after.signum()) {
                let (mut low, mut high) = (0.0, plan.h);
                let mut event = next;
                let mut event_h = plan.h;
                for _ in 0..60 {
                    event_h = (low + high) * 0.5;
                    event = self.step(state, launch, event_h, plan.burning, budget)?;
                    let value = event.position[2] - height;
                    if value == 0.0 || high - low < 1e-13 * plan.h {
                        break;
                    }
                    if value.signum() == before.signum() {
                        low = event_h;
                    } else {
                        high = event_h;
                    }
                }
                crossings.push((time + event_h, event));
            }
            state = next;
            time = plan.next_time;
        }
        Ok(crossings)
    }

    /// State when the projectile reaches `range` along the north (forward)
    /// axis, located with RK4 substeps; `None` if not reached within `horizon`.
    pub(crate) fn state_at_forward_range(
        self,
        launch: [f64; 3],
        initial_velocity: [f64; 3],
        range: f64,
        horizon: f64,
        step_seconds: f64,
        budget: &mut Budget,
    ) -> Result<Option<(f64, State)>, String> {
        let mut state = State {
            position: [0.0; 3],
            velocity: initial_velocity,
        };
        let mut time = 0.0;
        while horizon - time > horizon.max(1.0) * 1e-12 {
            let plan = self.step_size(state, launch, time, horizon, step_seconds)?;
            let next = self.step(state, launch, plan.h, plan.burning, budget)?;
            if next.position[1] >= range {
                let (mut low, mut high) = (0.0, plan.h);
                let mut event = next;
                let mut event_h = plan.h;
                for _ in 0..60 {
                    event_h = (low + high) * 0.5;
                    event = self.step(state, launch, event_h, plan.burning, budget)?;
                    let gap = event.position[1] - range;
                    if gap.abs() <= 1e-11 * range.max(1.0) || high - low < 1e-13 * plan.h {
                        break;
                    }
                    if gap < 0.0 {
                        low = event_h;
                    } else {
                        high = event_h;
                    }
                }
                return Ok(Some((time + event_h, event)));
            }
            if next.velocity[1] <= 0.0 {
                return Ok(None);
            }
            state = next;
            time = plan.next_time;
        }
        Ok(None)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StepPlan {
    pub(crate) h: f64,
    pub(crate) burning: bool,
    pub(crate) next_time: f64,
}

fn check_state(state: State) -> Result<(), String> {
    if !norm(state.position).is_finite()
        || !norm(state.velocity).is_finite()
        || norm(state.position) > 1e12
        || norm(state.velocity) > 1e9
    {
        return Err("Trajektoria przekroczyła numeryczne granice położenia lub prędkości".into());
    }
    Ok(())
}
fn add_scaled(state: State, slope: State, h: f64) -> State {
    State {
        position: std::array::from_fn(|i| state.position[i] + h * slope.position[i]),
        velocity: std::array::from_fn(|i| state.velocity[i] + h * slope.velocity[i]),
    }
}
pub(crate) struct Budget {
    pub(crate) used: u32,
    pub(crate) maximum: u32,
}
impl Budget {
    fn take(&mut self) -> Result<(), String> {
        if self.used >= self.maximum {
            return Err("Przekroczony łączny limit kroków RK4; brak częściowego wyniku".into());
        }
        self.used += 1;
        Ok(())
    }
}
pub(crate) fn validate_time_and_step(time: f64, step: f64) -> Result<(), String> {
    if !time.is_finite()
        || time < 1e-6
        || time > 600.0
        || !step.is_finite()
        || !(1e-5..=0.1).contains(&step)
    {
        return Err("Wymagane 0 < czas ≤ 600 s oraz krok RK4 od 0.00001 do 0.1 s".into());
    }
    Ok(())
}
pub(crate) fn launch_direction(elevation: f64, azimuth: f64) -> [f64; 3] {
    let (elevation, azimuth) = (
        elevation.to_radians(),
        azimuth.rem_euclid(360.0).to_radians(),
    );
    [
        elevation.cos() * azimuth.sin(),
        elevation.cos() * azimuth.cos(),
        elevation.sin(),
    ]
}
pub(crate) fn assumptions(profile: &WeaponProfile) -> Vec<String> {
    let mut result = vec![
        "Model fenomenologiczny gry, nie potwierdzenie mechaniki obecnego MTC".into(),
        "Stała grawitacja; nieruchome stanowisko; brak wiatru, kierowania i kolizji z terenem".into(),
        "Osie east/north/up; początek w stanowisku; opór przeciwny do prędkości względem nieruchomego ośrodka".into(),
        "RK4 z ograniczeniem kroku i pracy; zbieżność numeryczna nie dowodzi zgodności modelu z grą".into(),
        format!("Jednostka: {}; linearDrag 1/s, quadraticDrag 1/{}, rocketAcceleration {}/s², burnTime s", profile.unit, profile.unit, profile.unit),
    ];
    if let Some(d) = &profile.dynamics {
        result.push(format!(
            "Zadeklarowany model {:?}; lokalny status {}; źródło: {}",
            d.model, d.status, d.source
        ));
        if d.model == DynamicsModel::Rocket {
            result.push("Jawne założenie thrustDirectionFixedLaunch=true: napęd utrzymuje kierunek początkowy do burnTime; nie podąża za wektorem prędkości ani celem".into());
        }
    } else {
        result.push("Profil bez dynamics: wyłącznie dotychczasowy model vacuum".into());
    }
    result
}

pub fn simulate(
    profile: &WeaponProfile,
    input: &SimulationInput,
) -> Result<TrajectorySimulation, String> {
    let model = Model::from_profile(profile, &input.unit)?;
    validate_time_and_step(input.duration, input.options.step_seconds)?;
    if !input.elevation_deg.is_finite()
        || input.elevation_deg < profile.min_elevation_deg
        || input.elevation_deg > profile.max_elevation_deg
        || !input.azimuth_deg.is_finite()
        || input.azimuth_deg.abs() > 1e6
        || !(1..=100_000).contains(&input.options.max_steps)
    {
        return Err("Niepoprawny kierunek startu lub limit kroków symulacji".into());
    }
    let launch = launch_direction(input.elevation_deg, input.azimuth_deg);
    let mut state = State {
        position: [0.0; 3],
        velocity: launch.map(|v| v * model.speed),
    };
    let mut time = 0.0;
    let mut budget = Budget {
        used: 0,
        maximum: input.options.max_steps,
    };
    let mut points = vec![TrajectoryPoint {
        time,
        position: state.position,
        velocity: state.velocity,
    }];
    while input.duration - time > input.duration.max(1.0) * 1e-12 {
        let plan = model.step_size(
            state,
            launch,
            time,
            input.duration,
            input.options.step_seconds,
        )?;
        state = model.step(state, launch, plan.h, plan.burning, &mut budget)?;
        time = plan.next_time;
        points.push(TrajectoryPoint {
            time,
            position: state.position,
            velocity: state.velocity,
        });
    }
    Ok(TrajectorySimulation {
        points,
        unit: input.unit.clone(),
        steps_used: budget.used,
        assumptions: assumptions(profile),
    })
}

#[derive(Clone, Copy)]
struct Shot {
    elevation: f64,
    time: f64,
    position: [f64; 3],
    residual: f64,
}
struct Search<'a> {
    model: Model,
    input: &'a DynamicSolveInput,
    budget: Budget,
    evaluations: u32,
    minimum: f64,
}
impl Search<'_> {
    fn evaluate(&mut self, elevation: f64) -> Result<Option<Shot>, String> {
        if self.evaluations >= self.input.options.max_evaluations {
            return Err("Przekroczony limit prób kąta; brak częściowego rozwiązania".into());
        }
        self.evaluations += 1;
        let launch = launch_direction(elevation, self.input.azimuth_deg);
        let azimuth = self.input.azimuth_deg.rem_euclid(360.0).to_radians();
        let horizontal = [azimuth.sin(), azimuth.cos(), 0.0];
        let mut state = State {
            position: [0.0; 3],
            velocity: launch.map(|v| v * self.model.speed),
        };
        let mut time = 0.0;
        let options = &self.input.options;
        while options.max_time - time > options.max_time.max(1.0) * 1e-12 {
            let plan = self.model.step_size(
                state,
                launch,
                time,
                options.max_time,
                options.step_seconds,
            )?;
            let (h, burning) = (plan.h, plan.burning);
            let next = self
                .model
                .step(state, launch, h, burning, &mut self.budget)?;
            if dot(next.position, horizontal) >= self.input.distance {
                // Locate the target-range event with RK4 substeps, not a linear
                // interpolation that can fabricate a matching height/time.
                let (mut low, mut high) = (0.0, h);
                let mut event = next;
                let mut event_h = h;
                for _ in 0..40 {
                    event_h = (low + high) * 0.5;
                    event = self
                        .model
                        .step(state, launch, event_h, burning, &mut self.budget)?;
                    let range = dot(event.position, horizontal);
                    if (range - self.input.distance).abs() < options.hit_tolerance * 0.01 {
                        break;
                    }
                    if range < self.input.distance {
                        low = event_h;
                    } else {
                        high = event_h;
                    }
                }
                let residual = event.position[2] - self.input.height_delta;
                self.minimum = self.minimum.min(residual.abs());
                return Ok(Some(Shot {
                    elevation,
                    time: time + event_h,
                    position: event.position,
                    residual,
                }));
            }
            if dot(next.velocity, horizontal) < 0.0 {
                return Err("Integracja oporu utraciła monotoniczny ruch poziomy; brak wiarygodnego rozwiązania".into());
            }
            state = next;
            time = plan.next_time;
        }
        Ok(None)
    }
}

/// Static target only. Scan elevation bounds, refine sign-changing roots and
/// local minima of the height residual (including a tangent/max-range root).
/// A finite scan cannot guarantee all roots for arbitrary empirical models.
pub fn solve_dynamic(
    profile: &WeaponProfile,
    input: &DynamicSolveInput,
) -> Result<DynamicSolution, String> {
    let model = Model::from_profile(profile, &input.unit)?;
    let options = &input.options;
    validate_time_and_step(options.max_time, options.step_seconds)?;
    if !input.distance.is_finite()
        || input.distance <= 0.0
        || input.distance > 1e12
        || !input.height_delta.is_finite()
        || input.height_delta.abs() > 1e12
        || !input.azimuth_deg.is_finite()
        || input.azimuth_deg.abs() > 1e6
        || !(1..=5_000_000).contains(&options.max_work_steps)
        || !(3..=1024).contains(&options.max_evaluations)
        || !(3..=181).contains(&options.angle_samples)
        || options.angle_samples > options.max_evaluations
        || !options.hit_tolerance.is_finite()
        || options.hit_tolerance <= 0.0
        || options.hit_tolerance > 1.0
        || options.hit_tolerance >= input.distance * 0.01
    {
        return Err(
            "Niepoprawny cel, tolerancja trafienia lub limity wyszukiwania dynamics".into(),
        );
    }
    let mut search = Search {
        model,
        input,
        budget: Budget {
            used: 0,
            maximum: options.max_work_steps,
        },
        evaluations: 0,
        minimum: f64::INFINITY,
    };
    let mut samples = Vec::new();
    let mut candidates = Vec::new();
    let sample_count = if profile.min_elevation_deg == profile.max_elevation_deg {
        1
    } else {
        options.angle_samples
    };
    for index in 0..sample_count {
        let angle = profile.min_elevation_deg
            + (profile.max_elevation_deg - profile.min_elevation_deg) * f64::from(index)
                / f64::from(sample_count.saturating_sub(1).max(1));
        let shot = search.evaluate(angle)?;
        if let Some(shot) = shot.filter(|s| s.residual.abs() < options.hit_tolerance * 0.1) {
            candidates.push(shot);
        }
        samples.push(shot);
    }
    for pair in samples.windows(2) {
        let (Some(mut low), Some(mut high)) = (pair[0], pair[1]) else {
            continue;
        };
        if low.residual.signum() == high.residual.signum() {
            continue;
        }
        for _ in 0..48 {
            let Some(mid) = search.evaluate((low.elevation + high.elevation) * 0.5)? else {
                break;
            };
            if mid.residual.abs() < options.hit_tolerance * 0.02 {
                candidates.push(mid);
                break;
            }
            if mid.residual.signum() == low.residual.signum() {
                low = mid;
            } else {
                high = mid;
            }
        }
    }
    // Refine sampled local minima of |height residual|. This is essential when
    // a maximum-range trajectory touches the target and never changes sign.
    for triple in samples.windows(3) {
        let (Some(left), Some(middle), Some(right)) = (triple[0], triple[1], triple[2]) else {
            continue;
        };
        if middle.residual.abs() > left.residual.abs()
            || middle.residual.abs() > right.residual.abs()
        {
            continue;
        }
        let (mut a, mut b) = (left.elevation, right.elevation);
        let ratio = (5.0_f64.sqrt() - 1.0) * 0.5;
        let (mut c, mut d) = (b - ratio * (b - a), a + ratio * (b - a));
        let (Some(mut fc), Some(mut fd)) = (search.evaluate(c)?, search.evaluate(d)?) else {
            continue;
        };
        for _ in 0..48 {
            if (b - a).abs() < 1e-8 {
                break;
            }
            if fc.residual.abs() < fd.residual.abs() {
                b = d;
                d = c;
                fd = fc;
                c = b - ratio * (b - a);
                let Some(next) = search.evaluate(c)? else {
                    break;
                };
                fc = next;
            } else {
                a = c;
                c = d;
                fc = fd;
                d = a + ratio * (b - a);
                let Some(next) = search.evaluate(d)? else {
                    break;
                };
                fd = next;
            }
        }
        let best = if fc.residual.abs() < fd.residual.abs() {
            fc
        } else {
            fd
        };
        if best.residual.abs() < options.hit_tolerance * 0.1 {
            candidates.push(best);
        }
    }
    let azimuth = input.azimuth_deg.rem_euclid(360.0);
    let target = [
        input.distance * azimuth.to_radians().sin(),
        input.distance * azimuth.to_radians().cos(),
        input.height_delta,
    ];
    let mut arcs: Vec<DynamicArc> = Vec::new();
    candidates.sort_by(|a, b| a.elevation.total_cmp(&b.elevation));
    let angular_tolerance = (options.hit_tolerance / input.distance).atan().to_degrees() * 2.0;
    for shot in candidates {
        let miss = norm(std::array::from_fn(|i| shot.position[i] - target[i]));
        if miss > options.hit_tolerance {
            continue;
        }
        let arc = DynamicArc {
            elevation_deg: shot.elevation,
            azimuth_deg: azimuth,
            flight_time: shot.time,
            impact_position: shot.position,
            miss_distance: miss,
            high_arc: false,
        };
        if let Some(existing) = arcs
            .iter_mut()
            .find(|a| (a.elevation_deg - arc.elevation_deg).abs() <= angular_tolerance)
        {
            if arc.miss_distance < existing.miss_distance {
                *existing = arc;
            }
        } else {
            arcs.push(arc);
        }
    }
    if arcs.is_empty() {
        return Err(if search.minimum.is_finite() {
            format!("Brak rozwiązania dynamics w limitach; minimum |residual wysokości| {:.6} {}; nie jest to trafienie", search.minimum, input.unit)
        } else {
            "Cel nieosiągnięty w limicie czasu dynamics; minimum residual niedostępne".into()
        });
    }
    arcs.sort_by(|a, b| a.elevation_deg.total_cmp(&b.elevation_deg));
    if arcs.len() > 1 {
        if let Some(last) = arcs.last_mut() {
            last.high_arc = true;
        }
    }
    let mut assumptions = assumptions(profile);
    assumptions.push(format!("Cel statyczny; skan {} kątów, limity {} s / {} kroków / {} prób; tolerancja numeryczna {} {}", sample_count, options.max_time, options.max_work_steps, options.max_evaluations, options.hit_tolerance, input.unit));
    assumptions.push("Skan i lokalne minima nie gwarantują wszystkich korzeni dowolnego modelu; brak rozwiązania nie dowodzi braku trajektorii w grze".into());
    assumptions.push("highArc oznacza wyższy z co najmniej dwóch znalezionych korzeni; wynik nie zawiera wyprzedzenia celu ani platformy".into());
    Ok(DynamicSolution {
        arcs,
        unit: input.unit.clone(),
        minimum_residual_units: search.minimum,
        steps_used: search.budget.used,
        evaluations: search.evaluations,
        assumptions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mtc_profiles::{Category, ProjectileDynamics};

    fn profile() -> WeaponProfile {
        let mut p = mtc_profiles::built_in()
            .into_iter()
            .find(|p| p.category == Category::TubeArtillery)
            .unwrap();
        p.speed = Some(100.0);
        p.gravity = Some(10.0);
        p.min_elevation_deg = -10.0;
        p.max_elevation_deg = 89.0;
        p.status = "beta".into();
        p.calibrated = true;
        p.source = "Syntetyczny test; brak pomiarów rzeczywistej gry".into();
        p
    }
    fn with_drag(linear: f64, quadratic: f64) -> WeaponProfile {
        let mut p = profile();
        p.dynamics = Some(ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: p.unit.clone(),
            linear_drag: Some(linear),
            quadratic_drag: Some(quadratic),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test współczynników; nie MTC".into(),
        });
        p
    }
    fn with_rocket(acceleration: f64, burn: f64) -> WeaponProfile {
        let mut p = with_drag(0.0, 0.0);
        p.category = Category::RocketArtillery;
        let d = p.dynamics.as_mut().unwrap();
        d.model = DynamicsModel::Rocket;
        d.rocket_acceleration = Some(acceleration);
        d.burn_time = Some(burn);
        d.thrust_direction_fixed_launch = Some(true);
        p
    }
    fn simulation(elevation: f64, azimuth: f64, duration: f64) -> SimulationInput {
        SimulationInput {
            elevation_deg: elevation,
            azimuth_deg: azimuth,
            unit: "game_units".into(),
            duration,
            options: SimulationOptions::default(),
        }
    }
    fn target(distance: f64, height: f64) -> DynamicSolveInput {
        DynamicSolveInput {
            distance,
            height_delta: height,
            azimuth_deg: 37.0,
            unit: "game_units".into(),
            options: DynamicSolveOptions::default(),
        }
    }
    fn near(a: f64, b: f64, tolerance: f64) {
        assert!((a - b).abs() <= tolerance, "{a} != {b} within {tolerance}");
    }
    fn last(profile: &WeaponProfile, input: &SimulationInput) -> TrajectoryPoint {
        *simulate(profile, input).unwrap().points.last().unwrap()
    }
    fn check_arcs(p: &WeaponProfile, input: &DynamicSolveInput, solution: &DynamicSolution) {
        let a = input.azimuth_deg.to_radians();
        let target = [
            a.sin() * input.distance,
            a.cos() * input.distance,
            input.height_delta,
        ];
        assert!(!solution.arcs.is_empty());
        for arc in &solution.arcs {
            assert!(arc.flight_time <= input.options.max_time);
            assert!(arc.miss_distance <= input.options.hit_tolerance);
            let trajectory = last(
                p,
                &simulation(arc.elevation_deg, input.azimuth_deg, arc.flight_time),
            );
            let error = norm(std::array::from_fn(|i| trajectory.position[i] - target[i]));
            assert!(
                error < input.options.hit_tolerance * 1.2,
                "Reconstructed dynamic miss {error}"
            );
        }
        assert!(solution.steps_used <= input.options.max_work_steps);
        assert!(solution.evaluations <= input.options.max_evaluations);
    }

    #[test]
    fn rk4_vacuum_and_explicit_zero_drag_reconstruct_the_analytic_3d_trajectory() {
        let input = simulation(31.0, 73.0, 5.0);
        let direction = launch_direction(input.elevation_deg, input.azimuth_deg);
        for p in [profile(), with_drag(0.0, 0.0)] {
            let trajectory = simulate(&p, &input).unwrap();
            assert!(trajectory.points.len() > 2);
            for point in trajectory.points {
                let t = point.time;
                for (axis, direction) in direction.iter().enumerate() {
                    let gravity = if axis == 2 { 10.0 } else { 0.0 };
                    near(
                        point.position[axis],
                        direction * 100.0 * t - 0.5 * gravity * t * t,
                        1e-8,
                    );
                    near(point.velocity[axis], direction * 100.0 - gravity * t, 1e-8);
                }
            }
        }
    }

    #[test]
    fn linear_drag_matches_the_closed_form_game_trajectory() {
        let input = simulation(31.0, 73.0, 5.0);
        let direction = launch_direction(input.elevation_deg, input.azimuth_deg);
        let p = with_drag(0.13, 0.0);
        let result = last(&p, &input);
        let k = 0.13;
        let decay = (-k * input.duration).exp();
        for (axis, direction) in direction.iter().enumerate() {
            let offset = if axis == 2 { 10.0 / k } else { 0.0 };
            near(
                result.velocity[axis],
                (direction * 100.0 + offset) * decay - offset,
                1e-8,
            );
            near(
                result.position[axis],
                (direction * 100.0 + offset) * (1.0 - decay) / k - offset * input.duration,
                1e-7,
            );
        }
    }

    #[test]
    fn drag_decreases_progress_and_quadratic_drag_dissipates_energy() {
        let input = simulation(31.0, 0.0, 5.0);
        let vacuum = last(&profile(), &input);
        for p in [with_drag(0.1, 0.0), with_drag(0.0, 0.002)] {
            let trajectory = simulate(&p, &input).unwrap();
            let final_point = trajectory.points.last().unwrap();
            assert!(final_point.position[1] < vacuum.position[1] - 20.0);
            let energy = |point: &TrajectoryPoint| {
                0.5 * norm(point.velocity).powi(2) + 10.0 * point.position[2]
            };
            assert!(trajectory
                .points
                .windows(2)
                .all(|pair| energy(&pair[1]) <= energy(&pair[0]) + 1e-7));
        }
    }

    #[test]
    fn declared_thrust_reconstructs_exact_burnout_between_regular_steps() {
        let p = with_rocket(20.0, 1.27);
        let mut input = simulation(31.0, 73.0, 5.0);
        input.options.step_seconds = 0.03;
        let result = simulate(&p, &input).unwrap();
        assert!(result
            .points
            .iter()
            .any(|point| (point.time - 1.27).abs() < 1e-12));
        let point = result.points.last().unwrap();
        let direction = launch_direction(input.elevation_deg, input.azimuth_deg);
        let impulse = 20.0 * 1.27;
        let extra_distance = 20.0 * (1.27 * 5.0 - 0.5 * 1.27 * 1.27);
        for (axis, direction) in direction.iter().enumerate() {
            let gravity = if axis == 2 { 10.0 } else { 0.0 };
            near(
                point.velocity[axis],
                direction * (100.0 + impulse) - gravity * 5.0,
                1e-8,
            );
            near(
                point.position[axis],
                direction * (100.0 * 5.0 + extra_distance) - gravity * 12.5,
                1e-8,
            );
        }
        assert!(result
            .assumptions
            .iter()
            .any(|a| a.contains("thrustDirectionFixedLaunch=true")));
    }

    #[test]
    fn burnout_and_horizon_rounding_never_demand_a_sliver_step() {
        // Accumulated `time += h` used to stop one ulp before burnout/horizon
        // and then fail with "krok zbyt mały" for ordinary inputs.
        let mut failures = Vec::new();
        for burn in [0.1, 0.3, 0.7, 1.1, 1.3, 2.9, 3.3, 7.7] {
            for step in [0.003, 0.007, 0.01, 0.013, 0.02, 0.03, 0.07] {
                let p = with_rocket(15.0, burn);
                let mut input = simulation(40.0, 10.0, 12.3);
                input.options.step_seconds = step;
                if let Err(error) = simulate(&p, &input) {
                    failures.push(format!("burn {burn} step {step}: {error}"));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn numeric_zero_drag_search_agrees_with_both_vacuum_roots() {
        let p = with_drag(0.0, 0.0);
        let mut input = target(500.0, 20.0);
        input.options.hit_tolerance = 1e-5;
        let result = solve_dynamic(&p, &input).unwrap();
        let analytic = crate::solve(&profile(), input.distance, input.height_delta).unwrap();
        assert_eq!(result.arcs.len(), 2);
        for (numeric, analytic) in result.arcs.iter().zip(analytic.arcs) {
            near(numeric.elevation_deg, analytic.elevation_deg, 1e-4);
            near(numeric.flight_time, analytic.flight_time, 1e-5);
        }
        check_arcs(&p, &input, &result);
    }

    #[test]
    fn shooting_refines_tangent_maximum_range_without_sign_change() {
        let p = with_drag(0.0, 0.0);
        let input = target(1000.0, 0.0);
        let result = solve_dynamic(&p, &input).unwrap();
        assert_eq!(result.arcs.len(), 1);
        near(result.arcs[0].elevation_deg, 45.0, 0.1);
        assert!(result.minimum_residual_units < input.options.hit_tolerance);
        check_arcs(&p, &input, &result);
    }

    #[test]
    fn static_drag_solution_is_reconstructed_with_actual_drag_not_vacuum_substitution() {
        let p = with_drag(0.06, 0.0002);
        let input = target(300.0, 30.0);
        let result = solve_dynamic(&p, &input).unwrap();
        let vacuum = crate::solve(&profile(), input.distance, input.height_delta).unwrap();
        assert!((result.arcs[0].elevation_deg - vacuum.arcs[0].elevation_deg).abs() > 0.5);
        check_arcs(&p, &input, &result);
    }

    #[test]
    fn measured_thrust_assumption_enables_a_target_outside_the_zero_thrust_model() {
        let input = target(1300.0, 0.0);
        assert!(solve_dynamic(&with_rocket(0.0, 2.0), &input).is_err());
        let powered = with_rocket(20.0, 2.0);
        let result = solve_dynamic(&powered, &input).unwrap();
        check_arcs(&powered, &input, &result);
    }

    #[test]
    fn honors_elevation_bounds_and_time_horizon() {
        let mut p = with_drag(0.02, 0.0);
        p.max_elevation_deg = 25.0;
        let mut input = target(400.0, 0.0);
        let result = solve_dynamic(&p, &input).unwrap();
        assert_eq!(result.arcs.len(), 1);
        assert!(result.arcs[0].elevation_deg <= 25.0);
        input.options.max_time = 0.1;
        assert!(solve_dynamic(&p, &input)
            .unwrap_err()
            .contains("nieosiągnięty"));
        assert!(simulate(&p, &simulation(26.0, 0.0, 1.0)).is_err());
    }

    #[test]
    fn fixed_elevation_profile_evaluates_one_actual_trajectory_without_duplicate_work() {
        let mut p = with_drag(0.0, 0.0);
        p.min_elevation_deg = 30.0;
        p.max_elevation_deg = 30.0;
        let time = 300.0 / (100.0 * 30.0_f64.to_radians().cos());
        let input = target(300.0, 50.0 * time - 5.0 * time * time);
        let result = solve_dynamic(&p, &input).unwrap();
        assert_eq!(result.evaluations, 1);
        assert_eq!(result.arcs.len(), 1);
        check_arcs(&p, &input, &result);
    }

    #[test]
    fn integration_and_shooting_work_limits_return_errors_instead_of_partial_hits() {
        let p = with_drag(0.02, 0.0);
        let mut sim = simulation(30.0, 0.0, 5.0);
        sim.options.max_steps = 1;
        assert!(simulate(&p, &sim).unwrap_err().contains("limit kroków"));
        let mut input = target(400.0, 0.0);
        input.options.max_work_steps = 1;
        assert!(solve_dynamic(&p, &input)
            .unwrap_err()
            .contains("limit kroków"));
        input.options = DynamicSolveOptions::default();
        input.options.max_evaluations = input.options.angle_samples;
        assert!(solve_dynamic(&p, &input)
            .unwrap_err()
            .contains("limit prób"));
    }

    #[test]
    fn rejects_unit_mismatch_missing_coefficients_unsafe_power_and_time() {
        let mut p = with_drag(0.02, 0.0);
        let mut input = simulation(30.0, 0.0, 5.0);
        input.unit = "game_meters".into();
        assert!(simulate(&p, &input).is_err());
        input.unit = "game_units".into();
        p.dynamics.as_mut().unwrap().linear_drag = None;
        assert!(simulate(&p, &input).is_err());
        p = with_drag(1e20, 0.0);
        assert!(simulate(&p, &input).is_err());
        p = with_rocket(1e20, 2.0);
        assert!(simulate(&p, &input).is_err());
        for duration in [0.0, 601.0, f64::NAN] {
            input.duration = duration;
            assert!(simulate(&profile(), &input).is_err());
        }
    }

    #[test]
    fn non_vacuum_data_never_enter_vacuum_solvers_or_guided_prediction() {
        let p = with_drag(0.0, 0.0);
        assert!(crate::solve(&p, 500.0, 0.0)
            .unwrap_err()
            .contains("lack_model"));
        let input = crate::InterceptInput3d {
            relative: [500.0, 0.0, 0.0],
            target_velocity: [0.0; 3],
            own_velocity: [0.0; 3],
            inherit_own_velocity: Some(false),
            unit: p.unit.clone(),
            max_time: 30.0,
        };
        assert!(crate::solve_intercept_3d(&p, &input)
            .unwrap_err()
            .contains("lack_model"));
        let options = crate::CalibrationOptions {
            unit: p.unit.clone(),
            max_residual_units: 1.0,
            max_relative_residual: 0.01,
            min_samples: 3,
            source: "Syntetyczny test".into(),
            game_version: None,
        };
        assert!(crate::fit_vacuum(&p, &[], &options)
            .unwrap_err()
            .contains("lack_model"));
        let mut guided = with_rocket(20.0, 2.0);
        guided.category = Category::GuidedMissile;
        assert!(simulate(&guided, &simulation(30.0, 0.0, 5.0)).is_err());
    }
}
