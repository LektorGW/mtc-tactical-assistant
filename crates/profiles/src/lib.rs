use serde::{Deserialize, Serialize};
mod dynamics;
pub use dynamics::{DynamicsModel, ProjectileDynamics};
mod observed;
mod resolver;
mod reticle;
mod salvo;
pub use observed::ObservedSpecs;
pub use resolver::{
    ballistically_equivalent, normalize_weapon_label, profile_identity, resolve_weapon_labels,
    resolve_weapon_labels_with_aliases, vehicle_aliases, weapon_identities, OcrWeaponLabel,
    ResolutionStatus, VehicleAlias, WeaponIdentity, WeaponResolution,
};
pub use reticle::{ReticleMap, ReticleMark};
pub use salvo::SalvoDispersion;

fn default_unit() -> String {
    "game_units".into()
}

/// Provenance for public community data; never a verification of the live game.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceMetadata {
    pub repository: String,
    pub commit: String,
    pub fetched_at: String,
    pub license: String,
    pub urls: Vec<String>,
    pub experience_id: String,
}

impl SourceMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if !self.repository.starts_with("https://")
            || self.commit.len() != 40
            || !self.commit.bytes().all(|c| c.is_ascii_hexdigit())
            || self.fetched_at.trim().is_empty()
            || self.license.trim().is_empty()
            || self.experience_id.is_empty()
            || !self.experience_id.bytes().all(|c| c.is_ascii_digit())
            || self.urls.is_empty()
            || self.urls.iter().any(|url| !url.starts_with("https://"))
        {
            return Err("Niekompletne pochodzenie danych społeczności".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    RocketArtillery,
    TubeArtillery,
    Tank,
    HelicopterGun,
    HelicopterRocket,
    GuidedMissile,
    AntiAircraft,
    /// Locally created direct-fire profile (hand-held/vehicle gun, flat fire).
    DirectFire,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeaponProfile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub category: Category,
    pub game_version: String,
    pub status: String,
    pub source: String,
    pub calibrated: bool,
    pub speed: Option<f64>,
    pub gravity: Option<f64>,
    pub min_elevation_deg: f64,
    pub max_elevation_deg: f64,
    pub dispersion_sigma: Option<f64>,
    pub salvo_size: u32,
    pub notes: String,
    /// Legacy local profiles retain arbitrary, mutually consistent game units.
    #[serde(default = "default_unit")]
    pub unit: String,
    #[serde(default)]
    pub source_metadata: Option<SourceMetadata>,
    /// Opt-in per projectile, reset to false by every community import.
    #[serde(default)]
    pub source_acknowledged: bool,
    /// Absent in schema-1 snapshots: retain the legacy game vacuum model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<ProjectileDynamics>,
    /// Optional explicit launch dispersion for the salvo model; absent = no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salvo: Option<SalvoDispersion>,
    /// Speed read from the game UI or typed by the player, with assumptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_specs: Option<ObservedSpecs>,
    /// Optional player-defined sight graduation map (direct fire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reticle: Option<ReticleMap>,
}

impl WeaponProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.source.trim().is_empty()
            || self.game_version.trim().is_empty()
            || !["no_data", "experimental", "beta", "verified"].contains(&self.status.as_str())
        {
            return Err("Niepoprawny schemat lub pochodzenie profilu".into());
        }
        for (name, value) in [("prędkość", self.speed), ("grawitacja", self.gravity)] {
            if value.is_some_and(|x| !x.is_finite() || x <= 0.0) {
                return Err(format!("Niepoprawna wartość: {name}"));
            }
        }
        if !self.min_elevation_deg.is_finite()
            || !self.max_elevation_deg.is_finite()
            || self.min_elevation_deg < -89.0
            || self.max_elevation_deg > 89.0
            || self.min_elevation_deg > self.max_elevation_deg
            || self.salvo_size == 0
            || self.salvo_size > 1000
            || self
                .dispersion_sigma
                .is_some_and(|x| !x.is_finite() || x < 0.0)
        {
            return Err("Niepoprawne limity elewacji, salwy lub rozrzutu".into());
        }
        if self.calibrated && (self.speed.is_none() || self.gravity.is_none()) {
            return Err("Kalibracja wymaga prędkości i grawitacji".into());
        }
        if self.category == Category::GuidedMissile && self.calibrated {
            return Err("Profil naprowadzany nie obsługuje modelu balistycznego".into());
        }
        if !["game_units", "game_meters"].contains(&self.unit.as_str()) {
            return Err("Nieznana jednostka profilu".into());
        }
        if let Some(source) = &self.source_metadata {
            source.validate()?;
        }
        if let Some(dynamics) = &self.dynamics {
            dynamics.validate(&self.unit)?;
            if self.category == Category::GuidedMissile && dynamics.model != DynamicsModel::Vacuum {
                return Err("Mechanika naprowadzania jest nieznana; model oporu i napędu dla profilu naprowadzanego jest niedostępny".into());
            }
        }
        if let Some(salvo) = &self.salvo {
            salvo.validate(&self.unit)?;
        }
        if let Some(observed) = &self.observed_specs {
            // After a local calibration the reading stays as history only.
            observed.validate(if self.calibrated {
                Some(observed.muzzle_velocity)
            } else {
                self.speed
            })?;
        }
        if let Some(reticle) = &self.reticle {
            reticle.validate()?;
        }
        Ok(())
    }

    pub fn dynamics_model(&self) -> DynamicsModel {
        self.dynamics
            .as_ref()
            .map_or(DynamicsModel::Vacuum, |d| d.model)
    }

    pub fn require_vacuum(&self) -> Result<(), String> {
        if self.dynamics_model() != DynamicsModel::Vacuum {
            return Err("lack_model: wzór vacuum nie obsługuje aktywnych dynamics; użyj solvera numerycznego, a przechwycenie ruchomego celu pozostaje niedostępne".into());
        }
        Ok(())
    }

    /// Allow a local calibration or an explicitly acknowledged experimental source.
    /// Acknowledgement does not set calibrated or upgrade the validation status.
    pub fn can_calculate(&self) -> Result<(), String> {
        self.validate()?;
        if self.category == Category::GuidedMissile {
            return Err("Mechanika naprowadzania niezweryfikowana — brak predykcji".into());
        }
        if self.speed.is_none() || self.gravity.is_none() {
            return Err("Wymagana kalibracja profilu pocisku; brak parametrów".into());
        }
        if self.dynamics_model() != DynamicsModel::Vacuum {
            let dynamics = self.dynamics.as_ref().ok_or("Brak dynamics")?;
            dynamics.require_parameters()?;
            if !self.calibrated
                || self.status != "beta"
                || !dynamics.calibrated
                || dynamics.status != "beta"
                || dynamics.source.trim().is_empty()
            {
                return Err("Wymagana jawna lokalna kalibracja beta parametrów dynamics; akceptacja community nie kalibruje oporu ani napędu".into());
            }
            return Ok(());
        }
        if self.calibrated {
            return Ok(());
        }
        // Uncalibrated data need explicit provenance: a community snapshot or
        // a value read from the game UI / typed by the player, plus opt-in.
        if (self.source_metadata.is_none() && self.observed_specs.is_none())
            || !["experimental", "beta"].contains(&self.status.as_str())
        {
            return Err("Wymagana kalibracja profilu pocisku".into());
        }
        if !self.source_acknowledged {
            return Err(if self.source_metadata.is_some() {
                "Potwierdź użycie niezweryfikowanych danych społeczności dla tego pocisku".into()
            } else {
                "Potwierdź założenia odczytu z interfejsu gry (jednostka i grawitacja)".into()
            });
        }
        Ok(())
    }
}

