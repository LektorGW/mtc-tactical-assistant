//! Consent-gated local recording of captured frames as offline fixtures, and
//! deterministic replay of such a recording through the regular frame analysis.
//! Nothing here captures pixels, uses the network or runs without an explicit
//! request; the caller supplies frames it already obtained from the visible window.

use crate::{analyze_frame_with_ocr, ocr, stabilize_frame, AnalysisConfig, FrameAnalysis, OcrKind};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const RECORDING_SCHEMA_VERSION: u32 = 1;
pub const RECORDING_KIND: &str = "mtc-tactical-assistant-recording";
pub const RECORDING_MANIFEST: &str = "session.json";
pub const MAX_RECORDING_FRAMES: u32 = 10_000;
const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FRAME_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FRAME_AGE_MS: u64 = 2000;
const MANIFEST_FLUSH_FRAMES: u32 = 10;
const MAX_REPLAY_FRAMES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingFormat {
    Png,
    Jpeg,
}

impl RecordingFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLimits {
    /// Upper bound of stored frames per second (0.1–5).
    pub max_fps: f64,
    pub max_frames: u32,
    pub max_bytes: u64,
    pub max_duration_ms: u64,
}

impl RecordingLimits {
    pub fn validate(&self) -> Result<(), String> {
        if !self.max_fps.is_finite()
            || !(0.1..=5.0).contains(&self.max_fps)
            || !(1..=MAX_RECORDING_FRAMES).contains(&self.max_frames)
            || !(1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&self.max_bytes)
            || !(1000..=3_600_000).contains(&self.max_duration_ms)
        {
            return Err("Limity nagrania: 0,1–5 FPS, 1–10000 klatek, 1 MB–8 GB, 1 s–60 min".into());
        }
        Ok(())
    }

