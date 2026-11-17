//! Reader for the MTC "Choose Weapon" loadout screen: the "Specifications"
//! panel (Damage, Type, Muzzle Velocity, Penetration, Projectile Mass) and the
//! slot from the title ("Choose Weapon - Primary"). Values are read from the
//! visible UI only; the panel itself does not name the weapon it describes.

use crate::{ocr_layout, preprocess::upscale_for_ocr, OcrLayout, Roi};
use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecValue {
    pub value: f64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadoutSpecsStatus {
    /// Muzzle velocity read with its unit.
    Ready,
    /// Specifications panel found, muzzle velocity missing or unreadable.
    Partial,
    /// No specifications panel visible.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadoutSpecs {
    pub schema_version: u32,
    pub status: LoadoutSpecsStatus,
    pub timestamp_ms: u64,
    /// "Primary", "Secondary", "Tertiary"... from the screen title, if visible.
    pub slot: Option<String>,
    /// Never inferred: the specifications panel does not show the weapon name.
    pub weapon_name: Option<String>,
    pub damage: Option<SpecValue>,
    pub projectile_type: Option<String>,
    pub muzzle_velocity_mps: Option<SpecValue>,
    pub penetration_mm: Option<SpecValue>,
    pub projectile_mass_kg: Option<SpecValue>,
    /// Heuristic for the whole reading: 0.7 when every field parsed, lower otherwise.
    pub confidence: f64,
    pub source: String,
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
}

fn number_with_unit(text: &str, units: &[&str]) -> Option<SpecValue> {
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NUMBER.get_or_init(|| {
        regex::Regex::new(r"([0-9]+(?:[.,][0-9]+)?)\s*([A-Za-z/]+)?").expect("Valid regex")
    });
    let captures: Vec<_> = re.captures_iter(text).collect();
    if captures.len() != 1 {
        return None;
    }
    let unit = captures[0].get(2).map(|m| m.as_str().to_ascii_lowercase());
    if !units.is_empty() && !unit.as_deref().is_some_and(|u| units.contains(&u)) {
        return None;
    }
    Some(SpecValue {
        value: captures[0][1].replace(',', ".").parse().ok()?,
        text: text.trim().into(),
    })
}

fn label_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let lower = line.to_lowercase();
    let start = lower.find(label)?;
    let rest = &line[start + label.len()..];
    Some(rest.trim_start_matches([':', ';', '.', ' ']).trim())
}

/// Pure parser over OCR lines (deterministic tests).
pub fn parse_loadout_lines(lines: &[String], timestamp_ms: u64) -> LoadoutSpecs {
    let mut specs = LoadoutSpecs {
        schema_version: 1,
        status: LoadoutSpecsStatus::Blocked,
        timestamp_ms,
        slot: None,
        weapon_name: None,
        damage: None,
        projectile_type: None,
        muzzle_velocity_mps: None,
        penetration_mm: None,
        projectile_mass_kg: None,
        confidence: 0.0,
        source: "Odczytane z UI gry (ekran Choose Weapon / Specifications), OCR Windows".into(),
        lines: lines.to_vec(),
        warnings: Vec::new(),
    };
    let found = lines
        .iter()
        .any(|l| l.to_lowercase().contains("specification"));
    for line in lines {
        let lower = line.to_lowercase();
        if let Some(rest) = label_value(line, "choose weapon") {
            let slot = rest.trim_start_matches(['-', '–', ' ']).trim();
            if !slot.is_empty() && slot.chars().all(|c| c.is_alphabetic() || c == ' ') {
                specs.slot = Some(slot.into());
            }
        }
        if lower.starts_with("damage") {
            specs.damage = label_value(line, "damage").and_then(|v| number_with_unit(v, &[]));
        } else if lower.starts_with("type") {
            specs.projectile_type = label_value(line, "type")
                .filter(|v| !v.is_empty() && v.len() <= 32)
                .map(str::to_string);
        } else if lower.starts_with("muzzle velocity") {
            specs.muzzle_velocity_mps =
                label_value(line, "muzzle velocity").and_then(|v| number_with_unit(v, &["m/s"]));
        } else if lower.starts_with("penetration") {
            specs.penetration_mm =
                label_value(line, "penetration").and_then(|v| number_with_unit(v, &["mm"]));
        } else if lower.starts_with("projectile mass") {
            specs.projectile_mass_kg = label_value(line, "projectile mass")
                .and_then(|v| number_with_unit(v, &["kg", "g"]))
                .map(|mut v| {
                    if v.text.to_lowercase().trim_end().ends_with(" g") {
                        v.value /= 1000.0;
                    }
                    v
                });
        }
    }
    if !found {
        // Values without the panel heading may come from other UI; never report them.
        specs.damage = None;
        specs.projectile_type = None;
        specs.muzzle_velocity_mps = None;
        specs.penetration_mm = None;
        specs.projectile_mass_kg = None;
        specs
            .warnings
            .push("Nie widać panelu Specifications (ekran wyboru broni)".into());
        return specs;
    }
    let parsed = [
        specs.damage.is_some(),
        specs.projectile_type.is_some(),
        specs.muzzle_velocity_mps.is_some(),
        specs.penetration_mm.is_some(),
        specs.projectile_mass_kg.is_some(),
    ];
    let count = parsed.iter().filter(|p| **p).count();
    specs.confidence = 0.7 * count as f64 / parsed.len() as f64;
    specs.status = if specs.muzzle_velocity_mps.is_some() {
        LoadoutSpecsStatus::Ready
    } else {
        specs
            .warnings
            .push("Brak czytelnej prędkości wylotowej (Muzzle Velocity … m/s)".into());
        LoadoutSpecsStatus::Partial
    };
    if specs
        .projectile_mass_kg
        .as_ref()
        .is_some_and(|m| m.value == 0.0)
    {
        specs
            .warnings
            .push("Gra pokazuje masę pocisku 0.0 kg — wartość zaokrąglona lub nieustawiona".into());
    }
    specs
        .warnings
        .push("Panel nie podaje nazwy broni — przypisz odczyt do wybranej karty ręcznie".into());
    specs
}

