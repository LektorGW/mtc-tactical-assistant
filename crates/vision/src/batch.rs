use crate::{
    detect_markers, ocr, parse_numeric, stabilize, HudDiscovery, HudDiscoveryStatus, MarkerConfig,
    MarkerObservation, Observation, Roi,
};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OcrKind {
    Timer,
    Heading,
    Altitude,
    Speed,
    VehicleName,
    WeaponName,
    MapName,
    Text,
}

impl OcrKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Heading => "heading",
            Self::Altitude => "altitude",
            Self::Speed => "speed",
            Self::VehicleName => "vehicleName",
            Self::WeaponName => "weaponName",
            Self::MapName => "mapName",
            Self::Text => "text",
        }
    }

    fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Timer | Self::Heading | Self::Altitude | Self::Speed
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRegionConfig {
    pub id: String,
    pub kind: OcrKind,
    pub roi: Roi,
    pub upscale: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisConfig {
    pub schema_version: u32,
    pub reference_width: u32,
    pub reference_height: u32,
    pub aspect_tolerance: f64,
    pub max_age_ms: u64,
    pub ocr_regions: Vec<OcrRegionConfig>,
    pub marker_config: Option<MarkerConfig>,
}

impl AnalysisConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.reference_width == 0
            || self.reference_height == 0
            || !self.aspect_tolerance.is_finite()
            || !(0.0..=0.2).contains(&self.aspect_tolerance)
            || self.max_age_ms == 0
            || self.max_age_ms > 60_000
            || self.ocr_regions.len() > 16
        {
            return Err("Niepoprawna wersja, geometria lub limit analizy obrazu".into());
        }
        let mut ids = std::collections::HashSet::new();
        for region in &self.ocr_regions {
            region.roi.validate()?;
            if region.id.trim().is_empty()
                || region.id.len() > 80
                || !ids.insert(&region.id)
                || !(1..=4).contains(&region.upscale)
            {
                return Err("Obszary OCR wymagają unikalnych ID i powiększenia od 1 do 4".into());
            }
        }
        if let Some(config) = &self.marker_config {
            config.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextObservation {
    pub region_id: String,
    pub kind: OcrKind,
    pub text: String,
    pub confidence: f64,
    pub timestamp_ms: u64,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegionStatus {
    Observed,
    Empty,
    Ambiguous,
    Error,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRegionResult {
    pub id: String,
    pub kind: OcrKind,
    pub status: RegionStatus,
    pub text: String,
    pub observation: Option<Observation>,
    pub text_observation: Option<TextObservation>,
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisStatus {
    Ok,
    Partial,
    Blocked,
    Stale,
    Future,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameAnalysis {
    pub status: AnalysisStatus,
    pub timestamp_ms: u64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub elapsed_ms: f64,
    pub ocr_regions: Vec<OcrRegionResult>,
    pub numeric_observations: Vec<Observation>,
    pub text_observations: Vec<TextObservation>,
    pub markers: Vec<MarkerObservation>,
    pub marker_warning: Option<String>,
    pub warnings: Vec<String>,
}

/// Invalidate every result, including nested ROI observations. Call this after
/// a slow OCR pass or a source/configuration change before consumers use data.
pub fn invalidate_frame(analysis: &mut FrameAnalysis, status: AnalysisStatus, reason: &str) {
    analysis.status = status;
    analysis.numeric_observations.clear();
    analysis.text_observations.clear();
    analysis.markers.clear();
    for region in &mut analysis.ocr_regions {
        region.status = RegionStatus::Blocked;
        region.observation = None;
        region.text_observation = None;
        region.warning = Some(reason.into());
    }
    analysis.marker_warning = Some(reason.into());
    analysis.warnings.push(reason.into());
}

/// Analyze all calibrated ROIs from one captured frame. No image is recorded or
/// sent anywhere. Field failures remain separate so one failed OCR does not
/// discard unrelated valid measurements from the same frame.
pub fn analyze_frame(
    image: &RgbaImage,
    timestamp_ms: u64,
    now_ms: u64,
    config: &AnalysisConfig,
) -> Result<FrameAnalysis, String> {
    analyze_frame_with_ocr(image, timestamp_ms, now_ms, config, ocr)
}

/// Injectable OCR makes deterministic regression tests independent of system
/// languages. Production uses analyze_frame with the Windows implementation.
/// `recognize` may be called up to three times per ROI (plain enlargement, then
/// light-text fallbacks) until it returns an accepted value for the region kind.
pub fn analyze_frame_with_ocr<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    now_ms: u64,
    config: &AnalysisConfig,
    mut recognize: F,
) -> Result<FrameAnalysis, String>
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    analyze_frame_with_reader(image, timestamp_ms, now_ms, config, |region| {
        // The plain enlarged crop is read first; light-text normalization is a
        // deterministic fallback only when that read is not one valid value.
        crate::read_roi_text(
            image,
            region.roi,
            region.upscale,
            region.kind,
            &mut recognize,
        )
        .map(|(text, _)| text)
        .map_err(|reason| (RegionStatus::Error, reason))
    })
}

/// Reuse the full-frame OCR text for explicitly accepted automatic HUD regions.
/// Discovery and pixels must belong to the same source frame. There is no second
/// crop OCR or fallback to arbitrary text. Proposal confidence confirms region
/// placement only; measurements still need normal two-frame stabilization.
pub fn analyze_frame_from_discovery(
    image: &RgbaImage,
    timestamp_ms: u64,
    now_ms: u64,
    config: &AnalysisConfig,
    discovery: &HudDiscovery,
) -> Result<FrameAnalysis, String> {
    if discovery.schema_version != 1
        || discovery.timestamp_ms != timestamp_ms
        || discovery.reference_width != image.width()
        || discovery.reference_height != image.height()
        || !discovery.elapsed_ms.is_finite()
        || discovery.elapsed_ms < 0.0
        || discovery.regions.len() > 16
    {
        return Err("Odkrycie HUD nie odpowiada tej samej klatce lub ma błędny format".into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut kinds = std::collections::HashSet::new();
    for proposal in &discovery.regions {
        proposal.region.roi.validate()?;
        if proposal.region.id.trim().is_empty()
            || proposal.region.id.len() > 80
            || !(1..=4).contains(&proposal.region.upscale)
            || !ids.insert(&proposal.region.id)
            || !kinds.insert(proposal.region.kind.as_str())
            || !proposal.confidence.is_finite()
            || !(0.0..=1.0).contains(&proposal.confidence)
            || proposal.text.len() > 4096
            || proposal.value.is_some_and(|value| !value.is_finite())
        {
            return Err("Odkrycie HUD zawiera niepoprawne lub niejednoznaczne propozycje".into());
        }
    }
    let started = Instant::now();
    let mut analysis = analyze_frame_with_reader(image, timestamp_ms, now_ms, config, |region| {
        if discovery.status == HudDiscoveryStatus::Blocked {
            return Err((
                RegionStatus::Blocked,
                "Brak dowodów pełnoklatkowego OCR HUD".into(),
            ));
        }
        let proposal = discovery
            .regions
            .iter()
            .find(|proposal| proposal.region.id == region.id && proposal.region.kind == region.kind)
            .ok_or_else(|| {
                (
                    RegionStatus::Blocked,
                    "Nie wykryto tego pola w bieżącej klatce HUD".into(),
                )
            })?;
        if proposal.confidence < 0.85 || proposal.evidence.is_empty() {
            return Err((
                RegionStatus::Blocked,
                "Lokalizacja pola HUD wymaga dwóch świeżych zgodnych odkryć".into(),
            ));
        }
        let a = proposal.region.roi;
        let b = region.roi;
        if [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
            .iter()
            .any(|delta| delta.abs() > 1e-9)
        {
            return Err((
                RegionStatus::Blocked,
                "Propozycja HUD nie odpowiada zaakceptowanemu obszarowi".into(),
            ));
        }
        Ok(proposal.text.clone())
    })?;
    let completed_now =
        now_ms.saturating_add(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64);
    if timestamp_ms <= completed_now && completed_now - timestamp_ms > config.max_age_ms {
        invalidate_frame(
            &mut analysis,
            AnalysisStatus::Stale,
            "Klatka straciła świeżość podczas przetwarzania odkrytego HUD",
        );
    }
    Ok(analysis)
}

fn analyze_frame_with_reader<F>(
    image: &RgbaImage,
    timestamp_ms: u64,
    now_ms: u64,
    config: &AnalysisConfig,
    mut read_region: F,
) -> Result<FrameAnalysis, String>
where
    F: FnMut(&OcrRegionConfig) -> Result<String, (RegionStatus, String)>,
{
    let started = Instant::now();
    config.validate()?;
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Obraz jest pusty lub przekracza limit 64 milionów pikseli".into());
    }
    let mut analysis = FrameAnalysis {
        status: AnalysisStatus::Ok,
        timestamp_ms,
        frame_width: image.width(),
        frame_height: image.height(),
        elapsed_ms: 0.0,
        ocr_regions: Vec::new(),
        numeric_observations: Vec::new(),
        text_observations: Vec::new(),
        markers: Vec::new(),
        marker_warning: None,
        warnings: Vec::new(),
    };
    let failure = if timestamp_ms > now_ms {
        Some((AnalysisStatus::Future, "Klatka ma przyszły znacznik czasu"))
    } else if now_ms - timestamp_ms > config.max_age_ms {
        Some((
            AnalysisStatus::Stale,
            "Klatka jest przeterminowana; analiza wstrzymana",
        ))
    } else {
        let actual = f64::from(image.width()) / f64::from(image.height());
        let expected = f64::from(config.reference_width) / f64::from(config.reference_height);
        if (actual / expected - 1.0).abs() > config.aspect_tolerance {
            Some((
                AnalysisStatus::Blocked,
                "Proporcje okna zmieniły się; ponownie wyznacz obszary OCR i mapy",
            ))
        } else {
            None
        }
    };
    if let Some((status, reason)) = failure {
        analysis.status = status;
        analysis.warnings.push(reason.into());
        analysis.ocr_regions = config
            .ocr_regions
            .iter()
            .map(|region| OcrRegionResult {
                id: region.id.clone(),
                kind: region.kind,
                status: RegionStatus::Blocked,
                text: String::new(),
                observation: None,
                text_observation: None,
                warning: Some(reason.into()),
            })
            .collect();
        analysis.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        return Ok(analysis);
    }
    for region in &config.ocr_regions {
        let mut result = OcrRegionResult {
            id: region.id.clone(),
            kind: region.kind,
            status: RegionStatus::Empty,
            text: String::new(),
            observation: None,
            text_observation: None,
            warning: None,
        };
        let read = read_region(region);
        match read {
            Err((status, error)) => {
                result.status = status;
                result.warning = Some(error);
            }
            Ok(raw) => {
                result.text = raw.split_whitespace().collect::<Vec<_>>().join(" ");
                if result.text.is_empty() {
                    result.warning = Some("Nie odczytano jawnego tekstu w ROI".into());
                } else if result.text.len() > 4096 {
                    result.status = RegionStatus::Error;
                    result.text.clear();
                    result.warning = Some("Odczyt przekracza limit 4096 bajtów; zawęź ROI".into());
                } else if region.kind.is_numeric() {
                    result.observation =
                        parse_numeric(region.kind.as_str(), &result.text, timestamp_ms);
                    if let Some(observation) = &result.observation {
                        result.status = RegionStatus::Observed;
                        analysis.numeric_observations.push(observation.clone());
                    } else {
                        result.status = RegionStatus::Ambiguous;
                        result.warning =
                            Some("Odczyt nie zawiera jednej poprawnej wartości; zawęź ROI".into());
                    }
                } else {
                    let observation = TextObservation {
                        region_id: region.id.clone(),
                        kind: region.kind,
                        text: result.text.clone(),
                        confidence: 0.55,
                        timestamp_ms,
                        source: "Jawny tekst UI odczytany OCR Windows; pewność heurystyczna".into(),
                    };
                    result.status = RegionStatus::Observed;
                    result.text_observation = Some(observation.clone());
                    analysis.text_observations.push(observation);
                }
            }
        }
        if result.status != RegionStatus::Observed {
            analysis.status = AnalysisStatus::Partial;
        }
        analysis.ocr_regions.push(result);
    }
    if let Some(marker_config) = &config.marker_config {
        let detection = detect_markers(
            image,
            marker_config,
            timestamp_ms,
            now_ms,
            config.max_age_ms,
        )?;
        analysis.markers = detection.markers;
        analysis.marker_warning = detection.warning;
        if marker_config.enabled && analysis.marker_warning.is_some() {
            analysis.status = AnalysisStatus::Partial;
        }
    }
    analysis.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(analysis)
}

/// Compare only matching ROI IDs and kinds in successive frames. Callers must
/// reset previous analysis when the capture window or ROI calibration changes.
pub fn stabilize_frame(current: &mut FrameAnalysis, previous: Option<&FrameAnalysis>) {
    let Some(previous) = previous else { return };
    for result in &mut current.ocr_regions {
        let old = previous
            .ocr_regions
            .iter()
            .find(|old| old.id == result.id && old.kind == result.kind);
        if let Some(observation) = &mut result.observation {
            stabilize(observation, old.and_then(|old| old.observation.as_ref()));
        }
        if let Some(observation) = &mut result.text_observation {
            if let Some(old) = old.and_then(|old| old.text_observation.as_ref()) {
                if old.text == observation.text
                    && old.timestamp_ms < observation.timestamp_ms
                    && observation.timestamp_ms - old.timestamp_ms <= 2000
                {
                    observation.confidence = 0.85;
                }
            }
        }
    }
    current.numeric_observations = current
        .ocr_regions
        .iter()
        .filter_map(|result| result.observation.clone())
        .collect();
    current.text_observations = current
        .ocr_regions
        .iter()
        .filter_map(|result| result.text_observation.clone())
        .collect();
}
