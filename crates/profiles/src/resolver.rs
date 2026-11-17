use crate::{Category, WeaponProfile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OcrWeaponLabel {
    pub text: String,
    /// Confidence supplied by the OCR observation, not created by this resolver.
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponIdentity {
    pub profile_id: String,
    pub vehicle: String,
    pub projectile: Option<String>,
    pub category: Category,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatus {
    Matched,
    Ambiguous,
    NoMatch,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponResolution {
    pub status: ResolutionStatus,
    pub profile_id: Option<String>,
    pub candidates: Vec<WeaponIdentity>,
    /// Other catalog profiles with the same projectile and exactly identical
    /// ballistics as `profile_id` (e.g. M109 / M109A6). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equivalents: Vec<WeaponIdentity>,
    pub confidence: f64,
    pub evidence: Vec<String>,
}

/// Conservative normalization: punctuation/whitespace and case only. No
/// substitutions such as O->0, fuzzy substrings, or visually inferred weapons.
pub fn normalize_weapon_label(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The community name is flattened from the MIT snapshot's gun.name and
/// projectile.name using exactly " / ". Splitting that delimiter once keeps
/// projectile labels such as "9M22/M21 HE-Frag" intact.
pub fn profile_identity(profile: &WeaponProfile) -> WeaponIdentity {
    let (vehicle, projectile) = match profile.name.split_once(" / ") {
        Some((vehicle, projectile)) => (vehicle.trim(), Some(projectile.trim().to_string())),
        None => (profile.name.trim(), None),
    };
    WeaponIdentity {
        profile_id: profile.id.clone(),
        vehicle: vehicle.into(),
        projectile,
        category: profile.category.clone(),
    }
}

pub fn weapon_identities(profiles: &[WeaponProfile]) -> Vec<WeaponIdentity> {
    profiles.iter().map(profile_identity).collect()
}

fn normalized(label: Option<&OcrWeaponLabel>) -> Result<Option<(String, f64)>, String> {
    match label {
        None => Ok(None),
        Some(label) => {
            if label.text.len() > 512
                || !label.confidence.is_finite()
                || !(0.0..=1.0).contains(&label.confidence)
            {
                return Err("Niepoprawna jawna obserwacja nazwy uzbrojenia".into());
            }
            let text = normalize_weapon_label(&label.text);
            Ok((!text.is_empty()).then_some((text, label.confidence)))
        }
    }
}

/// Curated in-game weapon designation → catalog vehicle names. A narrowing
/// hint only (status experimental); it never creates parameters or profiles.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VehicleAlias {
    pub observed: String,
    pub vehicles: Vec<String>,
    pub status: String,
    pub source: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AliasFile {
    schema_version: u32,
    #[allow(dead_code)]
    description: String,
    aliases: Vec<VehicleAlias>,
}

pub fn vehicle_aliases() -> Vec<VehicleAlias> {
    let file: AliasFile =
        serde_json::from_str(include_str!("../../../data/weapons/vehicle-aliases.json"))
            .expect("invalid bundled vehicle aliases");
    assert_eq!(file.schema_version, 1, "unsupported vehicle alias schema");
    file.aliases
}

/// Exact comparison of everything that changes a trajectory or its spread.
pub fn ballistically_equivalent(a: &WeaponProfile, b: &WeaponProfile) -> bool {
    let json = |value: serde_json::Result<serde_json::Value>| value.ok();
    a.speed == b.speed
        && a.gravity == b.gravity
        && a.unit == b.unit
        && a.min_elevation_deg == b.min_elevation_deg
        && a.max_elevation_deg == b.max_elevation_deg
        && a.dispersion_sigma == b.dispersion_sigma
        && a.salvo_size == b.salvo_size
        && json(serde_json::to_value(&a.dynamics)) == json(serde_json::to_value(&b.dynamics))
        && json(serde_json::to_value(&a.salvo)) == json(serde_json::to_value(&b.salvo))
}

/// Resolve explicit labels independently of calibration/model availability.
/// A vehicle label with several projectile profiles stays ambiguous, and a
/// contradictory ammunition label never falls back to the first candidate.
/// Candidates with exactly identical ballistics form one unambiguous result.
pub fn resolve_weapon_labels(
    profiles: &[WeaponProfile],
    vehicle: Option<&OcrWeaponLabel>,
    ammunition: Option<&OcrWeaponLabel>,
) -> Result<WeaponResolution, String> {
    resolve_weapon_labels_with_aliases(profiles, &vehicle_aliases(), vehicle, ammunition)
}

pub fn resolve_weapon_labels_with_aliases(
    profiles: &[WeaponProfile],
    aliases: &[VehicleAlias],
    vehicle: Option<&OcrWeaponLabel>,
    ammunition: Option<&OcrWeaponLabel>,
) -> Result<WeaponResolution, String> {
    let vehicle = normalized(vehicle)?;
    let ammunition = normalized(ammunition)?;
    let mut evidence = vec![
        "Dopasowanie wyłącznie jawnych etykiet tekstowych; bez oceny sylwetki".into(),
        "Identyfikacja nie potwierdza fizyki, parametrów ani kalibracji".into(),
    ];
    if vehicle.is_none() && ammunition.is_none() {
        return Ok(WeaponResolution {
            status: ResolutionStatus::NoMatch,
            profile_id: None,
            candidates: Vec::new(),
            equivalents: Vec::new(),
            confidence: 0.0,
            evidence: vec!["Brak jawnej etykiety pojazdu lub amunicji".into()],
        });
    }
    let projectile_of = |profile: &WeaponProfile| {
        profile_identity(profile)
            .projectile
            .map(|name| normalize_weapon_label(&name))
    };
    let ammo_match = |profile: &WeaponProfile| {
        ammunition
            .as_ref()
            .is_none_or(|(label, _)| projectile_of(profile).as_ref() == Some(label))
    };
    let mut matched: Vec<&WeaponProfile> = Vec::new();
    let mut used_full_label = false;
    let mut vehicle_known = vehicle.is_none();
    for profile in profiles {
        let identity = profile_identity(profile);
        let full_match = vehicle.as_ref().is_some_and(|(label, _)| {
            identity.projectile.is_some() && *label == normalize_weapon_label(&profile.name)
        });
        let vehicle_match = vehicle.as_ref().is_none_or(|(label, _)| {
            *label == normalize_weapon_label(&identity.vehicle) || full_match
        });
        vehicle_known |= vehicle_match;
        if vehicle_match && ammo_match(profile) {
            matched.push(profile);
            used_full_label |= full_match;
        }
    }
    if let Some((label, _)) = &vehicle {
        evidence.push(format!(
            "Etykieta pojazdu/pełna: {label}; reguła: {}",
            if used_full_label {
                "pełna nazwa dokładna"
            } else {
                "nazwa pojazdu dokładna"
            }
        ));
    }
    if let Some((label, _)) = &ammunition {
        evidence.push(format!(
            "Etykieta amunicji: {label}; wymagana nazwa dokładna"
        ));
    }
    // Panel designation unknown to the catalog (e.g. "155 mm M185"): fall back
    // to the exact ammunition name, narrowed by a curated alias when present.
    let mut preferred_vehicles: Vec<String> = Vec::new();
    if !vehicle_known && ammunition.is_some() {
        let alias = vehicle.as_ref().and_then(|(label, _)| {
            aliases
                .iter()
                .find(|a| normalize_weapon_label(&a.observed) == *label)
        });
        let by_ammo: Vec<&WeaponProfile> = profiles.iter().filter(|p| ammo_match(p)).collect();
        if let Some(alias) = alias {
            preferred_vehicles = alias
                .vehicles
                .iter()
                .map(|v| normalize_weapon_label(v))
                .collect();
            evidence.push(format!(
                "Oznaczenie z panelu „{}” → {} (alias {}: {})",
                alias.observed,
                if alias.vehicles.is_empty() {
                    "brak pojazdu w katalogu".to_string()
                } else {
                    alias.vehicles.join(", ")
                },
                alias.status,
                alias.source
            ));
            let narrowed: Vec<&WeaponProfile> = by_ammo
                .iter()
                .copied()
                .filter(|p| {
                    preferred_vehicles
                        .contains(&normalize_weapon_label(&profile_identity(p).vehicle))
                })
                .collect();
            if !narrowed.is_empty() {
                matched = narrowed;
            } else if alias.vehicles.is_empty() {
                matched = by_ammo;
            } else {
                evidence.push(
                    "Alias wskazuje pojazd bez tej amunicji w katalogu — brak profilu".into(),
                );
            }
        } else {
            evidence.push(
                "Oznaczenie broni spoza katalogu; dopasowanie po dokładnej nazwie amunicji".into(),
            );
            matched = by_ammo;
        }
    }
    let confidence = vehicle
        .iter()
        .chain(ammunition.iter())
        .map(|(_, confidence)| *confidence)
        .min_by(f64::total_cmp)
        .unwrap_or(0.0);
    matched.sort_by(|a, b| a.id.cmp(&b.id));
    let projectile_unobserved = ammunition.is_none() && !used_full_label;
    let representative = matched
        .iter()
        .copied()
        .find(|p| {
            preferred_vehicles.contains(&normalize_weapon_label(&profile_identity(p).vehicle))
        })
        .or_else(|| matched.first().copied());
    let all_equivalent =
        representative.is_some_and(|rep| matched.iter().all(|p| ballistically_equivalent(rep, p)));
    let status = match matched.len() {
        0 => ResolutionStatus::NoMatch,
        _ if projectile_unobserved => ResolutionStatus::Ambiguous,
        1 => ResolutionStatus::Matched,
        _ if all_equivalent => ResolutionStatus::Matched,
        _ => ResolutionStatus::Ambiguous,
    };
    let mut equivalents = Vec::new();
    if let (ResolutionStatus::Matched, Some(rep)) = (&status, representative) {
        // Same projectile name and identical ballistics anywhere in the catalog.
        let name = projectile_of(rep);
        equivalents = profiles
            .iter()
            .filter(|p| {
                p.id != rep.id && projectile_of(p) == name && ballistically_equivalent(rep, p)
            })
            .map(profile_identity)
            .collect();
        equivalents.sort_by(|a, b| a.profile_id.cmp(&b.profile_id));
        if !equivalents.is_empty() {
            let mut names: Vec<String> = std::iter::once(profile_identity(rep).vehicle)
                .chain(equivalents.iter().map(|e| e.vehicle.clone()))
                .collect();
            names.dedup();
            evidence.push(format!(
                "{} — identyczne parametry balistyczne",
                names.join(" / ")
            ));
        }
    }
    if status == ResolutionStatus::Ambiguous {
        evidence.push(if projectile_unobserved {
            "Pojazd rozpoznany, amunicja nieobserwowana — wybierz lub potwierdź pocisk".into()
        } else {
            "Kilka profili o różnych parametrach pasuje; wybór pocisku jest zablokowany".into()
        });
    }
    let candidates: Vec<WeaponIdentity> = matched.iter().map(|p| profile_identity(p)).collect();
    Ok(WeaponResolution {
        confidence: if status == ResolutionStatus::NoMatch {
            0.0
        } else {
            confidence
        },
        profile_id: (status == ResolutionStatus::Matched)
            .then(|| representative.map(|p| p.id.clone()))
            .flatten(),
        status,
        candidates,
        equivalents,
        evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn label(text: &str) -> OcrWeaponLabel {
        OcrWeaponLabel {
            text: text.into(),
            confidence: 0.92,
        }
    }
    #[test]
    fn resolves_every_community_full_name_without_claiming_calibration() {
        let profiles = crate::community_profiles();
        for profile in &profiles {
            let observed = label(&profile.name.to_uppercase());
            let result = resolve_weapon_labels(&profiles, Some(&observed), None).unwrap();
            assert_eq!(result.status, ResolutionStatus::Matched);
            assert_eq!(result.profile_id.as_deref(), Some(profile.id.as_str()));
            assert!(!profile.calibrated);
        }
    }
    #[test]
    fn vehicle_only_returns_ammunition_choices_and_exact_ammo_narrows() {
        let profiles = crate::community_profiles();
        let vehicle = label("M109");
        let ambiguous = resolve_weapon_labels(&profiles, Some(&vehicle), None).unwrap();
        assert_eq!(ambiguous.status, ResolutionStatus::Ambiguous);
        assert_eq!(ambiguous.candidates.len(), 12);
        assert!(ambiguous.profile_id.is_none());
        let ammunition = label("M107 Low Charge VT");
        let matched = resolve_weapon_labels(&profiles, Some(&vehicle), Some(&ammunition)).unwrap();
        assert_eq!(matched.status, ResolutionStatus::Matched);
        assert_eq!(
            matched.candidates[0].projectile.as_deref(),
            Some("M107 Low Charge VT")
        );
    }
    #[test]
    fn shared_ammunition_and_contradictory_labels_do_not_guess() {
        let profiles = crate::community_profiles();
        // Shared ammunition with identical ballistics is one unambiguous result.
        let vog = resolve_weapon_labels(&profiles, None, Some(&label("VOG-17M"))).unwrap();
        assert_eq!(vog.status, ResolutionStatus::Matched);
        assert_eq!(vog.equivalents.len(), vog.candidates.len() - 1);
        // Shared name with different parameters stays ambiguous.
        assert_eq!(
            resolve_weapon_labels(&profiles, None, Some(&label("Medium Charge")))
                .unwrap()
                .status,
            ResolutionStatus::Ambiguous
        );
        assert_eq!(
            resolve_weapon_labels(
                &profiles,
                Some(&label("BM-21 Grad")),
                Some(&label("M107 Low Charge"))
            )
            .unwrap()
            .status,
            ResolutionStatus::NoMatch
        );
        assert_eq!(
            resolve_weapon_labels(&profiles, Some(&label("BM-21")), None)
                .unwrap()
                .status,
            ResolutionStatus::NoMatch
        );
        assert_eq!(
            resolve_weapon_labels(&profiles, Some(&label("BM-2I Grad")), None)
                .unwrap()
                .status,
            ResolutionStatus::NoMatch
        );
    }
    #[test]
    fn preserves_slashes_in_ammunition_and_uses_weaker_observed_confidence() {
        let profiles = crate::community_profiles();
        let vehicle = label("bm 21 grad");
        let ammunition = OcrWeaponLabel {
            text: "9M22/M21 HE-Frag".into(),
            confidence: 0.4,
        };
        let result = resolve_weapon_labels(&profiles, Some(&vehicle), Some(&ammunition)).unwrap();
        assert_eq!(result.status, ResolutionStatus::Matched);
        assert_eq!(result.confidence, 0.4);
        assert_eq!(
            result.candidates[0].projectile.as_deref(),
            Some("9M22/M21 HE-Frag")
        );
    }
    #[test]
    fn lone_vehicle_with_one_known_projectile_still_requires_ammunition_confirmation() {
        let profiles = crate::community_profiles();
        let result = resolve_weapon_labels(&profiles, Some(&label("BM-21 Grad")), None).unwrap();
        assert_eq!(result.status, ResolutionStatus::Ambiguous);
        assert_eq!(result.candidates.len(), 1);
        assert!(result.profile_id.is_none());
        let mut unnamed_ammunition = crate::built_in().remove(1);
        unnamed_ammunition.name = "Test vehicle without projectile label".into();
        let result = resolve_weapon_labels(
            &[unnamed_ammunition.clone()],
            Some(&label(&unnamed_ammunition.name)),
            None,
        )
        .unwrap();
        assert_eq!(result.status, ResolutionStatus::Ambiguous);
        assert!(result.profile_id.is_none());
    }
    #[test]
    fn imported_source_projectile_labels_agree_with_structured_identities() {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../data/weapons/community/source/importedGuns.json"
        ))
        .unwrap();
        let profiles = crate::community_profiles();
        for (key, gun) in source.as_object().unwrap() {
            for (index, projectile) in gun["projectiles"].as_array().unwrap().iter().enumerate() {
                let id = format!(
                    "community-{}-{}",
                    key.to_lowercase().trim_matches('_'),
                    index + 1
                );
                let identity =
                    profile_identity(profiles.iter().find(|profile| profile.id == id).unwrap());
                assert_eq!(identity.vehicle, gun["name"].as_str().unwrap());
                assert_eq!(
                    identity.projectile.as_deref(),
                    Some(projectile["name"].as_str().unwrap().trim())
                );
            }
        }
    }
    #[test]
    fn normalized_collisions_and_invalid_confidence_are_blocked() {
        let mut profiles = crate::built_in();
        profiles[0].name = "AB / HE".into();
        profiles[1].name = "A-B / HE".into();
        assert_eq!(
            resolve_weapon_labels(&profiles, Some(&label("AB / HE")), None)
                .unwrap()
                .status,
            ResolutionStatus::Ambiguous
        );
        let invalid = OcrWeaponLabel {
            text: "M109".into(),
            confidence: f64::NAN,
        };
        assert!(resolve_weapon_labels(&profiles, Some(&invalid), None).is_err());
    }

    #[test]
    fn panel_designation_resolves_by_exact_ammunition_with_equivalent_vehicle() {
        let profiles = crate::community_profiles();
        let panel = label("155 mm M185");
        let result =
            resolve_weapon_labels(&profiles, Some(&panel), Some(&label("M107 Medium Charge")))
                .unwrap();
        assert_eq!(result.status, ResolutionStatus::Matched);
        assert_eq!(result.profile_id.as_deref(), Some("community-m109-5"));
        let equivalents: Vec<_> = result
            .equivalents
            .iter()
            .map(|e| e.profile_id.as_str())
            .collect();
        assert_eq!(equivalents, ["community-m109a6-5"]);
        assert!(result
            .evidence
            .iter()
            .any(|e| e.contains("identyczne parametry")));
        assert!(result.evidence.iter().any(|e| e.contains("2026-10-16")));
        let pf = resolve_weapon_labels(
            &profiles,
            Some(&panel),
            Some(&label("M107 Medium Charge PF")),
        )
        .unwrap();
        assert_eq!(pf.profile_id.as_deref(), Some("community-m109-6"));
        // Without the alias the exact ammunition still resolves the equivalent pair.
        let plain = resolve_weapon_labels_with_aliases(
            &profiles,
            &[],
            Some(&panel),
            Some(&label("M107 Medium Charge")),
        )
        .unwrap();
        assert_eq!(plain.status, ResolutionStatus::Matched);
        assert_eq!(plain.candidates.len(), 2);
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!(json["equivalents"][0]["profileId"], "community-m109a6-5");
    }

    #[test]
    fn shared_ammunition_with_different_parameters_stays_ambiguous() {
        let mut profiles = crate::community_profiles();
        let result = resolve_weapon_labels(
            &profiles,
            Some(&label("Rocketetet")),
            Some(&label("HE Rockets")),
        )
        .unwrap();
        assert_eq!(result.status, ResolutionStatus::Ambiguous);
        assert!(result.profile_id.is_none());
        assert!(result.equivalents.is_empty());
        let a6 = profiles
            .iter_mut()
            .find(|p| p.id == "community-m109a6-5")
            .unwrap();
        a6.speed = Some(343.0);
        let changed = resolve_weapon_labels_with_aliases(
            &profiles,
            &[],
            Some(&label("155 mm M185")),
            Some(&label("M107 Medium Charge")),
        )
        .unwrap();
        assert_eq!(changed.status, ResolutionStatus::Ambiguous);
    }

    #[test]
    fn aliases_without_catalog_vehicle_and_unknown_labels_never_invent_profiles() {
        let profiles = crate::all_profiles();
        for (vehicle, ammo) in [
            ("73 mm 2A28", Some("PG-9")),
            ("73 mm 2A28", None),
            ("155 mm M185", None),
        ] {
            let ammo = ammo.map(label);
            let result =
                resolve_weapon_labels(&profiles, Some(&label(vehicle)), ammo.as_ref()).unwrap();
            assert_eq!(result.status, ResolutionStatus::NoMatch, "{vehicle}");
        }
        let ub32 =
            resolve_weapon_labels(&profiles, Some(&label("UB-32")), Some(&label("S-5K1"))).unwrap();
        assert_eq!(ub32.profile_id.as_deref(), Some("community-ub32-1"));
        let names: Vec<String> = profiles
            .iter()
            .map(|p| normalize_weapon_label(&profile_identity(p).vehicle))
            .collect();
        for alias in vehicle_aliases() {
            assert_eq!(alias.status, "experimental");
            assert!(alias.source.contains("zaobserwowane w panelu gry"));
            for vehicle in &alias.vehicles {
                let known = names.contains(&normalize_weapon_label(vehicle));
                assert_eq!(known, vehicle != "SPG-9", "{vehicle}");
            }
        }
    }
}
