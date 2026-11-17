use mtc_maps::{MapCalibration, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionSample {
    /// Stable ID of the manually selected visible marker, not just its color.
    pub source_id: String,
    pub map_position: Point,
    pub timestamp_ms: u64,
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfig {
    pub map_width: u32,
    pub map_height: u32,
    pub max_age_ms: u64,
    pub min_delta_ms: u64,
    pub max_speed_units_per_second: f64,
    pub min_confidence: f64,
    pub max_samples: usize,
    pub max_prediction_seconds: f64,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            map_width: 800,
            map_height: 600,
            max_age_ms: 3000,
            min_delta_ms: 200,
            // This is a rejection threshold configured by the user, not a
            // measured MTC vehicle speed or a claim about game units.
            max_speed_units_per_second: 500.0,
            min_confidence: 0.6,
            max_samples: 8,
            max_prediction_seconds: 10.0,
        }
    }
}

impl TrackerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.map_width == 0
            || self.map_height == 0
            || self.max_age_ms == 0
            || self.max_age_ms > 60_000
            || self.min_delta_ms == 0
            || self.min_delta_ms > self.max_age_ms
            || !self.max_speed_units_per_second.is_finite()
            || self.max_speed_units_per_second <= 0.0
            || !self.min_confidence.is_finite()
            || !(0.0..=1.0).contains(&self.min_confidence)
            || !(2..=64).contains(&self.max_samples)
            || !self.max_prediction_seconds.is_finite()
            || !(0.0..=60.0).contains(&self.max_prediction_seconds)
        {
            return Err("Niepoprawna konfiguracja śledzenia widocznego markera".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MotionStatus {
    Collecting,
    Ready,
    Stale,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MotionEstimate {
    pub status: MotionStatus,
    /// East and north in the world units of the supplied calibration per second.
    pub velocity: Option<[f64; 2]>,
    pub speed_units_per_second: Option<f64>,
    pub position_world: Option<Point>,
    pub count: usize,
    pub confidence: f64,
    pub timestamp_ms: u64,
    pub max_prediction_seconds: f64,
    pub assumptions: Vec<String>,
    pub warning: Option<String>,
}

impl MotionEstimate {
    fn unavailable(
        status: MotionStatus,
        count: usize,
        timestamp_ms: u64,
        max_prediction_seconds: f64,
        warning: &str,
    ) -> Self {
        Self {
            status,
            velocity: None,
            speed_units_per_second: None,
            position_world: None,
            count,
            confidence: 0.0,
            timestamp_ms,
            max_prediction_seconds,
            assumptions: Vec::new(),
            warning: Some(warning.into()),
        }
    }
}

/// Fit a constant velocity to fresh calibrated positions. Invalid or ambiguous
/// input produces an unavailable estimate, never an invented movement vector.
/// Callers reset samples whenever map, calibration, capture window or selection changes.
pub fn estimate_motion(
    samples: &[PositionSample],
    calibration: &MapCalibration,
    now_ms: u64,
    config: &TrackerConfig,
) -> Result<MotionEstimate, String> {
    config.validate()?;
    calibration.validate()?;
    let timestamp = samples.last().map_or(0, |sample| sample.timestamp_ms);
    let unavailable = |status, count, warning: &str| {
        MotionEstimate::unavailable(
            status,
            count,
            timestamp,
            config.max_prediction_seconds,
            warning,
        )
    };
    if samples.is_empty() {
        return Ok(unavailable(
            MotionStatus::Collecting,
            0,
            "Wymagane co najmniej dwie świeże obserwacje wybranego markera",
        ));
    }
    if samples.iter().any(|sample| sample.timestamp_ms > now_ms) {
        return Ok(unavailable(
            MotionStatus::Rejected,
            0,
            "Obserwacja ma przyszły znacznik czasu",
        ));
    }
    if now_ms - timestamp > config.max_age_ms {
        return Ok(unavailable(
            MotionStatus::Stale,
            0,
            "Pozycja markera jest przeterminowana; wyprzedzenie wstrzymane",
        ));
    }
    let fresh: Vec<_> = samples
        .iter()
        .filter(|sample| now_ms - sample.timestamp_ms <= config.max_age_ms)
        .collect();
    let fresh = &fresh[fresh.len().saturating_sub(config.max_samples)..];
    let count = fresh.len();
    let first = fresh[0];
    if first.source_id.trim().is_empty()
        || fresh.iter().any(|sample| {
            sample.source_id != first.source_id
                || !sample.map_position.valid()
                || sample.map_position.x < 0.0
                || sample.map_position.y < 0.0
                || sample.map_position.x >= f64::from(config.map_width)
                || sample.map_position.y >= f64::from(config.map_height)
                || !sample.confidence.is_finite()
                || sample.confidence < config.min_confidence
                || sample.confidence > 1.0
        })
    {
        return Ok(unavailable(
            MotionStatus::Rejected,
            count,
            "Niejednoznaczny marker, pozycja poza mapą lub zbyt niska pewność pomiaru",
        ));
    }
    if fresh
        .windows(2)
        .any(|pair| pair[1].timestamp_ms <= pair[0].timestamp_ms)
    {
        return Ok(unavailable(
            MotionStatus::Rejected,
            count,
            "Znaczniki czasu muszą być ściśle rosnące",
        ));
    }
    if count < 2 || timestamp - first.timestamp_ms < config.min_delta_ms {
        return Ok(unavailable(
            MotionStatus::Collecting,
            count,
            "Za mało świeżych pozycji lub zbyt krótki odstęp obserwacji",
        ));
    }
    let world: Vec<Point> = match fresh
        .iter()
        .map(|sample| calibration.transform(sample.map_position))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(world) => world,
        Err(_) => {
            return Ok(unavailable(
                MotionStatus::Rejected,
                count,
                "Transformacja obserwacji przekracza zakres współrzędnych",
            ));
        }
    };
    for (positions, observations) in world.windows(2).zip(fresh.windows(2)) {
        let delta_seconds =
            (observations[1].timestamp_ms - observations[0].timestamp_ms) as f64 / 1000.0;
        let speed = (positions[1].x - positions[0].x).hypot(positions[1].y - positions[0].y)
            / delta_seconds;
        if !speed.is_finite() || speed > config.max_speed_units_per_second {
            return Ok(unavailable(
                MotionStatus::Rejected,
                count,
                "Odrzucono skok markera przekraczający ustawiony limit prędkości",
            ));
        }
    }
    let elapsed: Vec<_> = fresh
        .iter()
        .map(|sample| (sample.timestamp_ms - first.timestamp_ms) as f64 / 1000.0)
        .collect();
    let mean_t = elapsed.iter().sum::<f64>() / count as f64;
    // Work relative to the first world point to avoid loss of precision from a
    // large calibrated world origin.
    let offset_x: Vec<_> = world.iter().map(|point| point.x - world[0].x).collect();
    let offset_y: Vec<_> = world.iter().map(|point| point.y - world[0].y).collect();
    let mean_x = offset_x.iter().sum::<f64>() / count as f64;
    let mean_y = offset_y.iter().sum::<f64>() / count as f64;
    let denominator = elapsed
        .iter()
        .map(|time| (time - mean_t).powi(2))
        .sum::<f64>();
    let vx = elapsed
        .iter()
        .zip(&offset_x)
        .map(|(time, x)| (time - mean_t) * (x - mean_x))
        .sum::<f64>()
        / denominator;
    let vy = elapsed
        .iter()
        .zip(&offset_y)
        .map(|(time, y)| (time - mean_t) * (y - mean_y))
        .sum::<f64>()
        / denominator;
    let speed = vx.hypot(vy);
    if !vx.is_finite() || !vy.is_finite() || speed > config.max_speed_units_per_second {
        return Ok(unavailable(
            MotionStatus::Rejected,
            count,
            "Niepoprawna lub zbyt szybka estymacja ruchu",
        ));
    }
    let residual = (elapsed
        .iter()
        .zip(offset_x.iter().zip(&offset_y))
        .map(|(time, (x, y))| {
            (x - (mean_x + vx * (time - mean_t))).powi(2)
                + (y - (mean_y + vy * (time - mean_t))).powi(2)
        })
        .sum::<f64>()
        / count as f64)
        .sqrt();
    let measurement_confidence = fresh
        .iter()
        .map(|sample| sample.confidence)
        .fold(1.0, f64::min);
    let confidence =
        measurement_confidence / (1.0 + residual / (calibration.uncertainty_units + 1.0));
    if !confidence.is_finite() || confidence < config.min_confidence {
        return Ok(unavailable(
            MotionStatus::Rejected,
            count,
            "Obserwowany ruch nie odpowiada dostatecznie modelowi stałej prędkości",
        ));
    }
    Ok(MotionEstimate {
        status: MotionStatus::Ready,
        velocity: Some([vx, vy]),
        speed_units_per_second: Some(speed),
        position_world: world.last().copied(),
        count,
        confidence,
        timestamp_ms: timestamp,
        max_prediction_seconds: config.max_prediction_seconds,
        assumptions: vec![
            "Stała prędkość w płaszczyźnie mapy; brak pomiaru wysokości celu".into(),
            "Prędkość pochodzi ze świeżych pozycji jawnie wybranego widocznego markera".into(),
            "Jednostki i orientacja pochodzą z kalibracji mapy użytkownika".into(),
        ],
        warning: Some("Ruch obserwowany; predykcja przybliżona, bez modelu manewrów".into()),
    })
}

/// Predict from the actual time of the newest observation, never from an
/// implicitly refreshed timestamp. Consumers must separately enforce freshness.
pub fn predict_position(estimate: &MotionEstimate, seconds: f64) -> Result<Point, String> {
    if estimate.status != MotionStatus::Ready
        || !seconds.is_finite()
        || seconds < 0.0
        || seconds > estimate.max_prediction_seconds
    {
        return Err("Brak gotowej estymacji lub horyzont predykcji poza limitem".into());
    }
    let position = estimate.position_world.ok_or("Brak pozycji obserwowanej")?;
    let velocity = estimate.velocity.ok_or("Brak prędkości obserwowanej")?;
    let predicted = Point {
        x: position.x + velocity[0] * seconds,
        y: position.y + velocity[1] * seconds,
    };
    if !predicted.valid() {
        return Err("Predykcja przekracza zakres współrzędnych".into());
    }
    Ok(predicted)
}
