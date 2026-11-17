//! Local, consent-driven capture of a selected visible window using public Windows APIs.
//! No game memory access, hooks, injection, input automation, network, or recording.

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureWindow {
    pub handle: i64,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureBackend {
    #[serde(rename = "wgc")]
    WindowsGraphicsCapture,
    #[serde(rename = "gdi")]
    Gdi,
}

/// Pixels start at the client area's (0, 0), excluding borders and title bar.
/// captured_at_ms is the source time, not the time at which a caller polls it.
#[derive(Debug)]
pub struct CapturedFrame {
    pub image: RgbaImage,
    pub backend: CaptureBackend,
    pub captured_at_ms: u64,
    pub source_timestamp_100ns: Option<i64>,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Closed,
    Hidden,
    Minimized,
    Unsupported,
    Timeout,
    Stopped,
    InvalidSize,
    GeometryChanged,
    FutureFrame {
        qpc_now: i64,
        qpc_frequency: i64,
        source_timestamp_100ns: i64,
        delta_100ns: i128,
    },
    Backend(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::FutureFrame {
            qpc_now,
            qpc_frequency,
            source_timestamp_100ns,
            delta_100ns,
        } = self
        {
            return write!(f, "Przyszła klatka WGC oczekuje lub jest odrzucona: qpc_now={qpc_now}, qpc_frequency={qpc_frequency}, source_timestamp_100ns={source_timestamp_100ns}, delta_100ns={delta_100ns}");
        }
        f.write_str(match self {
            Self::Closed => "Okno przechwytywania zostało zamknięte",
            Self::Hidden => "Okno przechwytywania jest ukryte",
            Self::Minimized => "Okno przechwytywania jest zminimalizowane",
            Self::Unsupported => "Windows Graphics Capture jest niedostępne na tym systemie",
            Self::Timeout => "Brak nowej, aktualnej klatki w wyznaczonym czasie",
            Self::Stopped => "Przechwytywanie zostało zatrzymane",
            Self::InvalidSize => "Nieobsługiwany rozmiar obrazu",
            Self::GeometryChanged => "Zmieniła się geometria okna; oczekiwanie na zgodną klatkę",
            Self::Backend(message) => message,
            Self::FutureFrame { .. } => unreachable!("Handled diagnostic above"),
        })
    }
}

impl std::error::Error for CaptureError {}

impl CaptureError {
    /// A persistent session that returned this error will not recover by itself
    /// (lost GPU device, dead worker, closed source). The consumer should drop it
    /// and start a new one on the next request. Visibility, geometry and timeout
    /// states are transient and must keep the session.
    pub fn requires_session_restart(&self) -> bool {
        matches!(
            self,
            Self::Closed | Self::Stopped | Self::Unsupported | Self::InvalidSize | Self::Backend(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unrecoverable_errors_require_a_new_capture_session() {
        for error in [
            CaptureError::Closed,
            CaptureError::Stopped,
            CaptureError::Backend("DXGI_ERROR_DEVICE_REMOVED".into()),
        ] {
            assert!(error.requires_session_restart(), "{error}");
        }
        for error in [
            CaptureError::Hidden,
            CaptureError::Minimized,
            CaptureError::Timeout,
            CaptureError::GeometryChanged,
        ] {
            assert!(!error.requires_session_restart(), "{error}");
        }
    }
}

pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(windows)]
mod native;
#[cfg(windows)]
mod wgc;
#[cfg(windows)]
pub use native::{capture_window, cursor_client_position, list_windows};
#[cfg(windows)]
pub use wgc::WindowCaptureSession;

/// Explicitly selects the backend. A WGC failure never silently switches to GDI.
/// For periodic analysis retain a WindowCaptureSession instead of restarting WGC.
#[cfg(windows)]
pub fn capture_window_with_backend(
    handle: i64,
    backend: CaptureBackend,
) -> Result<CapturedFrame, CaptureError> {
    match backend {
        CaptureBackend::WindowsGraphicsCapture => {
            let mut session = WindowCaptureSession::start(handle)?;
            let frame = session.next_frame(std::time::Duration::from_secs(2));
            session.stop()?;
            frame
        }
        CaptureBackend::Gdi => {
            let captured_at_ms = unix_time_ms();
            Ok(CapturedFrame {
                image: native::capture_gdi(handle)?,
                backend,
                captured_at_ms,
                source_timestamp_100ns: None,
                sequence: 1,
            })
        }
    }
}