pub fn built_in() -> Vec<WeaponProfile> {
    serde_json::from_str(include_str!("../../../data/weapons/builtin.json"))
        .expect("invalid bundled profiles")
}

pub fn community_profiles() -> Vec<WeaponProfile> {
    serde_json::from_str(include_str!(
        "../../../data/weapons/community-profiles.json"
    ))
    .expect("invalid bundled community profiles")
}

pub fn all_profiles() -> Vec<WeaponProfile> {
    let mut profiles = built_in();
    profiles.extend(community_profiles());
    profiles
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeightRange {
    pub minimum: f64,
    pub maximum: f64,
}

/// World dimensions from source config, not screen calibration or a heightmap.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommunityMap {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub key: String,
    pub size_studs: f64,
    pub size_game_meters: f64,
    pub unit: String,
    pub status: String,
    pub in_rotation: bool,
    pub height_range_studs: Option<HeightRange>,
    pub source_metadata: SourceMetadata,
}

impl CommunityMap {
    pub fn validate(&self) -> Result<(), String> {
        self.source_metadata.validate()?;
        if self.schema_version != 1
            || self.id.is_empty()
            || self.name.is_empty()
            || self.key.is_empty()
            || self.unit != "game_meters"
            || self.status != "experimental"
            || !self.size_studs.is_finite()
            || self.size_studs <= 0.0
            || !self.size_game_meters.is_finite()
            || self.size_game_meters <= 0.0
            || (self.size_game_meters - self.size_studs * 0.36).abs() > 1e-8
            || self.height_range_studs.as_ref().is_some_and(|h| {
                !h.minimum.is_finite() || !h.maximum.is_finite() || h.minimum > h.maximum
            })
        {
            return Err("Niepoprawne wymiary lub pochodzenie mapy społeczności".into());
        }
        Ok(())
    }

