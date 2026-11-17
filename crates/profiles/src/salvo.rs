use serde::{Deserialize, Serialize};

fn no_data() -> String {
    "no_data".into()
}

/// Explicit per-projectile launch dispersion used by the salvo model.
///
/// Every value must come from an observation, a local calibration or a cited
/// source. `None` means "brak danych"; it is never replaced by a default. The
/// angular value is interpreted as an isotropic 1σ deviation along each of the
/// two axes perpendicular to the launch direction (explicit model assumption).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SalvoDispersion {
    pub unit: String,
    /// 1σ angular launch dispersion per transverse axis, milliradians.
    #[serde(default)]
    pub angular_sigma_mrad: Option<f64>,
    /// 1σ launch speed dispersion, profile unit per second.
    #[serde(default)]
    pub speed_sigma: Option<f64>,
    /// Time between consecutive launches of one salvo, seconds (timing only).
    #[serde(default)]
    pub ripple_interval: Option<f64>,
    #[serde(default)]
    pub calibrated: bool,
    #[serde(default = "no_data")]
    pub status: String,
    #[serde(default)]
    pub source: String,
}

impl SalvoDispersion {
    pub fn validate(&self, profile_unit: &str) -> Result<(), String> {
        if !["game_units", "game_meters"].contains(&self.unit.as_str()) || self.unit != profile_unit
        {
            return Err("Jednostka rozrzutu salwy musi odpowiadać jednostce profilu".into());
        }
        if !["no_data", "experimental", "beta"].contains(&self.status.as_str())
            || self.source.len() > 2048
        {
            return Err("Niepoprawny status lub źródło rozrzutu salwy".into());
        }
        for (name, value, maximum) in [
            ("angularSigmaMrad", self.angular_sigma_mrad, 500.0),
            ("speedSigma", self.speed_sigma, 1e9),
        ] {
            if value.is_some_and(|v| !v.is_finite() || v < 0.0 || v > maximum) {
                return Err(format!("Niepoprawny parametr rozrzutu salwy: {name}"));
            }
        }
        if self
            .ripple_interval
            .is_some_and(|v| !v.is_finite() || v < 0.0 || v > 600.0)
        {
            return Err("rippleInterval musi być liczbą sekund od 0 do 600".into());
        }
        if self.calibrated && self.status != "beta" {
            return Err("Lokalna kalibracja rozrzutu salwy wymaga statusu beta".into());
        }
        if self.status != "no_data" {
            self.require_parameters()?;
        }
        Ok(())
    }

    /// Usable only with an explicit source, a non-`no_data` status and at
    /// least one measured dispersion component.
    pub fn require_parameters(&self) -> Result<(), String> {
        if self.status == "no_data" || self.source.trim().is_empty() {
            return Err(
                "Brak danych rozrzutu salwy: wymagane źródło i status experimental/beta".into(),
            );
        }
        if self.angular_sigma_mrad.is_none() && self.speed_sigma.is_none() {
            return Err("Brak danych rozrzutu salwy: wymagany rozrzut kątowy lub prędkości".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wiki() -> SalvoDispersion {
        SalvoDispersion {
            unit: "game_meters".into(),
            angular_sigma_mrad: Some(1.75),
            speed_sigma: None,
            ripple_interval: None,
            calibrated: false,
            status: "experimental".into(),
            source: "Test".into(),
        }
    }

    #[test]
    fn accepts_single_explicit_component_and_keeps_missing_ones_as_no_data() {
        let s = wiki();
        s.validate("game_meters").unwrap();
        s.require_parameters().unwrap();
        assert!(s.speed_sigma.is_none());
    }

    #[test]
    fn rejects_missing_source_components_units_and_false_calibration() {
        let mut s = wiki();
        s.source = " ".into();
        assert!(s.validate("game_meters").is_err());
        let mut s = wiki();
        s.angular_sigma_mrad = None;
        assert!(s.validate("game_meters").is_err());
        assert!(wiki().validate("game_units").is_err());
        let mut s = wiki();
        s.calibrated = true;
        assert!(s.validate("game_meters").is_err());
        for value in [-1.0, f64::NAN, f64::INFINITY, 501.0] {
            let mut s = wiki();
            s.angular_sigma_mrad = Some(value);
            assert!(s.validate("game_meters").is_err());
        }
        let mut s = wiki();
        s.status = "verified".into();
        assert!(s.validate("game_meters").is_err());
    }

    #[test]
    fn no_data_block_is_valid_but_never_usable() {
        let mut s = wiki();
        s.status = "no_data".into();
        s.angular_sigma_mrad = None;
        s.validate("game_meters").unwrap();
        assert!(s.require_parameters().is_err());
    }
}
