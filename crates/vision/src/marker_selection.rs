use crate::{MarkerObservation, MarkerRole};
use mtc_maps::Point;
use serde::{Deserialize, Serialize};

/// The role and point are explicitly confirmed by the user. The consumer binds
/// scope to window, capture epoch, map, calibration and color/shape settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSelectionHint {
    pub selection_id: String,
    pub signature_id: String,
    pub role: MarkerRole,
    pub confirmed_point: Point,
    pub timestamp_ms: u64,
    pub scope: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub map_width: u32,
    pub map_height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSelectionConfig {
    pub scope: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub max_age_ms: u64,
    pub initial_radius_pixels: f64,
    pub base_gate_pixels: f64,
    pub max_association_radius_pixels: f64,
    /// Explicit rejection limit in reference-map pixels per second.
    pub max_speed_pixels_per_second: f64,
    pub ambiguity_margin_pixels: f64,
    pub min_confidence: f64,
}

impl MarkerSelectionConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.scope.trim().is_empty()
            || self.scope.len() > 16_384
            || self.frame_width == 0
            || self.frame_height == 0
            || self.map_width == 0
            || self.map_height == 0
            || u64::from(self.frame_width) * u64::from(self.frame_height) > 64_000_000
            || u64::from(self.map_width) * u64::from(self.map_height) > 64_000_000
            || self.max_age_ms == 0
            || self.max_age_ms > 2000
            || !self.initial_radius_pixels.is_finite()
            || self.initial_radius_pixels <= 0.0
            || self.initial_radius_pixels > 256.0
            || !self.base_gate_pixels.is_finite()
            || self.base_gate_pixels <= 0.0
            || !self.max_association_radius_pixels.is_finite()
            || self.max_association_radius_pixels < self.base_gate_pixels
            || self.max_association_radius_pixels > 256.0
            || !self.max_speed_pixels_per_second.is_finite()
            || self.max_speed_pixels_per_second <= 0.0
            || self.max_speed_pixels_per_second > 1_000_000.0
            || !self.ambiguity_margin_pixels.is_finite()
            || self.ambiguity_margin_pixels < 0.0
            || self.ambiguity_margin_pixels > self.max_association_radius_pixels
            || !self.min_confidence.is_finite()
            || !(0.6..=1.0).contains(&self.min_confidence)
        {
            return Err("Niepoprawna geometria, zakres lub limity wyboru markera".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSelectionState {
    pub hint: MarkerSelectionHint,
    /// Actual observed candidates only; predicted points never become samples.
    pub last: MarkerObservation,
    pub previous: Option<MarkerObservation>,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerSelectionStatus {
    Collecting,
    Ready,
    Ambiguous,
    Stale,
    Blocked,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerSelectionResult {
    pub status: MarkerSelectionStatus,
    pub selected: Option<MarkerObservation>,
    pub track_id: Option<String>,
    pub count: u32,
    pub confidence: f64,
    pub timestamp_ms: u64,
    pub state: Option<MarkerSelectionState>,
    pub warning: Option<String>,
}

fn unavailable(
    status: MarkerSelectionStatus,
    timestamp_ms: u64,
    warning: &str,
) -> MarkerSelectionResult {
    MarkerSelectionResult {
        status,
        selected: None,
        track_id: None,
        count: 0,
        confidence: 0.0,
        timestamp_ms,
        state: None,
        warning: Some(warning.into()),
    }
}

fn same_hint(a: &MarkerSelectionHint, b: &MarkerSelectionHint) -> bool {
    a.selection_id == b.selection_id
        && a.signature_id == b.signature_id
        && a.role == b.role
        && a.scope == b.scope
        && a.timestamp_ms == b.timestamp_ms
        && a.confirmed_point == b.confirmed_point
        && a.frame_width == b.frame_width
        && a.frame_height == b.frame_height
        && a.map_width == b.map_width
        && a.map_height == b.map_height
}

fn valid_point(point: Point, config: &MarkerSelectionConfig) -> bool {
    point.valid()
        && point.x >= 0.0
        && point.y >= 0.0
        && point.x < f64::from(config.map_width)
        && point.y < f64::from(config.map_height)
}

fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Associate an explicit click with visible candidates. There is no selection
/// without a hint and no fallback to the first candidate on ties or missing data.
pub fn select_confirmed_marker(
    candidates: &[MarkerObservation],
    hint: &MarkerSelectionHint,
    previous_state: Option<&MarkerSelectionState>,
    timestamp_ms: u64,
    now_ms: u64,
    config: &MarkerSelectionConfig,
) -> Result<MarkerSelectionResult, String> {
    config.validate()?;
    if hint.selection_id.trim().is_empty()
        || hint.selection_id.len() > 80
        || hint.signature_id.trim().is_empty()
        || hint.signature_id.len() > 80
        || hint.role == MarkerRole::Unassigned
        || !valid_point(hint.confirmed_point, config)
    {
        return Err(
            "Wybór wymaga jawnego identyfikatora, roli i potwierdzonego punktu na mapie".into(),
        );
    }
    let blocked = |status, reason| unavailable(status, timestamp_ms, reason);
    if hint.scope != config.scope
        || hint.frame_width != config.frame_width
        || hint.frame_height != config.frame_height
        || hint.map_width != config.map_width
        || hint.map_height != config.map_height
    {
        return Ok(blocked(
            MarkerSelectionStatus::Blocked,
            "Zmieniło się źródło, mapa lub geometria; wybierz marker ponownie",
        ));
    }
    if timestamp_ms > now_ms
        || hint.timestamp_ms > timestamp_ms
        || now_ms - timestamp_ms > config.max_age_ms
    {
        return Ok(blocked(
            MarkerSelectionStatus::Stale,
            "Klatka lub ręczne potwierdzenie ma niepoprawny/przeterminowany czas",
        ));
    }
    if candidates.len() > 512 {
        return Ok(blocked(
            MarkerSelectionStatus::Blocked,
            "Zbyt wiele widocznych kandydatów; wybór wstrzymany",
        ));
    }
    let state = previous_state.filter(|s| same_hint(&s.hint, hint));
    let (predicted, gate) = if let Some(s) = state {
        if !valid_point(s.last.map_position, config)
            || s.last.signature_id != hint.signature_id
            || !s.last.confidence.is_finite()
            || s.last.confidence < config.min_confidence
            || s.last.confidence > 1.0
            || s.count == 0
            || s.count > 64
            || s.last.timestamp_ms < hint.timestamp_ms
            || s.last.timestamp_ms > timestamp_ms
        {
            return Ok(blocked(
                MarkerSelectionStatus::Rejected,
                "Niepoprawny lub zmieniony stan śledzenia markera",
            ));
        }
        if now_ms - s.last.timestamp_ms > config.max_age_ms {
            return Ok(blocked(
                MarkerSelectionStatus::Stale,
                "Utracono świeżą obserwację; wybierz widoczną ikonę ponownie",
            ));
        }
        if s.last.timestamp_ms == timestamp_ms {
            let mut result = blocked(
                MarkerSelectionStatus::Blocked,
                "Ta sama klatka nie jest drugim potwierdzeniem markera",
            );
            result.count = s.count;
            result.state = Some(s.clone());
            return Ok(result);
        }
        let dt = (timestamp_ms - s.last.timestamp_ms) as f64 / 1000.0;
        let mut predicted = s.last.map_position;
        if let Some(prior) = &s.previous {
            if prior.timestamp_ms >= s.last.timestamp_ms
                || prior.timestamp_ms < hint.timestamp_ms
                || !valid_point(prior.map_position, config)
                || prior.signature_id != hint.signature_id
                || !prior.confidence.is_finite()
                || prior.confidence < config.min_confidence
                || prior.confidence > 1.0
            {
                return Ok(blocked(
                    MarkerSelectionStatus::Rejected,
                    "Historia markera ma niespójne czasy lub tożsamość",
                ));
            }
            if now_ms - prior.timestamp_ms <= config.max_age_ms {
                let interval = (s.last.timestamp_ms - prior.timestamp_ms) as f64 / 1000.0;
                let vx = (s.last.map_position.x - prior.map_position.x) / interval;
                let vy = (s.last.map_position.y - prior.map_position.y) / interval;
                if vx.hypot(vy) > config.max_speed_pixels_per_second {
                    return Ok(blocked(
                        MarkerSelectionStatus::Rejected,
                        "Historia zawiera skok przekraczający limit ruchu",
                    ));
                }
                predicted.x += vx * dt;
                predicted.y += vy * dt;
            }
        }
        if !valid_point(predicted, config) {
            return Ok(blocked(
                MarkerSelectionStatus::Rejected,
                "Predykcja wyboru wychodzi poza mapę; nie zwracamy ukrytej pozycji",
            ));
        }
        (
            predicted,
            (config.base_gate_pixels + config.max_speed_pixels_per_second * dt)
                .min(config.max_association_radius_pixels),
        )
    } else {
        if now_ms - hint.timestamp_ms > config.max_age_ms {
            return Ok(blocked(
                MarkerSelectionStatus::Stale,
                "Ręcznie wskazany punkt stracił świeżość; potwierdź go na nowej klatce",
            ));
        }
        (hint.confirmed_point, config.initial_radius_pixels)
    };
    let relevant: Vec<_> = candidates
        .iter()
        .filter(|m| {
            m.signature_id == hint.signature_id
                && (m.role == MarkerRole::Unassigned || m.role == hint.role)
        })
        .collect();
    if relevant.iter().any(|m| {
        !valid_point(m.map_position, config)
            || m.pixel_count == 0
            || m.timestamp_ms != timestamp_ms
            || !m.confidence.is_finite()
            || !(0.0..=1.0).contains(&m.confidence)
            || m.bounding_box.validate().is_err()
            || !m.frame_position_normalized.valid()
            || m.frame_position_normalized.x < 0.0
            || m.frame_position_normalized.x >= 1.0
            || m.frame_position_normalized.y < 0.0
            || m.frame_position_normalized.y >= 1.0
    }) {
        return Ok(blocked(
            MarkerSelectionStatus::Rejected,
            "Kandydaci nie pochodzą z tej samej świeżej klatki lub mają błędną geometrię",
        ));
    }
    let mut eligible: Vec<_> = relevant
        .into_iter()
        .filter(|m| m.confidence >= config.min_confidence)
        .filter_map(|m| {
            let dist = distance(m.map_position, predicted);
            let speed_ok = state.is_none_or(|s| {
                distance(m.map_position, s.last.map_position)
                    <= config.max_speed_pixels_per_second
                        * ((timestamp_ms - s.last.timestamp_ms) as f64 / 1000.0)
            });
            (dist <= gate && speed_ok).then_some((dist, m))
        })
        .collect();
    eligible.sort_by(|a, b| a.0.total_cmp(&b.0));
    if eligible.is_empty() {
        return Ok(blocked(
            MarkerSelectionStatus::Rejected,
            "Brak widocznego jednoznacznego markera w bramce ruchu; nie wybieramy innej ikony",
        ));
    }
    if eligible.len() > 1 && eligible[1].0 - eligible[0].0 <= config.ambiguity_margin_pixels {
        return Ok(blocked(
            MarkerSelectionStatus::Ambiguous,
            "Kilka ikon równie dobrze pasuje do ręcznego wyboru; potwierdź konkretną ikonę",
        ));
    }
    let observed = eligible[0].1;
    let count = state.map_or(1, |s| s.count.saturating_add(1).min(64));
    let mut selected = observed.clone();
    selected.role = hint.role;
    let status = if count >= 2 {
        MarkerSelectionStatus::Ready
    } else {
        MarkerSelectionStatus::Collecting
    };
    if count >= 2 {
        selected.confidence = 0.9;
    }
    selected.source = "Widoczna ikona wskazana ręcznie; lokalna bramka położenia i ruchu".into();
    let next = MarkerSelectionState {
        hint: hint.clone(),
        last: observed.clone(),
        previous: state.map(|s| s.last.clone()),
        count,
    };
    Ok(MarkerSelectionResult { status, confidence: selected.confidence, selected: Some(selected),
        track_id: Some(hint.selection_id.clone()), count, timestamp_ms, state: Some(next),
        warning: Some("Tożsamość z jawnego wskazania i bliskości; krzyżowanie podobnych ikon wymaga ponownego potwierdzenia".into()) })
}
