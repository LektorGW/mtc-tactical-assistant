use crate::{
    ocr_layout, parse_numeric, OcrKind, OcrLayout, OcrLineBox, OcrRegionConfig, OcrWordBox, Roi,
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HudDiscoveryStatus {
    Ready,
    Partial,
    Ambiguous,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HudRegionProposal {
    pub region: OcrRegionConfig,
    pub confidence: f64,
    pub text: String,
    pub value: Option<f64>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HudDiscovery {
    pub schema_version: u32,
    pub status: HudDiscoveryStatus,
    pub timestamp_ms: u64,
    pub reference_width: u32,
    pub reference_height: u32,
    pub regions: Vec<HudRegionProposal>,
    pub warnings: Vec<String>,
    pub elapsed_ms: f64,
    pub source: String,
}

/// Full-frame OCR proposes regions, never weapon parameters or world coordinates.
/// Automatic consumers must confirm fresh agreement across at least two frames.
pub fn discover_hud(
    image: &RgbaImage,
    timestamp_ms: u64,
    now_ms: u64,
) -> Result<HudDiscovery, String> {
    let started = Instant::now();
    if timestamp_ms > now_ms || now_ms - timestamp_ms > 2000 {
        return Ok(blocked_discovery(
            image.width(),
            image.height(),
            timestamp_ms,
            "Klatka HUD jest przeterminowana lub ma przyszły znacznik czasu",
        ));
    }
    let layout = ocr_layout(image)?;
    let completed_now =
        now_ms.saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
    let mut discovery = discovery_from_layout(&layout, timestamp_ms, completed_now);
    discovery.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(discovery)
}

fn blocked_discovery(width: u32, height: u32, timestamp_ms: u64, warning: &str) -> HudDiscovery {
    HudDiscovery {
        schema_version: 1,
        status: HudDiscoveryStatus::Blocked,
        timestamp_ms,
        reference_width: width,
        reference_height: height,
        regions: Vec::new(),
        warnings: vec![warning.into()],
        elapsed_ms: 0.0,
        source: "Automatyczna lokalizacja HUD z geometrii jawnego tekstu OCR".into(),
    }
}

/// Deterministic discovery policy over visible OCR words and their source ROIs.
/// One frame never receives >=85% heuristic confidence. Missing evidence means
/// no proposal; repeated candidates of one kind are explicitly ambiguous.
pub fn discovery_from_layout(layout: &OcrLayout, timestamp_ms: u64, now_ms: u64) -> HudDiscovery {
    let mut output = blocked_discovery(
        layout.source_width,
        layout.source_height,
        timestamp_ms,
        "Brak jednoznacznych dowodów HUD; zachowaj ręczne ROI",
    );
    if let Err(error) = layout.validate() {
        output.warnings = vec![error];
        return output;
    }
    if timestamp_ms > now_ms || now_ms - timestamp_ms > 2000 {
        output.warnings =
            vec!["Klatka HUD jest przeterminowana lub ma przyszły znacznik czasu".into()];
        return output;
    }
    if layout.source_width < 160
        || layout.source_height < 90
        || layout
            .text_angle_deg
            .is_some_and(|angle| angle.abs() > 10.0)
    {
        output.warnings = vec![
            "Nieprawidłowy rozmiar lub silny obrót obrazu; discovery wymaga czytelnej klatki HUD"
                .into(),
        ];
        return output;
    }
    output.warnings.clear();
    output.elapsed_ms = layout.elapsed_ms;
    let mut candidates = Vec::new();
    let mut ambiguous = false;
    for line in &layout.lines {
        if selector_line(line) {
            continue;
        }
        // An unlabeled timer must be the entire short line in the central top
        // HUD. A clock in chat or several times on a line is not a match timer.
        let compact = line
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>();
        let center = center_x(line.roi);
        if strict_timer(&compact)
            && (0.2..=0.8).contains(&center)
            && line.roi.y < 0.4
            && line.roi.width <= 0.25
        {
            candidates.push(proposal(
                layout,
                OcrKind::Timer,
                line.roi,
                compact,
                0.72,
                vec!["Jedna wartość MM:SS w krótkim wierszu centralnej górnej części HUD".into()],
            ));
        }
        for (index, word) in line.words.iter().enumerate() {
            let (label, suffix) = word
                .text
                .split_once(':')
                .map_or((word.text.as_str(), None), |(label, value)| {
                    (label, Some(value))
                });
            let Some(kind) = label_kind(label) else {
                continue;
            };
            // Abbreviations on a photographed keyboard are not telemetry.
            if ["alt", "spd", "hdg"].contains(&normalize_label(label).as_str())
                && word.roi.y > 0.8
                && !word.text.contains(':')
            {
                continue;
            }
            let mut values: Vec<OcrWordBox> = Vec::new();
            if let Some(suffix) = suffix.filter(|text| !text.trim().is_empty()) {
                values.push(OcrWordBox {
                    text: suffix.into(),
                    roi: word.roi,
                });
            }
            values.extend(
                line.words
                    .iter()
                    .skip(index + 1)
                    .take_while(|next| {
                        label_kind(next.text.split(':').next().unwrap_or("")).is_none()
                    })
                    .cloned(),
            );
            if values.is_empty() {
                match nearby_value_line(layout, line, word) {
                    Ok(nearby) => values = nearby,
                    Err(()) => {
                        ambiguous = true;
                        output.warnings.push(format!(
                            "Kilka równie bliskich wartości przy etykiecie {} — wskaż ROI ręcznie",
                            word.text
                        ));
                        continue;
                    }
                }
            }
            if matches!(
                kind,
                OcrKind::VehicleName | OcrKind::WeaponName | OcrKind::MapName
            ) {
                if suffix.is_some_and(|text| !text.trim().is_empty()) {
                    output.warnings.push(format!("Etykieta i nazwa {} dzielą jedno BoundingRect — wskaż samą wartość ręcznie",kind.as_str()));
                    continue;
                }
                if values.first().is_some_and(|value| {
                    normalize_label(&value.text) == "name"
                        || normalize_label(&value.text) == "nazwa"
                }) {
                    values.remove(0);
                }
                let text = values
                    .iter()
                    .map(|value| value.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                if text.is_empty()
                    || text.len() > 256
                    || !text.chars().any(char::is_alphabetic)
                    || text.contains(':')
                    || selector_text(&text)
                {
                    continue;
                }
                if let Some(roi) =
                    crate::ocr_layout::union_rois(values.iter().map(|value| value.roi))
                {
                    candidates.push(proposal(
                        layout,
                        kind,
                        roi,
                        text,
                        0.76,
                        vec![
                            format!("Jawna etykieta UI: {}", word.text),
                            "Nazwa pochodzi wyłącznie z tekstu przypisanego do etykiety".into(),
                        ],
                    ));
                }
            } else {
                let text = values
                    .iter()
                    .map(|value| value.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                let numeric: Vec<_> = values
                    .iter()
                    .filter(|value| strict_numeric(&value.text, kind))
                    .collect();
                if numeric.len() > 1
                    || (kind == OcrKind::Timer
                        && values
                            .iter()
                            .filter(|value| strict_timer(&value.text))
                            .count()
                            > 1)
                {
                    ambiguous = true;
                    output.warnings.push(format!(
                        "Etykieta {} ma kilka wartości — odczyt niejednoznaczny",
                        word.text
                    ));
                    continue;
                }
                let parsed = if kind == OcrKind::Timer {
                    if strict_timer(&text.replace(' ', "")) {
                        parse_numeric("timer", &text.replace(' ', ""), timestamp_ms)
                    } else {
                        None
                    }
                } else {
                    parse_numeric(kind.as_str(), &text, timestamp_ms)
                };
                if let Some(observation) = parsed {
                    if kind != OcrKind::Timer && numeric.len() != 1 {
                        continue;
                    }
                    let roi = if kind == OcrKind::Timer {
                        crate::ocr_layout::union_rois(values.iter().map(|value| value.roi))
                    } else {
                        Some(numeric[0].roi)
                    };
                    if let Some(roi) = roi {
                        candidates.push(proposal(
                            layout,
                            kind,
                            roi,
                            text,
                            0.78,
                            vec![
                                format!("Jawna etykieta UI: {}", word.text),
                                "Dokładnie jedna poprawna wartość przypisana do etykiety".into(),
                            ],
                        ));
                        candidates.last_mut().unwrap().value = Some(observation.value);
                    }
                }
            }
        }
    }
    let timer_center = candidates
        .iter()
        .find(|candidate| candidate.region.kind == OcrKind::Timer)
        .map_or(0.5, |candidate| center_x(candidate.region.roi));
    let compass = compass_candidates(layout, timer_center, &candidates);
    candidates.extend(compass);
    let kinds = [
        OcrKind::Timer,
        OcrKind::Heading,
        OcrKind::Speed,
        OcrKind::Altitude,
        OcrKind::VehicleName,
        OcrKind::WeaponName,
        OcrKind::MapName,
    ];
    for kind in kinds {
        let mut selected: Vec<HudRegionProposal> = Vec::new();
        for candidate in candidates
            .iter()
            .filter(|candidate| candidate.region.kind == kind)
        {
            if let Some(existing) = selected
                .iter_mut()
                .find(|existing| same_region(existing, candidate))
            {
                if candidate.confidence > existing.confidence {
                    *existing = candidate.clone();
                }
            } else {
                selected.push(candidate.clone());
            }
        }
        if selected.len() == 1 {
            output.regions.push(selected.remove(0));
        } else if selected.len() > 1 {
            ambiguous = true;
            output.warnings.push(format!(
                "Wiele kandydatów {} — wymagane ręczne wskazanie właściwego obszaru",
                kind.as_str()
            ));
        }
    }
    if output.regions.len() > 16 {
        output.regions.clear();
        output
            .warnings
            .push("Przekroczono limit 16 obszarów HUD".into());
        return output;
    }
    output.status = if ambiguous {
        HudDiscoveryStatus::Ambiguous
    } else if output.regions.len() >= 2 {
        HudDiscoveryStatus::Ready
    } else if output.regions.len() == 1 {
        HudDiscoveryStatus::Partial
    } else {
        HudDiscoveryStatus::Blocked
    };
    if !output
        .regions
        .iter()
        .any(|region| region.region.kind == OcrKind::Heading)
    {
        output.warnings.push("Brak jawnej etykiety kierunku lub wyróżnionego odczytu przy skali kompasu; nie wybieramy dowolnej liczby".into());
    }
    if output.regions.is_empty() && !ambiguous {
        output
            .warnings
            .push("Nie znaleziono jednoznacznych danych HUD; ręczne ROI pozostają dostępne".into());
    }
    output
}

fn proposal(
    layout: &OcrLayout,
    kind: OcrKind,
    roi: Roi,
    text: String,
    confidence: f64,
    evidence: Vec<String>,
) -> HudRegionProposal {
    let dx = 2.0 / f64::from(layout.source_width);
    let dy = 2.0 / f64::from(layout.source_height);
    let x = (roi.x - dx).max(0.0);
    let y = (roi.y - dy).max(0.0);
    let roi = Roi {
        x,
        y,
        width: (roi.x + roi.width + dx).min(1.0) - x,
        height: (roi.y + roi.height + dy).min(1.0) - y,
    };
    let value = parse_numeric(kind.as_str(), &text, 0).map(|observation| observation.value);
    HudRegionProposal {
        region: OcrRegionConfig {
            id: format!("auto_{}", kind.as_str()),
            kind,
            roi,
            upscale: if roi.height * f64::from(layout.source_height) < 22.0 {
                3
            } else {
                2
            },
        },
        confidence,
        text,
        value,
        evidence,
    }
}

fn normalize_label(text: &str) -> String {
    text.trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase()
}

fn label_kind(text: &str) -> Option<OcrKind> {
    match normalize_label(text).as_str() {
        "time" | "timer" | "czas" | "remaining" => Some(OcrKind::Timer),
        "heading" | "hdg" | "azimuth" | "bearing" | "azymut" | "kompas" | "kierunek" => {
            Some(OcrKind::Heading)
        }
        "speed" | "spd" | "velocity" | "airspeed" | "groundspeed" | "prędkość" | "predkosc" => {
            Some(OcrKind::Speed)
        }
        "altitude" | "alt" | "height" | "wysokość" | "wysokosc" => Some(OcrKind::Altitude),
        "vehicle" | "pojazd" => Some(OcrKind::VehicleName),
        "weapon" | "uzbrojenie" | "broń" | "bron" | "ammo" | "ammunition" | "projectile"
        | "amunicja" | "pocisk" => Some(OcrKind::WeaponName),
        "map" | "mapa" => Some(OcrKind::MapName),
        _ => None,
    }
}

fn selector_text(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        matches!(
            normalize_label(word).as_str(),
            "choose" | "select" | "wybierz" | "menu" | "spawn"
        )
    })
}
fn selector_line(line: &OcrLineBox) -> bool {
    selector_text(&line.text)
}

fn strict_timer(text: &str) -> bool {
    let Some((minutes, seconds)) = text.trim().split_once(':') else {
        return false;
    };
    (1..=2).contains(&minutes.len())
        && seconds.len() == 2
        && minutes.bytes().all(|c| c.is_ascii_digit())
        && seconds.bytes().all(|c| c.is_ascii_digit())
        && seconds.parse::<u32>().is_ok_and(|v| v < 60)
}

fn strict_numeric(text: &str, kind: OcrKind) -> bool {
    let text = text.trim();
    if kind == OcrKind::Heading {
        let text = text.trim_end_matches('°');
        return !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
            && parse_numeric("heading", text, 0).is_some();
    }
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NUMBER.get_or_init(|| {
        regex::Regex::new(r"(?i)^[-+]?[0-9]+(?:[.,][0-9]+)?(?:m|ft|km/h|kph|mph|kt|kn|m/s)?$")
            .unwrap()
    });
    re.is_match(text)
}

fn nearby_value_line(
    layout: &OcrLayout,
    label_line: &OcrLineBox,
    label: &OcrWordBox,
) -> Result<Vec<OcrWordBox>, ()> {
    let mut nearby: Vec<_> = layout
        .lines
        .iter()
        .filter(|line| {
            !std::ptr::eq(*line, label_line)
                && !selector_line(line)
                && !line
                    .words
                    .iter()
                    .any(|word| label_kind(word.text.split(':').next().unwrap_or("")).is_some())
        })
        .filter_map(|line| {
            let same_band = (center_y(line.roi) - center_y(label.roi)).abs()
                <= line.roi.height.max(label.roi.height) * 0.65
                && line.roi.x >= label.roi.x + label.roi.width
                && line.roi.x - label.roi.x - label.roi.width <= 0.18;
            let below = line.roi.y >= label.roi.y + label.roi.height
                && line.roi.y - label.roi.y - label.roi.height <= label.roi.height * 1.8
                && (line.roi.x - label.roi.x).abs() <= 0.04;
            if same_band || below {
                Some((
                    line,
                    (line.roi.x - label.roi.x - label.roi.width).abs()
                        + (line.roi.y - label.roi.y).abs(),
                ))
            } else {
                None
            }
        })
        .collect();
    nearby.sort_by(|a, b| a.1.total_cmp(&b.1));
    if nearby.len() > 1 && (nearby[1].1 - nearby[0].1).abs() < label.roi.height * 0.3 {
        return Err(());
    }
    Ok(nearby
        .first()
        .map(|(line, _)| line.words.clone())
        .unwrap_or_default())
}

fn compass_candidates(
    layout: &OcrLayout,
    anchor_x: f64,
    labeled: &[HudRegionProposal],
) -> Vec<HudRegionProposal> {
    let words: Vec<_> = layout
        .lines
        .iter()
        .filter(|line| !selector_line(line))
        .filter(|line| {
            !line
                .words
                .iter()
                .any(|word| label_kind(word.text.split(':').next().unwrap_or("")).is_some())
        })
        .flat_map(|line| line.words.iter())
        .filter_map(|word| {
            if labeled
                .iter()
                .filter(|proposal| proposal.region.kind != OcrKind::Heading)
                .any(|proposal| {
                    let roi = proposal.region.roi;
                    center_x(word.roi) >= roi.x
                        && center_x(word.roi) <= roi.x + roi.width
                        && center_y(word.roi) >= roi.y
                        && center_y(word.roi) <= roi.y + roi.height
                })
            {
                return None;
            }
            let text = word.text.trim_end_matches('°');
            if word.roi.y > 0.4
                || text.is_empty()
                || text.len() > 3
                || !text.bytes().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            let value = text.parse::<f64>().ok()?;
            if !(0.0..360.0).contains(&value) {
                return None;
            }
            Some((word, value))
        })
        .collect();
    let mut result = Vec::new();
    for (word, value) in &words {
        let center = center_x(word.roi);
        if (center - anchor_x).abs() > 0.13 {
            continue;
        }
        let neighbors: Vec<_> = words
            .iter()
            .filter(|(other, _)| {
                !std::ptr::eq(*other, *word)
                    && (center_y(other.roi) - center_y(word.roi)).abs() <= word.roi.height * 2.0
                    && other.roi.height <= word.roi.height / 1.35
            })
            .collect();
        let left = neighbors
            .iter()
            .filter(|(other, _)| center_x(other.roi) < center)
            .max_by(|a, b| center_x(a.0.roi).total_cmp(&center_x(b.0.roi)));
        let right = neighbors
            .iter()
            .filter(|(other, _)| center_x(other.roi) > center)
            .min_by(|a, b| center_x(a.0.roi).total_cmp(&center_x(b.0.roi)));
        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };
        let span = center_x(right.0.roi) - center_x(left.0.roi);
        let relative = (center - center_x(left.0.roi)) / span;
        let arc = (right.1 - left.1).rem_euclid(360.0);
        if span < 0.15
            || !(0.3..=0.7).contains(&relative)
            || arc <= 0.0
            || arc > 180.0
            || (*value - left.1).rem_euclid(360.0) > arc
        {
            continue;
        }
        result.push(proposal(
            layout,
            OcrKind::Heading,
            word.roi,
            word.text.clone(),
            0.75,
            vec![
                "Odczyt centralny jest co najmniej 1,35× wyższy niż sąsiednie oznaczenia skali"
                    .into(),
                format!(
                    "Wartość {} leży między widocznymi oznaczeniami {} i {} w górnej skali",
                    value, left.1, right.1
                ),
            ],
        ));
    }
    result
}

fn same_region(a: &HudRegionProposal, b: &HudRegionProposal) -> bool {
    if a.region.kind != b.region.kind
        || a.value != b.value
        || (a.value.is_none() && a.text != b.text)
    {
        return false;
    }
    let a = a.region.roi;
    let b = b.region.roi;
    let width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let intersection = width * height;
    intersection / (a.width * a.height + b.width * b.height - intersection) > 0.5
}
fn center_x(roi: Roi) -> f64 {
    roi.x + roi.width / 2.0
}
fn center_y(roi: Roi) -> f64 {
    roi.y + roi.height / 2.0
}
