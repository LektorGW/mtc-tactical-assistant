use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsModel {
    Vacuum,
    Drag,
    Rocket,
}

fn no_data() -> String {
    "no_data".into()
}

/// Phenomenological game coefficients, never inferred from a vehicle category.
/// Local calibration of launch speed/gravity does not calibrate these fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectileDynamics {
    pub model: DynamicsModel,
    pub unit: String,
    /// Acceleration = -linear_drag * velocity; units 1/second.
    pub linear_drag: Option<f64>,
    /// Acceleration = -quadratic_drag * |velocity| * velocity; units 1/game unit.
    pub quadratic_drag: Option<f64>,
    /// Constant thrust acceleration during burn_time; game units/second².
    pub rocket_acceleration: Option<f64>,
    pub burn_time: Option<f64>,
    /// Only a direction fixed at launch is implemented. It is an assumption,
    /// not a claim about MTC thrust, guidance, aerodynamic forces or drag.
    pub thrust_direction_fixed_launch: Option<bool>,
    #[serde(default)]
    pub calibrated: bool,
    #[serde(default = "no_data")]
    pub status: String,
    #[serde(default)]
    pub source: String,
}

impl ProjectileDynamics {
    pub fn validate(&self, profile_unit: &str) -> Result<(), String> {
        if !["game_units", "game_meters"].contains(&self.unit.as_str()) || self.unit != profile_unit
        {
            return Err("Jednostka dynamics musi odpowiadać jednostce profilu gry".into());
        }
        if !["no_data", "experimental", "beta"].contains(&self.status.as_str())
            || self.source.len() > 2048
        {
            return Err("Niepoprawny status lub źródło modelu dynamics".into());
        }
        for (name, value) in [
            ("linearDrag", self.linear_drag),
            ("quadraticDrag", self.quadratic_drag),
            ("rocketAcceleration", self.rocket_acceleration),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v < 0.0) {
                return Err(format!("Niepoprawny współczynnik dynamics: {name}"));
            }
        }
        if self.burn_time.is_some_and(|v| !v.is_finite() || v <= 0.0) {
            return Err("burnTime musi być skończoną dodatnią liczbą sekund".into());
        }
        match self.model {
            DynamicsModel::Vacuum => {
                if self.linear_drag.is_some()
                    || self.quadratic_drag.is_some()
                    || self.rocket_acceleration.is_some()
                    || self.burn_time.is_some()
                    || self.thrust_direction_fixed_launch.is_some()
                {
                    return Err(
                        "Model vacuum nie może po cichu ignorować współczynników dynamics".into(),
                    );
                }
            }
            DynamicsModel::Drag => {
                if self.rocket_acceleration.is_some()
                    || self.burn_time.is_some()
                    || self.thrust_direction_fixed_launch.is_some()
                {
                    return Err("Model drag nie może ignorować parametrów napędu rakiety".into());
                }
            }
            DynamicsModel::Rocket => {
                if self.thrust_direction_fixed_launch == Some(false) {
                    return Err("lack_model: obsługiwany jest wyłącznie jawnie zadeklarowany stały kierunek napędu przy starcie".into());
                }
            }
        }
        if self.calibrated {
            if self.status != "beta" || self.source.trim().is_empty() {
                return Err(
                    "Lokalna kalibracja dynamics wymaga statusu beta i opisu pomiarów w grze"
                        .into(),
                );
            }
            self.require_parameters()?;
        }
        Ok(())
    }

    pub fn require_parameters(&self) -> Result<(), String> {
        if self.model == DynamicsModel::Vacuum {
            return Ok(());
        }
        if self.linear_drag.is_none() || self.quadratic_drag.is_none() {
            return Err("lack_model: wymagane jawne linearDrag i quadraticDrag; potwierdzone zero jest dozwolone".into());
        }
        if self.model == DynamicsModel::Rocket
            && (self.rocket_acceleration.is_none()
                || self.burn_time.is_none()
                || self.thrust_direction_fixed_launch != Some(true))
        {
            return Err("lack_model: rakieta wymaga rocketAcceleration, burnTime i thrustDirectionFixedLaunch=true".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag() -> ProjectileDynamics {
        ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: "game_units".into(),
            linear_drag: Some(0.0),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test; brak pomiarów rzeczywistej gry".into(),
        }
    }

    #[test]
    fn explicit_zero_coefficients_are_valid_but_absent_coefficients_are_not_calibration() {
        let mut d = drag();
        d.validate("game_units").unwrap();
        d.linear_drag = None;
        assert!(d.validate("game_units").is_err());
        d.calibrated = false;
        d.validate("game_units").unwrap();
        assert!(d.require_parameters().is_err());
    }

    #[test]
    fn rejects_inconsistent_units_invalid_coefficients_and_false_verification() {
        let d = drag();
        assert!(d.validate("game_meters").is_err());
        for value in [-1.0, f64::NAN, f64::INFINITY] {
            let mut invalid = d.clone();
            invalid.quadratic_drag = Some(value);
            assert!(invalid.validate("game_units").is_err());
        }
        let mut invalid = d.clone();
        invalid.status = "verified".into();
        assert!(invalid.validate("game_units").is_err());
        invalid = d;
        invalid.source.clear();
        assert!(invalid.validate("game_units").is_err());
    }

    #[test]
    fn rocket_requires_declared_fixed_launch_direction_and_positive_burn_time() {
        let mut d = drag();
        d.model = DynamicsModel::Rocket;
        assert!(d.validate("game_units").is_err());
        d.rocket_acceleration = Some(0.0);
        d.burn_time = Some(2.0);
        d.thrust_direction_fixed_launch = Some(true);
        d.validate("game_units").unwrap();
        d.thrust_direction_fixed_launch = Some(false);
        assert!(d.validate("game_units").is_err());
        d.thrust_direction_fixed_launch = Some(true);
        d.burn_time = Some(0.0);
        assert!(d.validate("game_units").is_err());
    }

    #[test]
    fn selecting_vacuum_or_drag_does_not_silently_ignore_thrust() {
        let mut d = drag();
        d.model = DynamicsModel::Vacuum;
        assert!(d.validate("game_units").is_err());
        d = drag();
        d.rocket_acceleration = Some(1.0);
        assert!(d.validate("game_units").is_err());
    }
}