    /// Only use for a visible, complete square map image with manually confirmed
    /// extent. This cannot infer minimap zoom, crop, rotation, or map identity.
    pub fn game_meters_per_pixel(
        &self,
        complete_map_width_pixels: f64,
        source_acknowledged: bool,
    ) -> Result<f64, String> {
        self.validate()?;
        if !source_acknowledged {
            return Err("Potwierdź eksperymentalny wymiar mapy społeczności".into());
        }
        if !complete_map_width_pixels.is_finite() || complete_map_width_pixels < 1.0 {
            return Err("Wymagana szerokość pełnego obrazu mapy".into());
        }
        Ok(self.size_game_meters / complete_map_width_pixels)
    }
}

pub fn community_maps() -> Vec<CommunityMap> {
    serde_json::from_str(include_str!("../../../data/maps/community-maps.json"))
        .expect("invalid bundled community maps")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_built_in_categories_have_valid_profiles_without_fabricated_constants() {
        let p = built_in();
        assert_eq!(p.len(), 7);
        for profile in p {
            profile.validate().unwrap();
            assert!(!profile.calibrated);
            assert!(profile.speed.is_none());
        }
    }
    #[test]
    fn rejects_false_calibration_and_unknown_schema() {
        let mut p = built_in().remove(0);
        p.calibrated = true;
        assert!(p.validate().is_err());
        p.calibrated = false;
        p.schema_version = 2;
        assert!(p.validate().is_err());
    }
    #[test]
    fn rejects_nonfinite_parameters() {
        let mut p = built_in().remove(0);
        p.speed = Some(f64::NAN);
        assert!(p.validate().is_err());
    }
    #[test]
    fn community_snapshot_preserves_provenance_and_requires_opt_in_per_projectile() {
        let profiles = community_profiles();
        assert_eq!(profiles.len(), 153);
        let mut ids = std::collections::HashSet::new();
        for mut p in profiles {
            p.validate().unwrap();
            assert!(ids.insert(p.id.clone()));
            assert_eq!(p.unit, "game_meters");
            assert_eq!(p.status, "experimental");
            assert!(!p.calibrated);
            assert!(!p.source_acknowledged);
            assert!(p.can_calculate().is_err());
            assert_eq!(
                p.source_metadata.as_ref().unwrap().experience_id,
                "9520328730"
            );
            p.source_acknowledged = true;
            assert_eq!(
                p.can_calculate().is_ok(),
                p.category != Category::GuidedMissile
            );
            assert!(!p.calibrated);
        }
    }
    #[test]
    fn acknowledgement_without_parameters_and_provenance_is_not_calibration() {
        let mut p = built_in().remove(0);
        p.source_acknowledged = true;
        assert!(p.can_calculate().is_err());
        p.speed = Some(100.0);
        p.gravity = Some(10.0);
        assert!(p.can_calculate().is_err());
        p.unit = "real_world_meters".into();
        assert!(p.validate().is_err());
    }
    #[test]
    fn rejects_broken_source_commit() {
        let mut p = community_profiles().remove(0);
        p.source_metadata.as_mut().unwrap().commit = "HEAD".into();
        assert!(p.validate().is_err());
    }
    #[test]
    fn old_snapshot_omits_dynamics_and_community_acknowledgement_cannot_enable_new_coefficients() {
        let profiles = all_profiles();
        assert!(profiles.iter().all(|p| p.dynamics.is_none()));
        assert!(built_in().iter().all(|p| p.salvo.is_none()));
        assert!(profiles.iter().all(|p| !serde_json::to_value(p)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("dynamics")));
        let mut profile = community_profiles().remove(0);
        profile.source_acknowledged = true;
        profile.dynamics = Some(ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: profile.unit.clone(),
            linear_drag: Some(0.1),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: true,
            status: "beta".into(),
            source: "Syntetyczny test współczynników".into(),
        });
        assert!(profile.can_calculate().is_err());
        profile.calibrated = true;
        profile.status = "beta".into();
        profile.can_calculate().unwrap();
        assert!(profile.require_vacuum().is_err());
        profile.dynamics.as_mut().unwrap().calibrated = false;
        assert!(profile.can_calculate().is_err());
    }
    #[test]
    fn rejects_drag_dynamics_on_guided_profile_like_ui_validation() {
        let mut profile = built_in()
            .into_iter()
            .find(|p| p.category == Category::GuidedMissile)
            .unwrap();
        profile.dynamics = Some(ProjectileDynamics {
            model: DynamicsModel::Drag,
            unit: profile.unit.clone(),
            linear_drag: Some(0.1),
            quadratic_drag: Some(0.0),
            rocket_acceleration: None,
            burn_time: None,
            thrust_direction_fixed_launch: None,
            calibrated: false,
            status: "no_data".into(),
            source: String::new(),
        });
        assert!(profile.validate().unwrap_err().contains("naprowadzania"));
    }
    #[test]
    fn only_the_cited_grad_profile_carries_explicit_experimental_salvo_dispersion() {
        let curated: serde_json::Value = serde_json::from_str(include_str!(
            "../../../data/weapons/community/curated-parameters.json"
        ))
        .unwrap();
        let profiles = community_profiles();
        let with_salvo: Vec<_> = profiles.iter().filter(|p| p.salvo.is_some()).collect();
        assert_eq!(with_salvo.len(), 1);
        let grad = with_salvo[0];
        assert_eq!(grad.id, "community-bm_21_grad-1");
        let salvo = grad.salvo.as_ref().unwrap();
        assert_eq!(salvo.angular_sigma_mrad, Some(1.75));
        assert!(salvo.speed_sigma.is_none());
        assert_eq!(salvo.status, "experimental");
        assert!(!salvo.calibrated);
        assert!(salvo.source.contains("https://multicrew.wiki/"));
        assert!(salvo.source.contains("2026-10-15"));
        assert_eq!(
            serde_json::to_value(salvo).unwrap(),
            curated["profiles"]["community-bm_21_grad-1"]["salvo"]
        );
        assert!(grad.dispersion_sigma.is_none());
        assert_eq!(grad.status, "experimental");
    }
    fn direct_fire_profile() -> WeaponProfile {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "id": "direct-test", "name": "RPG-7V2 / PG-7VR",
            "category": "direct_fire", "gameVersion": "odczyt UI gry 2026-10-16",
            "status": "experimental", "source": "odczytane z UI gry 2026-10-16 (Loadout)",
            "calibrated": false, "speed": 120.0, "gravity": 17.64,
            "minElevationDeg": -89, "maxElevationDeg": 89, "dispersionSigma": null,
            "salvoSize": 1, "notes": "", "unit": "game_meters", "sourceAcknowledged": false,
            "observedSpecs": {
                "screen": "loadout", "observedAt": "2026-10-16T10:00:00Z",
                "weaponName": "RPG-7V2 PG-7VR", "muzzleVelocity": 120.0,
                "velocityUnitAssumption": "Założenie: m/s z UI = m gry/s",
                "gravitySource": "Założenie: 17.64 m gry/s² z kalkulatora grand-hawk"
            }
        }))
        .unwrap()
    }
    #[test]
    fn observed_loadout_speed_is_usable_only_after_acknowledging_assumptions() {
        let mut p = direct_fire_profile();
        p.validate().unwrap();
        assert!(p.can_calculate().unwrap_err().contains("założenia"));
        p.source_acknowledged = true;
        p.can_calculate().unwrap();
        assert!(!p.calibrated);
        p.observed_specs = None;
        assert!(p.can_calculate().is_err());
    }
    #[test]
    fn observed_speed_must_match_profile_and_unknown_fields_are_rejected() {
        let mut p = direct_fire_profile();
        p.speed = Some(130.0);
        assert!(p.validate().is_err());
        let mut value = serde_json::to_value(direct_fire_profile()).unwrap();
        value["observedSpecs"]["guessedDrag"] = serde_json::json!(0.1);
        assert!(serde_json::from_value::<WeaponProfile>(value).is_err());
        let mut value = serde_json::to_value(direct_fire_profile()).unwrap();
        value["verified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<WeaponProfile>(value).is_err());
    }
    #[test]
    fn map_catalog_converts_source_studs_and_does_not_infer_height() {
        let maps = community_maps();
        assert_eq!(maps.len(), 25);
        for m in &maps {
            m.validate().unwrap();
            assert!(m.game_meters_per_pixel(1000.0, false).is_err());
        }
        let m = maps.iter().find(|m| m.id == "normandy").unwrap();
        assert!((m.size_game_meters - 2151.36).abs() < 1e-8);
        assert!((m.game_meters_per_pixel(1000.0, true).unwrap() - 2.15136).abs() < 1e-8);
        assert!(m.game_meters_per_pixel(f64::NAN, true).is_err());
        assert!(m.game_meters_per_pixel(0.0, true).is_err());
    }
}