fn layout_lines(layout: &OcrLayout) -> Vec<String> {
    let mut lines: Vec<_> = layout.lines.iter().collect();
    lines.sort_by(|a, b| {
        a.roi
            .y
            .total_cmp(&b.roi.y)
            .then(a.roi.x.total_cmp(&b.roi.x))
    });
    lines.iter().map(|l| l.text.clone()).collect()
}

/// Reads the loadout screen from a client frame or a crop of it.
pub fn read_loadout_specs(image: &RgbaImage, timestamp_ms: u64) -> Result<LoadoutSpecs, String> {
    read_loadout_specs_with(image, timestamp_ms, ocr_layout)
}

pub fn read_loadout_specs_with<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    mut layout: F,
) -> Result<LoadoutSpecs, String>
where
    F: FnMut(&RgbaImage) -> Result<OcrLayout, String>,
{
    // Whole image first; large noisy frames sometimes give no OCR lines, so the
    // right part (where the panel sits) is retried enlarged.
    let mut best = parse_loadout_lines(&layout_lines(&layout(image)?), timestamp_ms);
    if best.status == LoadoutSpecsStatus::Ready {
        return Ok(best);
    }
    let regions = [
        Roi {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        },
        Roi {
            x: 0.6,
            y: 0.2,
            width: 0.25,
            height: 0.45,
        },
    ];
    for roi in regions {
        let Ok(crop) = roi.crop(image) else { continue };
        if crop.width() < 200 {
            continue;
        }
        let Ok(enlarged) = upscale_for_ocr(&crop, 2) else {
            continue;
        };
        let candidate = parse_loadout_lines(&layout_lines(&layout(&enlarged)?), timestamp_ms);
        let slot = best.slot.clone();
        if candidate.confidence > best.confidence {
            best = candidate;
            best.slot = best.slot.clone().or(slot);
        }
        if best.status == LoadoutSpecsStatus::Ready {
            break;
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_all_specification_fields_with_units() {
        let s = parse_loadout_lines(
            &lines(&[
                "Choose Weapon - Primary",
                "Specifications",
                "Damage: 66",
                "Type: AP",
                "Muzzle Velocity: 825 m/s",
                "Penetration: 18 mm",
                "Projectile Mass: 0.0 kg",
            ]),
            5,
        );
        assert_eq!(s.status, LoadoutSpecsStatus::Ready);
        assert_eq!(s.slot.as_deref(), Some("Primary"));
        assert_eq!(s.muzzle_velocity_mps.unwrap().value, 825.0);
        assert_eq!(s.damage.unwrap().value, 66.0);
        assert_eq!(s.projectile_type.as_deref(), Some("AP"));
        assert_eq!(s.penetration_mm.unwrap().value, 18.0);
        assert_eq!(s.projectile_mass_kg.unwrap().value, 0.0);
        assert!(s.weapon_name.is_none());
        assert!((s.confidence - 0.7).abs() < 1e-9);
    }

    #[test]
    fn rejects_velocity_without_unit_or_with_two_numbers() {
        for bad in [
            "Muzzle Velocity: 825",
            "Muzzle Velocity: 825 m/s 900 m/s",
            "Muzzle Velocity: 82S km/h",
        ] {
            let s = parse_loadout_lines(&lines(&["Specifications", bad]), 1);
            assert!(s.muzzle_velocity_mps.is_none(), "{bad}");
            assert_eq!(s.status, LoadoutSpecsStatus::Partial);
        }
        let none = parse_loadout_lines(&lines(&["Muzzle Velocity: 825 m/s"]), 1);
        assert_eq!(
            none.status,
            LoadoutSpecsStatus::Blocked,
            "values without the panel heading"
        );
    }
}
