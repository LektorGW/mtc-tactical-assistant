use serde::{Deserialize, Serialize};

/// Provenance of a launch speed read from the game's own UI (e.g. the Loadout
/// "Specifications: Muzzle Velocity" line) or typed in by the player. It is an
/// observation, not a verification: the displayed unit and the gravity that
/// applies to the weapon stay explicit, unverified assumptions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedSpecs {
    /// `loadout` (read from the game UI) or `manual` (typed by the player).
    pub screen: String,
    /// When the value was read, e.g. an ISO 8601 timestamp.
    pub observed_at: String,
    #[serde(default)]
    pub weapon_name: Option<String>,
    /// Value exactly as displayed ("m/s"); must equal the profile speed.
    pub muzzle_velocity: f64,
    /// Explicit statement how the displayed "m/s" maps to the profile unit.
    pub velocity_unit_assumption: String,
    /// Where the profile gravity comes from (assumption until measured).
    pub gravity_source: String,
}

impl ObservedSpecs {
    pub fn validate(&self, speed: Option<f64>) -> Result<(), String> {
        let text_ok = |text: &str, maximum: usize| !text.trim().is_empty() && text.len() <= maximum;
        if !["loadout", "manual"].contains(&self.screen.as_str())
            || !text_ok(&self.observed_at, 64)
            || !text_ok(&self.velocity_unit_assumption, 512)
            || !text_ok(&self.gravity_source, 512)
            || self
                .weapon_name
                .as_ref()
                .is_some_and(|name| !text_ok(name, 200))
        {
            return Err("Niekompletne pochodzenie odczytu z interfejsu gry".into());
        }
        if !self.muzzle_velocity.is_finite() || self.muzzle_velocity <= 0.0 {
            return Err("Niepoprawna odczytana prędkość wylotowa".into());
        }
        match speed {
            Some(speed) if (speed - self.muzzle_velocity).abs() <= 1e-9 * speed.abs().max(1.0) => {
                Ok(())
            }
            _ => Err("Prędkość profilu musi być równa odczytanej prędkości wylotowej".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loadout() -> ObservedSpecs {
        ObservedSpecs {
            screen: "loadout".into(),
            observed_at: "2026-10-16T10:00:00Z".into(),
            weapon_name: Some("RPG-7V2 PG-7VR".into()),
            muzzle_velocity: 120.0,
            velocity_unit_assumption: "Założenie: m/s z UI = m gry/s".into(),
            gravity_source: "Założenie: 17.64 m gry/s² z kalkulatora".into(),
        }
    }

    #[test]
    fn accepts_complete_observation_matching_profile_speed() {
        loadout().validate(Some(120.0)).unwrap();
    }

    #[test]
    fn rejects_speed_mismatch_missing_assumptions_and_unknown_screen() {
        assert!(loadout().validate(Some(121.0)).is_err());
        assert!(loadout().validate(None).is_err());
        let mut o = loadout();
        o.gravity_source = " ".into();
        assert!(o.validate(Some(120.0)).is_err());
        let mut o = loadout();
        o.screen = "ocr_guess".into();
        assert!(o.validate(Some(120.0)).is_err());
        let mut o = loadout();
        o.muzzle_velocity = f64::NAN;
        assert!(o.validate(Some(120.0)).is_err());
    }
}