    /// Minimum distance between stored source timestamps.
    pub fn frame_interval_ms(&self) -> u64 {
        (1000.0 / self.max_fps).ceil() as u64
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingConsent {
    /// Always true in a valid recording; the writer refuses to start otherwise.
    pub explicit: bool,
    pub granted_at_ms: u64,
    pub statement: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSource {
    pub handle: Option<i64>,
    pub window_title: Option<String>,
    /// `wgc` or `gdi`.
    pub backend: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedFrame {
    pub index: u32,
    /// File name inside the recording directory, never a path.
    pub file: String,
    /// Source time of the captured frame (Unix ms), not the write time.
    pub timestamp_ms: u64,
    pub source_timestamp_100ns: Option<i64>,
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSession {
    pub schema_version: u32,
    pub kind: String,
    pub created_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub completed: bool,
    pub stop_reason: Option<String>,
    pub consent: RecordingConsent,
    pub source: RecordingSource,
    pub format: RecordingFormat,
    pub jpeg_quality: Option<u8>,
    pub limits: RecordingLimits,
    /// ROI/marker configuration active when recording started (normalized ROIs).
    pub analysis_config: Option<AnalysisConfig>,
    pub frames: Vec<RecordedFrame>,
    pub total_bytes: u64,
    pub notes: Vec<String>,
}

pub struct RecordingOptions {
    /// Must be true; set only after the user ticked the consent checkbox.
    pub consent: bool,
    pub format: RecordingFormat,
    pub jpeg_quality: u8,
    pub limits: RecordingLimits,
    pub source: RecordingSource,
    pub analysis_config: Option<AnalysisConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrameDecision {
    Recorded(RecordedFrame),
    /// The frame is valid but not stored (FPS limit, duplicate, stale/future time).
    Skipped(&'static str),
    /// A frame, size or time limit ends the recording; nothing was written.
    LimitReached(String),
}

/// Writes frames supplied by the caller into one new directory. The manifest is
/// rewritten periodically, so an interrupted recording remains loadable.
pub struct RecordingWriter {
    directory: PathBuf,
    session: RecordingSession,
    started_at_ms: u64,
    last_timestamp: Option<u64>,
    last_sequence: Option<u64>,
    unflushed: u32,
    finished: bool,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(|e| format!("Zapis nagrania: {e}"))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("Zapis nagrania: {e}"))
}

fn rgb_bytes(image: &RgbaImage) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(image.width() as usize * image.height() as usize * 3);
    for pixel in image.as_raw().as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

/// Encodes without alpha: captured client pixels are opaque.
pub fn encode_recording_frame(
    image: &RgbaImage,
    format: RecordingFormat,
    jpeg_quality: u8,
) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > 64_000_000
    {
        return Err("Nieobsługiwany rozmiar klatki nagrania".into());
    }
    let rgb = rgb_bytes(image);
    let mut bytes = Vec::new();
    match format {
        RecordingFormat::Png => image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(
            &rgb,
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        ),
        RecordingFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, jpeg_quality)
                .write_image(
                    &rgb,
                    image.width(),
                    image.height(),
                    image::ExtendedColorType::Rgb8,
                )
        }
    }
    .map_err(|e| format!("Kodowanie klatki: {e}"))?;
    Ok(bytes)
}

impl RecordingWriter {
    pub fn create(parent: &Path, options: RecordingOptions, now_ms: u64) -> Result<Self, String> {
        if !options.consent {
            return Err("Nagrywanie wymaga jawnej zgody zaznaczonej w tej sesji".into());
        }
        options.limits.validate()?;
        if !(40..=100).contains(&options.jpeg_quality) {
            return Err("Jakość JPEG musi wynosić 40–100".into());
        }
        if options.source.backend != "wgc" && options.source.backend != "gdi" {
            return Err("Nieznany backend źródła nagrania".into());
        }
        if let Some(config) = &options.analysis_config {
            validate_recording_config(config)?;
        }
        // `absolute` keeps a normal path; canonicalize would add a verbatim prefix on Windows.
        let parent = std::path::absolute(parent)
            .map_err(|e| format!("Katalog nagrania jest niedostępny: {e}"))?;
        if !parent.is_dir() {
            return Err("Wybierz istniejący katalog na nagrania".into());
        }
        // A new leaf directory is created exclusively; earlier recordings are never reused.
        let mut directory = None;
        for attempt in 0..100u32 {
            let name = if attempt == 0 {
                format!("mtc-recording-{now_ms}")
            } else {
                format!("mtc-recording-{now_ms}-{attempt}")
            };
            let candidate = parent.join(name);
            match std::fs::create_dir(&candidate) {
                Ok(()) => {
                    directory = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Nie można utworzyć katalogu nagrania: {error}")),
            }
        }
        let directory = directory.ok_or("Nie można utworzyć unikalnego katalogu nagrania")?;
        let session = RecordingSession {
            schema_version: RECORDING_SCHEMA_VERSION,
            kind: RECORDING_KIND.into(),
            created_at_ms: now_ms,
            ended_at_ms: None,
            completed: false,
            stop_reason: None,
            consent: RecordingConsent {
                explicit: true,
                granted_at_ms: now_ms,
                statement: "Użytkownik jawnie zezwolił na lokalny zapis klatek wybranego okna do tego katalogu. Zgoda nie jest zapamiętywana między sesjami.".into(),
            },
            source: options.source,
            format: options.format,
            jpeg_quality: (options.format == RecordingFormat::Jpeg).then_some(options.jpeg_quality),
            limits: options.limits,
            analysis_config: options.analysis_config,
            frames: Vec::new(),
            total_bytes: 0,
            notes: vec![
                "Klatki zawierają wyłącznie obszar klienta wybranego okna, bez kanału alfa.".into(),
                "Czas klatki to czas źródła przechwycenia (Unix ms).".into(),
                "Pliki nie są wysyłane; służą jako lokalne fixtures do analizy offline.".into(),
            ],
        };
        let writer = Self {
            directory,
            session,
            started_at_ms: now_ms,
            last_timestamp: None,
            last_sequence: None,
            unflushed: 0,
            finished: false,
        };
        writer.write_manifest()?;
        Ok(writer)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn session(&self) -> &RecordingSession {
        &self.session
    }

    fn write_manifest(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.session).map_err(|e| e.to_string())?;
        write_atomic(&self.directory.join(RECORDING_MANIFEST), &bytes)
    }

    /// Reason to stop before the next frame, including the wall-clock duration limit.
    pub fn limit_reason(&self, now_ms: u64) -> Option<String> {
        let limits = &self.session.limits;
        if self.session.frames.len() as u64 >= u64::from(limits.max_frames) {
            Some(format!("Osiągnięto limit {} klatek", limits.max_frames))
        } else if now_ms.saturating_sub(self.started_at_ms) >= limits.max_duration_ms {
            Some(format!(
                "Osiągnięto limit czasu {} s",
                limits.max_duration_ms / 1000
            ))
        } else if self.session.total_bytes >= limits.max_bytes {
            Some("Osiągnięto limit rozmiaru nagrania".into())
        } else {
            None
        }
    }

    pub fn offer(
        &mut self,
        image: &RgbaImage,
        timestamp_ms: u64,
        sequence: u64,
        source_timestamp_100ns: Option<i64>,
        now_ms: u64,
    ) -> Result<FrameDecision, String> {
        if self.finished {
            return Err("Nagranie zostało zakończone".into());
        }
        if let Some(reason) = self.limit_reason(now_ms) {
            return Ok(FrameDecision::LimitReached(reason));
        }
        if timestamp_ms > now_ms {
            return Ok(FrameDecision::Skipped("Klatka ma przyszły znacznik czasu"));
        }
        if now_ms - timestamp_ms > MAX_FRAME_AGE_MS || timestamp_ms < self.started_at_ms {
            return Ok(FrameDecision::Skipped("Klatka jest przeterminowana"));
        }
        if self.last_sequence == Some(sequence) {
            return Ok(FrameDecision::Skipped("Duplikat klatki źródłowej"));
        }
        if let Some(last) = self.last_timestamp {
            if timestamp_ms < last.saturating_add(self.session.limits.frame_interval_ms()) {
                return Ok(FrameDecision::Skipped("Limit FPS nagrania"));
            }
        }
        let quality = self.session.jpeg_quality.unwrap_or(90);
        let bytes = encode_recording_frame(image, self.session.format, quality)?;
        let total = self.session.total_bytes.saturating_add(bytes.len() as u64);
        if total > self.session.limits.max_bytes {
            return Ok(FrameDecision::LimitReached(
                "Kolejna klatka przekroczyłaby limit rozmiaru nagrania".into(),
            ));
        }
        let index = self.session.frames.len() as u32 + 1;
        let file = format!("frame-{index:06}.{}", self.session.format.extension());
        write_atomic(&self.directory.join(&file), &bytes)?;
        let frame = RecordedFrame {
            index,
            file,
            timestamp_ms,
            source_timestamp_100ns,
            sequence,
            width: image.width(),
            height: image.height(),
            bytes: bytes.len() as u64,
        };
        self.session.frames.push(frame.clone());
        self.session.total_bytes = total;
        self.last_timestamp = Some(timestamp_ms);
        self.last_sequence = Some(sequence);
        self.unflushed += 1;
        if self.unflushed >= MANIFEST_FLUSH_FRAMES {
            self.write_manifest()?;
            self.unflushed = 0;
        }
        Ok(FrameDecision::Recorded(frame))
    }

    pub fn finish(mut self, reason: &str, now_ms: u64) -> Result<RecordingSession, String> {
        self.finish_in_place(reason, now_ms)?;
        Ok(self.session.clone())
    }

    fn finish_in_place(&mut self, reason: &str, now_ms: u64) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.session.completed = true;
        self.session.ended_at_ms = Some(now_ms);
        self.session.stop_reason = Some(reason.into());
        self.write_manifest()
    }
}

impl Drop for RecordingWriter {
    fn drop(&mut self) {
        if !self.finished {
            // Best effort: keep frames already written loadable after a crash path.
            self.session.stop_reason = Some("Nagranie przerwane bez zakończenia".into());
            let _ = self.write_manifest();
        }
    }
}

/// A reference size of 0 x 0 means "use the frame size", as in live analysis.
pub fn validate_recording_config(config: &AnalysisConfig) -> Result<(), String> {
    if config.reference_width == 0 && config.reference_height == 0 {
        let mut normalized = config.clone();
        normalized.reference_width = 1;
        normalized.reference_height = 1;
        normalized.validate()
    } else {
        config.validate()
    }
}

fn valid_frame_name(name: &str, format: RecordingFormat) -> bool {
    let Some(stem) = name.strip_prefix("frame-") else {
        return false;
    };
    let Some(digits) = stem.strip_suffix(&format!(".{}", format.extension())) else {
        return false;
    };
    digits.len() == 6 && digits.bytes().all(|c| c.is_ascii_digit())
}

fn recording_directory(path: &Path) -> PathBuf {
    if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(RECORDING_MANIFEST))
    {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.to_path_buf()
    }
}

/// Loads and validates `session.json`. `path` may be the recording directory or
/// the manifest itself. Frame names are checked so a manifest cannot point outside.
pub fn load_recording(path: &Path) -> Result<(PathBuf, RecordingSession), String> {
    let directory = recording_directory(path);
    let manifest = directory.join(RECORDING_MANIFEST);
    let file = std::fs::File::open(&manifest)
        .map_err(|e| format!("Brak pliku {RECORDING_MANIFEST} nagrania: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Plik session.json przekracza 32 MB".into());
    }
    let session: RecordingSession =
        serde_json::from_slice(&bytes).map_err(|e| format!("Niepoprawny session.json: {e}"))?;
    validate_session(&session)?;
    Ok((directory, session))
}

pub fn validate_session(session: &RecordingSession) -> Result<(), String> {
    if session.schema_version != RECORDING_SCHEMA_VERSION || session.kind != RECORDING_KIND {
        return Err("Nieobsługiwana wersja lub rodzaj nagrania".into());
    }
    if !session.consent.explicit {
        return Err("Nagranie nie zawiera jawnej zgody na zapis".into());
    }
    session.limits.validate()?;
    if session.frames.len() > MAX_RECORDING_FRAMES as usize {
        return Err("Nagranie przekracza limit 10000 klatek".into());
    }
    if let Some(config) = &session.analysis_config {
        validate_recording_config(config)?;
    }
    let mut previous: Option<&RecordedFrame> = None;
    for frame in &session.frames {
        if !valid_frame_name(&frame.file, session.format)
            || frame.width == 0
            || frame.height == 0
            || u64::from(frame.width) * u64::from(frame.height) > 64_000_000
            || frame.bytes > MAX_FRAME_FILE_BYTES
            || previous
                .is_some_and(|p| frame.index <= p.index || frame.timestamp_ms <= p.timestamp_ms)
        {
            return Err(format!(
                "Niepoprawny wpis klatki {} w session.json",
                frame.index
            ));
        }
        previous = Some(frame);
    }
    Ok(())
}

/// Decodes one recorded frame with size limits and checks it against the manifest.
pub fn read_recorded_frame(directory: &Path, frame: &RecordedFrame) -> Result<RgbaImage, String> {
    if frame.file.contains(['/', '\\']) || frame.file.contains("..") {
        return Err("Niepoprawna nazwa pliku klatki".into());
    }
    let file = std::fs::File::open(directory.join(&frame.file))
        .map_err(|e| format!("Brak klatki {}: {e}", frame.file))?;
    let mut bytes = Vec::new();
    file.take(MAX_FRAME_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FRAME_FILE_BYTES {
        return Err("Plik klatki przekracza 64 MB".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    if image.dimensions() != (frame.width, frame.height) {
        return Err(format!(
            "Wymiary klatki {} nie zgadzają się z session.json",
            frame.file
        ));
    }
    Ok(image)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingFrameAnalysis {
    pub index: u32,
    pub file: String,
    pub timestamp_ms: u64,
    pub analysis: Option<FrameAnalysis>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingRegionSummary {
    pub id: String,
    pub kind: OcrKind,
    pub frames: u32,
    pub observed: u32,
    /// Observations whose heuristic confidence reached 0.85 after stabilization.
    pub stable: u32,
    pub ambiguous: u32,
    pub empty: u32,
    pub errors: u32,
    pub blocked: u32,
    pub last_text: Option<String>,
    pub last_value: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAnalysis {
    pub schema_version: u32,
    pub directory: String,
    pub total_frames: usize,
    pub start: usize,
    pub frames: Vec<RecordingFrameAnalysis>,
    pub regions: Vec<RecordingRegionSummary>,
    pub warnings: Vec<String>,
    pub elapsed_ms: f64,
}

/// Replays recorded frames through `analyze_frame` with system OCR.
pub fn analyse_recording(
    path: &Path,
    config: Option<AnalysisConfig>,
    start: usize,
    count: usize,
) -> Result<RecordingAnalysis, String> {
    analyse_recording_with_ocr(path, config, start, count, ocr)
}

/// Deterministic replay: each frame is analysed at its own recorded source time
/// (`now = timestamp`), so freshness refers to the moment of recording, and
/// consecutive frames of one page are stabilized exactly like live frames.
pub fn analyse_recording_with_ocr<F>(
    path: &Path,
    config: Option<AnalysisConfig>,
    start: usize,
    count: usize,
    mut recognize: F,
) -> Result<RecordingAnalysis, String>
where
    F: FnMut(&RgbaImage) -> Result<String, String>,
{
    let started = Instant::now();
    let (directory, session) = load_recording(path)?;
    let mut warnings = vec![
        "Analiza offline używa czasu nagranej klatki; świeżość dotyczy chwili nagrania, nie bieżącego odczytu.".to_string(),
    ];
    let config = match config {
        Some(config) => config,
        None => session
            .analysis_config
            .clone()
            .ok_or("Nagranie nie zawiera konfiguracji ROI; podaj bieżącą konfigurację analizy")?,
    };
    validate_recording_config(&config)?;
    if start >= session.frames.len() && !session.frames.is_empty() {
        return Err("Początek analizy wykracza poza liczbę klatek nagrania".into());
    }
    let count = count.clamp(1, MAX_REPLAY_FRAMES);
    if start > 0 {
        warnings
            .push("Stabilizacja zaczyna się od pierwszej klatki analizowanego fragmentu".into());
    }
    if !session.completed {
        warnings.push(
            "Nagranie nie zostało poprawnie zakończone; analizowane są zapisane klatki".into(),
        );
    }
    let mut regions: Vec<RecordingRegionSummary> = config
        .ocr_regions
        .iter()
        .map(|region| RecordingRegionSummary {
            id: region.id.clone(),
            kind: region.kind,
            frames: 0,
            observed: 0,
            stable: 0,
            ambiguous: 0,
            empty: 0,
            errors: 0,
            blocked: 0,
            last_text: None,
            last_value: None,
        })
        .collect();
    let mut previous: Option<FrameAnalysis> = None;
    let mut frames = Vec::new();
    for frame in session.frames.iter().skip(start).take(count) {
        let result = read_recorded_frame(&directory, frame).and_then(|image| {
            let mut frame_config = config.clone();
            if frame_config.reference_width == 0 && frame_config.reference_height == 0 {
                frame_config.reference_width = image.width();
                frame_config.reference_height = image.height();
            }
            analyze_frame_with_ocr(
                &image,
                frame.timestamp_ms,
                frame.timestamp_ms,
                &frame_config,
                &mut recognize,
            )
        });
        match result {
            Ok(mut analysis) => {
                stabilize_frame(&mut analysis, previous.as_ref());
                for region in &analysis.ocr_regions {
                    let Some(summary) = regions.iter_mut().find(|s| s.id == region.id) else {
                        continue;
                    };
                    summary.frames += 1;
                    match region.status {
                        crate::RegionStatus::Observed => summary.observed += 1,
                        crate::RegionStatus::Ambiguous => summary.ambiguous += 1,
                        crate::RegionStatus::Empty => summary.empty += 1,
                        crate::RegionStatus::Error => summary.errors += 1,
                        crate::RegionStatus::Blocked => summary.blocked += 1,
                    }
                    let confidence = region
                        .observation
                        .as_ref()
                        .map(|o| o.confidence)
                        .or(region.text_observation.as_ref().map(|o| o.confidence));
                    if confidence.is_some_and(|c| c >= 0.85) {
                        summary.stable += 1;
                    }
                    if !region.text.is_empty() {
                        summary.last_text = Some(region.text.clone());
                    }
                    if let Some(observation) = &region.observation {
                        summary.last_value = Some(observation.value);
                    }
                }
                frames.push(RecordingFrameAnalysis {
                    index: frame.index,
                    file: frame.file.clone(),
                    timestamp_ms: frame.timestamp_ms,
                    analysis: Some(analysis.clone()),
                    error: None,
                });
                previous = Some(analysis);
            }
            Err(error) => {
                // A missing or corrupt frame breaks stabilization continuity.
                previous = None;
                frames.push(RecordingFrameAnalysis {
                    index: frame.index,
                    file: frame.file.clone(),
                    timestamp_ms: frame.timestamp_ms,
                    analysis: None,
                    error: Some(error),
                });
            }
        }
    }
    Ok(RecordingAnalysis {
        schema_version: 1,
        directory: directory.display().to_string(),
        total_frames: session.frames.len(),
        start,
        frames,
        regions,
        warnings,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OcrRegionConfig, Roi};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mtc-vision-recording-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn config() -> AnalysisConfig {
        AnalysisConfig {
            schema_version: 1,
            reference_width: 0,
            reference_height: 0,
            aspect_tolerance: 0.03,
            max_age_ms: 2000,
            ocr_regions: vec![OcrRegionConfig {
                id: "timer".into(),
                kind: OcrKind::Timer,
                roi: Roi {
                    x: 0.25,
                    y: 0.0,
                    width: 0.5,
                    height: 0.5,
                },
                upscale: 2,
            }],
            marker_config: None,
        }
    }

    fn options(consent: bool, format: RecordingFormat) -> RecordingOptions {
        RecordingOptions {
            consent,
            format,
            jpeg_quality: 85,
            limits: RecordingLimits {
                max_fps: 2.0,
                max_frames: 3,
                max_bytes: 4 * 1024 * 1024,
                max_duration_ms: 60_000,
            },
            source: RecordingSource {
                handle: Some(42),
                window_title: Some("Syntetyczne okno testowe".into()),
                backend: "wgc".into(),
            },
            analysis_config: Some(config()),
        }
    }

    fn frame(shade: u8) -> RgbaImage {
        RgbaImage::from_fn(32, 20, |x, y| {
            image::Rgba([shade, (x * 7) as u8, (y * 11) as u8, 255])
        })
    }

    #[test]
    fn refuses_recording_without_explicit_consent_and_writes_nothing() {
        let dir = TempDir::new("consent");
        assert!(
            RecordingWriter::create(&dir.0, options(false, RecordingFormat::Png), 1000).is_err()
        );
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
    }

    #[test]
    fn rejects_invalid_limits_backend_and_missing_parent_directory() {
        let dir = TempDir::new("limits");
        let mut bad = options(true, RecordingFormat::Png);
        bad.limits.max_fps = 30.0;
        assert!(RecordingWriter::create(&dir.0, bad, 1000).is_err());
        let mut bad = options(true, RecordingFormat::Png);
        bad.source.backend = "memory".into();
        assert!(RecordingWriter::create(&dir.0, bad, 1000).is_err());
        assert!(RecordingWriter::create(
            &dir.0.join("missing"),
            options(true, RecordingFormat::Png),
            1000
        )
        .is_err());
    }

    #[test]
    fn stores_frames_with_source_times_and_respects_fps_duplicates_and_staleness() {
        let dir = TempDir::new("fps");
        let mut writer =
            RecordingWriter::create(&dir.0, options(true, RecordingFormat::Png), 1000).unwrap();
        let first = writer.offer(&frame(10), 1100, 1, Some(5), 1150).unwrap();
        assert!(matches!(first, FrameDecision::Recorded(ref f) if f.file == "frame-000001.png"));
        // 2 FPS limit: a frame 200 ms later is skipped, 500 ms later is stored.
        assert_eq!(
            writer.offer(&frame(11), 1300, 2, None, 1350).unwrap(),
            FrameDecision::Skipped("Limit FPS nagrania")
        );
        assert_eq!(
            writer.offer(&frame(11), 1700, 1, None, 1750).unwrap(),
            FrameDecision::Skipped("Duplikat klatki źródłowej")
        );
        assert_eq!(
            writer.offer(&frame(11), 1700, 3, None, 5000).unwrap(),
            FrameDecision::Skipped("Klatka jest przeterminowana")
        );
        assert_eq!(
            writer.offer(&frame(11), 1800, 3, None, 1700).unwrap(),
            FrameDecision::Skipped("Klatka ma przyszły znacznik czasu")
        );
        assert!(matches!(
            writer.offer(&frame(12), 1600, 4, None, 1650).unwrap(),
            FrameDecision::Recorded(_)
        ));
        let directory = writer.directory().to_path_buf();
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("mtc-recording-1000"));
        let session = writer.finish("Zatrzymano przez użytkownika", 2000).unwrap();
        assert!(session.completed && session.consent.explicit);
        assert_eq!(session.frames.len(), 2);
        let (_, loaded) = load_recording(&directory.join(RECORDING_MANIFEST)).unwrap();
        assert_eq!(loaded.frames, session.frames);
        assert_eq!(loaded.frames[0].timestamp_ms, 1100);
        assert_eq!(loaded.frames[0].source_timestamp_100ns, Some(5));
        assert_eq!(loaded.analysis_config.unwrap().ocr_regions[0].id, "timer");
        let image = read_recorded_frame(&directory, &loaded.frames[1]).unwrap();
        assert_eq!(image.get_pixel(3, 2).0, frame(12).get_pixel(3, 2).0);
    }

    #[test]
    fn stops_at_frame_size_and_duration_limits_without_writing_extra_files() {
        let dir = TempDir::new("stop");
        let mut writer =
            RecordingWriter::create(&dir.0, options(true, RecordingFormat::Jpeg), 1000).unwrap();
        for (i, time) in [1100u64, 1600, 2100].into_iter().enumerate() {
            assert!(matches!(
                writer
                    .offer(&frame(i as u8), time, i as u64, None, time)
                    .unwrap(),
                FrameDecision::Recorded(_)
            ));
        }
        assert!(matches!(
            writer.offer(&frame(9), 2600, 9, None, 2600).unwrap(),
            FrameDecision::LimitReached(_)
        ));
        assert_eq!(std::fs::read_dir(writer.directory()).unwrap().count(), 4);
        let mut options = options(true, RecordingFormat::Png);
        options.limits.max_frames = 100;
        options.limits.max_bytes = 1024 * 1024;
        let mut writer = RecordingWriter::create(&dir.0, options, 1000).unwrap();
        assert!(writer.limit_reason(1000 + 59_999).is_none());
        assert!(writer.limit_reason(1000 + 60_000).is_some());
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let noise = RgbaImage::from_fn(1024, 1024, |_, _| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let [r, g, b, ..] = state.to_le_bytes();
            image::Rgba([r, g, b, 255])
        });
        assert!(matches!(
            writer.offer(&noise, 1100, 1, None, 1100).unwrap(),
            FrameDecision::LimitReached(_)
        ));
        assert!(writer.session().frames.is_empty());
    }

    #[test]
    fn never_reuses_an_existing_recording_directory() {
        let dir = TempDir::new("unique");
        let a = RecordingWriter::create(&dir.0, options(true, RecordingFormat::Png), 1000).unwrap();
        let b = RecordingWriter::create(&dir.0, options(true, RecordingFormat::Png), 1000).unwrap();
        assert_ne!(a.directory(), b.directory());
    }

    #[test]
    fn rejects_manifest_entries_pointing_outside_the_recording() {
        let dir = TempDir::new("traversal");
        let mut writer =
            RecordingWriter::create(&dir.0, options(true, RecordingFormat::Png), 1000).unwrap();
        writer.offer(&frame(1), 1100, 1, None, 1100).unwrap();
        let directory = writer.directory().to_path_buf();
        let mut session = writer.finish("Koniec", 1200).unwrap();
        for name in [
            "../secret.png",
            "frame-000001.jpg",
            "frame-1.png",
            "C:\\x\\frame-000001.png",
        ] {
            session.frames[0].file = name.into();
            assert!(validate_session(&session).is_err(), "{name}");
        }
        session.frames[0].file = "frame-000001.png".into();
        session.consent.explicit = false;
        assert!(validate_session(&session).is_err());
        assert!(load_recording(&directory).is_ok());
    }

    #[test]
    fn replays_recording_at_recorded_time_and_stabilizes_consecutive_frames() {
        let dir = TempDir::new("replay");
        let mut writer =
            RecordingWriter::create(&dir.0, options(true, RecordingFormat::Png), 1000).unwrap();
        for (i, time) in [1100u64, 1600, 2100].into_iter().enumerate() {
            writer
                .offer(&frame(i as u8), time, i as u64, None, time)
                .unwrap();
        }
        let directory = writer.directory().to_path_buf();
        writer.finish("Koniec", 2200).unwrap();
        let mut reads = ["27:42", "27:41", "garbage", "garbage", "garbage"].into_iter();
        let replay = analyse_recording_with_ocr(&directory, None, 0, 10, |_| {
            Ok(reads.next().unwrap_or("garbage").into())
        })
        .unwrap();
        assert_eq!(replay.total_frames, 3);
        assert_eq!(replay.frames.len(), 3);
        let first = replay.frames[0].analysis.as_ref().unwrap();
        assert_eq!(first.timestamp_ms, 1100);
        assert_eq!(first.numeric_observations[0].confidence, 0.6);
        let second = replay.frames[1].analysis.as_ref().unwrap();
        assert_eq!(second.numeric_observations[0].value, 1661.0);
        assert_eq!(second.numeric_observations[0].confidence, 0.9);
        let summary = &replay.regions[0];
        assert_eq!(
            (
                summary.frames,
                summary.observed,
                summary.stable,
                summary.ambiguous
            ),
            (3, 2, 1, 1)
        );
        // Paging past the end is explicit, and a recording without ROI needs a config.
        assert!(analyse_recording_with_ocr(&directory, None, 3, 1, |_| Ok(String::new())).is_err());
        std::fs::remove_file(directory.join("frame-000002.png")).unwrap();
        let replay =
            analyse_recording_with_ocr(&directory, Some(config()), 0, 3, |_| Ok("27:42".into()))
                .unwrap();
        assert!(replay.frames[1].error.is_some());
        // Continuity is broken by the missing frame: the third is not stabilized by the first.
        let third = replay.frames[2].analysis.as_ref().unwrap();
        assert_eq!(third.numeric_observations[0].confidence, 0.6);
    }
}
