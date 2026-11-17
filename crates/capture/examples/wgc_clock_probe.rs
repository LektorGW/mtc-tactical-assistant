//! Captures only the passive HUD helper created by this process, never another app.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use mtc_capture::WindowCaptureSession;
    use std::{
        io::{BufRead, BufReader},
        os::windows::process::CommandExt,
        path::Path,
        process::{Child, Command, Stdio},
        time::Duration,
    };
    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetWindowThreadProcessId};

    struct Fixture(Child);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let args: Vec<_> = std::env::args().collect();
    let restart = args.iter().any(|arg| arg == "--restart");
    let iterations = args
        .windows(2)
        .find(|p| p[0] == "--iterations")
        .and_then(|p| p[1].parse::<u32>().ok())
        .unwrap_or(40);
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/examples/hud_fixture.exe");
    let mut fixture = Fixture(
        Command::new(fixture_path)
            .args(["--multi-hud", "--passive", "--seconds", "120"])
            .creation_flags(0x08000000)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut reader = BufReader::new(fixture.0.stdout.take().ok_or("Missing fixture stdout")?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let handle = line.trim().parse::<i64>()?;
    let mut owner = 0;
    unsafe {
        GetWindowThreadProcessId(HWND(handle as *mut _), Some(&mut owner));
    }
    if owner != fixture.0.id() {
        return Err("Refusing to capture a window outside our fixture process".into());
    }
    let mut failures = 0;
    let mut passed_frames = 0;
    let mut capture = WindowCaptureSession::start_with_interval(handle, Duration::from_millis(50))?;
    for i in 0..iterations {
        for j in 0..2 {
            match capture.next_frame(Duration::from_secs(2)) {
                Ok(frame) => {
                    passed_frames += 1;
                    println!(
                        "iteration={i} frame={j} source_100ns={} captured_ms={} dimensions={}x{}",
                        frame.source_timestamp_100ns.unwrap_or(-1),
                        frame.captured_at_ms,
                        frame.image.width(),
                        frame.image.height()
                    );
                }
                Err(error) => {
                    failures += 1;
                    eprintln!("iteration={i} frame={j} capture_error={error}");
                    break;
                }
            }
        }
        if restart {
            eprintln!("iteration={i} before_stop");
            capture.stop()?;
            eprintln!("iteration={i} after_stop");
            if i + 1 < iterations {
                capture =
                    WindowCaptureSession::start_with_interval(handle, Duration::from_millis(50))?;
            }
        }
    }
    capture.stop()?;
    println!("summary iterations={iterations} passed_frames={passed_frames} failures={failures}");
    if failures > 0 {
        return Err(format!("{failures} capture failures").into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    panic!("Windows-only capture probe");
}
