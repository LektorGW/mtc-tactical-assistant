//! Consent-gated test recording of the selected visible window into a local
//! directory chosen by the user, plus loading such a recording for offline
//! analysis. Recording is off by default, never persisted as a setting and
//! stops on request, on any limit, when the source window closes or when the
//! panel that shows the "NAGRYWANIE" indicator is closed.
use crate::now_ms;
use mtc_capture::{CaptureBackend, CaptureError, WindowCaptureSession};
use mtc_vision::{
    AnalysisConfig, FrameDecision, RecordingFormat, RecordingLimits, RecordingOptions,
    RecordingSession, RecordingSource, RecordingWriter,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

fn default_jpeg_quality() -> u8 {
    90
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordingRequest {
    /// Explicit checkbox state from this UI session; `false` is always refused.
    consent: bool,
    directory: String,
    handle: i64,
    backend: CaptureBackend,
    format: RecordingFormat,
    #[serde(default = "default_jpeg_quality")]
    jpeg_quality: u8,
    max_fps: f64,
    max_frames: u32,
    max_megabytes: u64,
    max_seconds: u64,
    #[serde(default)]
    config: Option<AnalysisConfig>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    active: bool,
    directory: Option<String>,
    handle: Option<i64>,
    backend: Option<CaptureBackend>,
    format: Option<RecordingFormat>,
    frames: u32,
    bytes: u64,
    skipped: u32,
    started_at_ms: Option<u64>,
    elapsed_ms: u64,
    last_frame_ms: Option<u64>,
    max_frames: u32,
    max_bytes: u64,
    max_duration_ms: u64,
    stop_reason: Option<String>,
    /// Latest transient capture problem (hidden window, timeout); cleared by the next frame.
    warning: Option<String>,
    error: Option<String>,
}

struct ActiveRecording {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

#[derive(Default)]
struct Recorder {
    active: Option<ActiveRecording>,
    status: Arc<Mutex<RecordingStatus>>,
}

fn recorder() -> &'static Mutex<Recorder> {
    static RECORDER: OnceLock<Mutex<Recorder>> = OnceLock::new();
    RECORDER.get_or_init(|| Mutex::new(Recorder::default()))
}

fn snapshot(status: &Arc<Mutex<RecordingStatus>>) -> RecordingStatus {
    let mut current = status.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if current.active {
        if let Some(started) = current.started_at_ms {
            current.elapsed_ms = now_ms().saturating_sub(started);
        }
    }
    current
}

fn update(status: &Arc<Mutex<RecordingStatus>>, change: impl FnOnce(&mut RecordingStatus)) {
    change(&mut status.lock().unwrap_or_else(|e| e.into_inner()));
}

fn limits_from(request: &RecordingRequest) -> Result<RecordingLimits, String> {
    let limits = RecordingLimits {
        max_fps: request.max_fps,
        max_frames: request.max_frames,
        max_bytes: request
            .max_megabytes
            .checked_mul(1024 * 1024)
            .ok_or("Limit rozmiaru nagrania jest zbyt duży")?,
        max_duration_ms: request
            .max_seconds
            .checked_mul(1000)
            .ok_or("Limit czasu nagrania jest zbyt duży")?,
    };
    limits.validate()?;
    Ok(limits)
}

/// Sleeps in short steps so a stop request is honoured within ~50 ms.
fn wait(stop: &AtomicBool, duration: Duration) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::Acquire) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

fn run_recording(
    handle: i64,
    backend: CaptureBackend,
    mut writer: RecordingWriter,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<RecordingStatus>>,
) {
    let interval = Duration::from_millis(writer.session().limits.frame_interval_ms());
    let mut session: Option<WindowCaptureSession> = None;
    let mut gdi_sequence = 0u64;
    let mut last_gdi = None::<Instant>;
    let reason = loop {
        if stop.load(Ordering::Acquire) {
            break "Zatrzymano przez użytkownika".to_string();
        }
        if let Some(reason) = writer.limit_reason(now_ms()) {
            break reason;
        }
        let frame = match backend {
            CaptureBackend::WindowsGraphicsCapture => {
                if session.is_none() {
                    match WindowCaptureSession::start_with_interval(handle, interval) {
                        Ok(started) => session = Some(started),
                        Err(CaptureError::Closed) => {
                            break "Okno źródłowe zostało zamknięte".to_string()
                        }
                        Err(error) => {
                            update(&status, |s| s.warning = Some(error.to_string()));
                            wait(&stop, Duration::from_millis(500));
                            continue;
                        }
                    }
                }
                // A short timeout keeps stop requests and limits responsive.
                match session.as_mut() {
                    Some(active) => active.next_frame(Duration::from_millis(500)),
                    None => continue,
                }
            }
            CaptureBackend::Gdi => {
                if let Some(last) = last_gdi {
                    wait(&stop, interval.saturating_sub(last.elapsed()));
                    if stop.load(Ordering::Acquire) {
                        continue;
                    }
                }
                last_gdi = Some(Instant::now());
                mtc_capture::capture_window_with_backend(handle, backend).map(|mut frame| {
                    // Single GDI captures all report sequence 1; number them here.
                    gdi_sequence += 1;
                    frame.sequence = gdi_sequence;
                    frame
                })
            }
        };
        match frame {
            Ok(frame) => match writer.offer(
                &frame.image,
                frame.captured_at_ms,
                frame.sequence,
                frame.source_timestamp_100ns,
                now_ms(),
            ) {
                Ok(FrameDecision::Recorded(recorded)) => update(&status, |s| {
                    s.frames = recorded.index;
                    s.bytes = writer.session().total_bytes;
                    s.last_frame_ms = Some(recorded.timestamp_ms);
                    s.warning = None;
                }),
                Ok(FrameDecision::Skipped(_)) => update(&status, |s| s.skipped += 1),
                Ok(FrameDecision::LimitReached(reason)) => break reason,
                Err(error) => {
                    update(&status, |s| s.error = Some(error.clone()));
                    break format!("Błąd zapisu nagrania: {error}");
                }
            },
            Err(CaptureError::Timeout) => {}
            Err(CaptureError::Closed) => break "Okno źródłowe zostało zamknięte".to_string(),
            Err(error) if error.requires_session_restart() => {
                if let Some(mut old) = session.take() {
                    let _ = old.stop();
                }
                update(&status, |s| s.warning = Some(error.to_string()));
                wait(&stop, Duration::from_millis(500));
            }
            Err(error) => {
                // Hidden/minimized/resizing window: nothing is written until it is visible.
                update(&status, |s| s.warning = Some(error.to_string()));
                wait(&stop, Duration::from_millis(200));
            }
        }
    };
    if let Some(mut active) = session.take() {
        let _ = active.stop();
    }
    let finished = writer.finish(&reason, now_ms());
    update(&status, |s| {
        s.active = false;
        if let Some(started) = s.started_at_ms {
            s.elapsed_ms = now_ms().saturating_sub(started);
        }
        s.stop_reason = Some(reason);
        if let Err(error) = finished {
            s.error = Some(error);
        }
    });
}

#[tauri::command]
pub async fn recording_start(request: RecordingRequest) -> Result<RecordingStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !request.consent {
            return Err("Nagrywanie wymaga jawnego zaznaczenia zgody w tej sesji".into());
        }
        let limits = limits_from(&request)?;
        let mut recorder = recorder().lock().map_err(|e| e.to_string())?;
        if let Some(active) = recorder.active.take() {
            if !active.worker.is_finished() {
                recorder.active = Some(active);
                return Err("Nagrywanie już trwa; zatrzymaj je przed rozpoczęciem nowego".into());
            }
            let _ = active.worker.join();
        }
        let window_title = mtc_capture::list_windows()
            .ok()
            .and_then(|windows| windows.into_iter().find(|w| w.handle == request.handle))
            .map(|w| w.title);
        if window_title.is_none() {
            return Err("Wybrane okno nie jest widoczne; odśwież listę okien".into());
        }
        let backend_name = match request.backend {
            CaptureBackend::WindowsGraphicsCapture => "wgc",
            CaptureBackend::Gdi => "gdi",
        };
        // Invalid ROI metadata never blocks a consented recording; it is omitted
        // and reported, and offline analysis then needs an explicit configuration.
        let (config, config_warning) = match request.config {
            Some(config) => match mtc_vision::validate_recording_config(&config) {
                Ok(()) => (Some(config), None),
                Err(error) => (None, Some(format!("Nagranie bez metadanych ROI: {error}"))),
            },
            None => (None, None),
        };
        let started_at = now_ms();
        let writer = RecordingWriter::create(
            Path::new(&request.directory),
            RecordingOptions {
                consent: request.consent,
                format: request.format,
                jpeg_quality: request.jpeg_quality,
                limits: limits.clone(),
                source: RecordingSource {
                    handle: Some(request.handle),
                    window_title,
                    backend: backend_name.into(),
                },
                analysis_config: config,
            },
            started_at,
        )?;
        let status = Arc::new(Mutex::new(RecordingStatus {
            active: true,
            directory: Some(writer.directory().display().to_string()),
            handle: Some(request.handle),
            backend: Some(request.backend),
            format: Some(request.format),
            started_at_ms: Some(started_at),
            max_frames: limits.max_frames,
            max_bytes: limits.max_bytes,
            max_duration_ms: limits.max_duration_ms,
            warning: config_warning,
            ..RecordingStatus::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_status = Arc::clone(&status);
        let worker_stop = Arc::clone(&stop);
        let (handle, backend) = (request.handle, request.backend);
        let worker = std::thread::Builder::new()
            .name("mtc-recording".into())
            .spawn(move || run_recording(handle, backend, writer, worker_stop, worker_status))
            .map_err(|e| format!("Nie udało się uruchomić nagrywania: {e}"))?;
        recorder.status = Arc::clone(&status);
        recorder.active = Some(ActiveRecording { stop, worker });
        Ok(snapshot(&status))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn recording_stop() -> Result<RecordingStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let (active, status) = {
            let mut recorder = recorder().lock().map_err(|e| e.to_string())?;
            (recorder.active.take(), Arc::clone(&recorder.status))
        };
        if let Some(active) = active {
            active.stop.store(true, Ordering::Release);
            active
                .worker
                .join()
                .map_err(|_| "Wątek nagrywania zakończył się błędem".to_string())?;
        }
        Ok(snapshot(&status))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn recording_status() -> Result<RecordingStatus, String> {
    let recorder = recorder().lock().map_err(|e| e.to_string())?;
    Ok(snapshot(&recorder.status))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedRecordingFrame {
    index: u32,
    file: String,
    path: String,
    timestamp_ms: u64,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedRecording {
    directory: String,
    session: RecordingSession,
    frames: Vec<LoadedRecordingFrame>,
}

#[tauri::command]
pub async fn recording_load(path: String) -> Result<LoadedRecording, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (directory, session) = mtc_vision::load_recording(Path::new(&path))?;
        let frames = session
            .frames
            .iter()
            .map(|frame| LoadedRecordingFrame {
                index: frame.index,
                file: frame.file.clone(),
                path: directory.join(&frame.file).display().to_string(),
                timestamp_ms: frame.timestamp_ms,
                width: frame.width,
                height: frame.height,
            })
            .collect();
        Ok(LoadedRecording {
            directory: directory.display().to_string(),
            session,
            frames,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn analyse_recording(
    path: String,
    config: Option<AnalysisConfig>,
    start: usize,
    count: usize,
) -> Result<mtc_vision::RecordingAnalysis, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mtc_vision::analyse_recording(Path::new(&path), config, start, count)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(consent: bool) -> RecordingRequest {
        serde_json::from_value(serde_json::json!({
            "consent": consent,
            "directory": "C:/nagrania",
            "handle": 7,
            "backend": "wgc",
            "format": "png",
            "maxFps": 1.0,
            "maxFrames": 300,
            "maxMegabytes": 500,
            "maxSeconds": 300
        }))
        .unwrap()
    }

    #[test]
    fn converts_ui_limits_to_native_units_and_rejects_overflow() {
        let limits = limits_from(&request(true)).unwrap();
        assert_eq!(limits.max_bytes, 500 * 1024 * 1024);
        assert_eq!(limits.max_duration_ms, 300_000);
        let mut huge = request(true);
        huge.max_megabytes = u64::MAX;
        assert!(limits_from(&huge).is_err());
        let mut fast = request(true);
        fast.max_fps = 60.0;
        assert!(limits_from(&fast).is_err());
    }

    #[test]
    fn request_rejects_unknown_fields_so_consent_cannot_be_smuggled_as_a_setting() {
        assert!(
            serde_json::from_value::<RecordingRequest>(serde_json::json!({
                "consent": true, "rememberConsent": true, "directory": "C:/x", "handle": 1,
                "backend": "wgc", "format": "png", "maxFps": 1.0, "maxFrames": 1,
                "maxMegabytes": 1, "maxSeconds": 1
            }))
            .is_err()
        );
        assert!(!request(false).consent);
    }

    #[test]
    fn refuses_to_start_without_consent_before_touching_the_disk() {
        let error = tauri::async_runtime::block_on(recording_start(request(false))).unwrap_err();
        assert!(error.contains("zgody"), "{error}");
        assert!(!recording_status().unwrap().active);
    }

    #[test]
    fn stop_request_interrupts_waiting_promptly() {
        let stop = AtomicBool::new(true);
        let started = Instant::now();
        wait(&stop, Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
