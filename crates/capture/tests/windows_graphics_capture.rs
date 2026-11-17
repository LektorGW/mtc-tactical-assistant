#![cfg(windows)]
//! This is a real interactive-desktop integration test capturing only a window created here.
//! Never targets Roblox, the user's active app, or stores images.
use mtc_capture::{CaptureBackend, CaptureError, WindowCaptureSession};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{Dwm::DwmFlush, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

static PHASE: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(window, &mut paint);
        let mut rect = RECT::default();
        let _ = GetClientRect(window, &mut rect);
        FillRect(dc, &rect, HBRUSH(GetStockObject(WHITE_BRUSH).0));
        let red = CreateSolidBrush(COLORREF(0x0000ff));
        FillRect(
            dc,
            &RECT {
                left: 10,
                top: 10,
                right: 25,
                bottom: 25,
            },
            red,
        );
        let _ = DeleteObject(HGDIOBJ(red.0));
        let blue = CreateSolidBrush(COLORREF(0xff0000));
        FillRect(
            dc,
            &RECT {
                left: 30,
                top: 10,
                right: 45,
                bottom: 25,
            },
            blue,
        );
        let _ = DeleteObject(HGDIOBJ(blue.0));
        let phase = PHASE.load(Ordering::Acquire);
        let animation = CreateSolidBrush(COLORREF(if phase.is_multiple_of(2) {
            0x00ff00
        } else {
            0x000000
        }));
        FillRect(
            dc,
            &RECT {
                left: 60,
                top: 40,
                right: 90,
                bottom: 70,
            },
            animation,
        );
        let _ = DeleteObject(HGDIOBJ(animation.0));
        let _ = EndPaint(window, &paint);
        return LRESULT(0);
    }
    DefWindowProcW(window, message, wparam, lparam)
}

enum Command {
    Resize,
    Minimize,
    Restore,
    Hide,
    Show,
    Close,
}
struct Fixture {
    handle: i64,
    commands: mpsc::Sender<(Command, mpsc::SyncSender<()>)>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn create() -> Self {
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let (commands, command_rx) = mpsc::channel::<(Command, mpsc::SyncSender<()>)>();
        let worker = thread::spawn(move || unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let module = GetModuleHandleW(None).unwrap();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: w!("MtcWgcFixtureTest"),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let window = CreateWindowExW(
                WS_EX_TOPMOST,
                class.lpszClassName,
                w!("MTC WGC test fixture"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                80,
                80,
                400,
                240,
                None,
                None,
                Some(module.into()),
                None,
            )
            .unwrap();
            let _ = UpdateWindow(window);
            DwmFlush().unwrap();
            ready.send(window.0 as i64).unwrap();
            let mut tick = Instant::now();
            let mut open = true;
            while open {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if let Ok((command, ack)) = command_rx.try_recv() {
                    match command {
                        Command::Resize => {
                            SetWindowPos(
                                window,
                                None,
                                0,
                                0,
                                560,
                                320,
                                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                            )
                            .unwrap();
                        }
                        Command::Minimize => {
                            let _ = ShowWindow(window, SW_MINIMIZE);
                        }
                        Command::Restore => {
                            let _ = ShowWindow(window, SW_RESTORE);
                        }
                        Command::Hide => {
                            let _ = ShowWindow(window, SW_HIDE);
                        }
                        Command::Show => {
                            let _ = ShowWindow(window, SW_SHOW);
                        }
                        Command::Close => {
                            DestroyWindow(window).unwrap();
                            open = false;
                        }
                    }
                    if open {
                        let _ = InvalidateRect(Some(window), None, false);
                        let _ = UpdateWindow(window);
                        DwmFlush().unwrap();
                    }
                    let _ = ack.send(());
                }
                if open && tick.elapsed() >= Duration::from_millis(100) {
                    PHASE.fetch_add(1, Ordering::AcqRel);
                    let _ = InvalidateRect(Some(window), None, false);
                    tick = Instant::now();
                }
                thread::sleep(Duration::from_millis(5));
            }
            UnregisterClassW(class.lpszClassName, Some(module.into())).unwrap();
        });
        Self {
            handle: ready_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            commands,
            worker: Some(worker),
        }
    }
    fn command(&self, command: Command) {
        let (ack, ack_rx) = mpsc::sync_channel(1);
        self.commands.send((command, ack)).unwrap();
        ack_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    fn client_size(&self) -> (u32, u32) {
        unsafe {
            let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let mut rect = RECT::default();
            GetClientRect(HWND(self.handle as *mut _), &mut rect).unwrap();
            SetThreadDpiAwarenessContext(previous);
            (
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            )
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let (ack, _) = mpsc::sync_channel(1);
            let _ = self.commands.send((Command::Close, ack));
            let _ = worker.join();
        }
    }
}

fn assert_client_frame(frame: &mtc_capture::CapturedFrame, expected: (u32, u32)) {
    assert_eq!(frame.backend, CaptureBackend::WindowsGraphicsCapture);
    assert_eq!(
        frame.image.dimensions(),
        expected,
        "Capture must exclude titlebar and frame"
    );
    assert_eq!(
        frame.image.get_pixel(15, 15).0[..3],
        [255, 0, 0],
        "Red fixture marker must stay at client (15,15)"
    );
    assert_eq!(
        frame.image.get_pixel(35, 15).0[..3],
        [0, 0, 255],
        "Blue fixture marker must stay at client (35,15)"
    );
    assert!(frame.source_timestamp_100ns.unwrap() > 0);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(now_ms.saturating_sub(frame.captured_at_ms) <= 1_000);
}

fn stable_client_frame(
    capture: &mut WindowCaptureSession,
    fixture: &Fixture,
) -> mtc_capture::CapturedFrame {
    // Win32 resize/show can expose a transitional blank surface before WM_PAINT reaches DWM.
    // Wait for our known painted pixels; persistent coordinate offsets must still fail.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let frame = capture.next_frame(Duration::from_secs(1)).unwrap();
        if frame.image.dimensions() == fixture.client_size()
            && frame.image.get_pixel(15, 15).0[..3] == [255, 0, 0]
            && frame.image.get_pixel(35, 15).0[..3] == [0, 0, 255]
        {
            assert_client_frame(&frame, fixture.client_size());
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "Client surface did not stabilize after painting: dimensions={:?}, red={:?}",
            frame.image.dimensions(),
            frame.image.get_pixel(15, 15)
        );
    }
}

#[test]
fn captures_client_resizes_and_handles_window_lifecycle() {
    let mut fixture = Fixture::create();
    let mut capture =
        WindowCaptureSession::start_with_interval(fixture.handle, Duration::from_millis(66))
            .expect("WGC needs a Windows interactive desktop and supported graphics driver");
    assert_eq!(capture.handle(), fixture.handle);
    let first = stable_client_frame(&mut capture, &fixture);
    let second = capture.next_frame(Duration::from_secs(2)).unwrap();
    assert!(second.sequence > first.sequence);
    assert!(second.source_timestamp_100ns > first.source_timestamp_100ns);
    fixture.command(Command::Resize);
    let _ = stable_client_frame(&mut capture, &fixture);
    fixture.command(Command::Minimize);
    assert_eq!(
        capture.next_frame(Duration::from_millis(200)).unwrap_err(),
        CaptureError::Minimized
    );
    assert!(!mtc_capture::list_windows()
        .unwrap()
        .iter()
        .any(|w| w.handle == fixture.handle));
    fixture.command(Command::Restore);
    thread::sleep(Duration::from_millis(150));
    let _ = stable_client_frame(&mut capture, &fixture);
    fixture.command(Command::Hide);
    assert_eq!(
        capture.next_frame(Duration::from_millis(200)).unwrap_err(),
        CaptureError::Hidden
    );
    assert!(!mtc_capture::list_windows()
        .unwrap()
        .iter()
        .any(|w| w.handle == fixture.handle));
    fixture.command(Command::Show);
    thread::sleep(Duration::from_millis(150));
    let _ = stable_client_frame(&mut capture, &fixture);
    capture.stop().unwrap();
    assert_eq!(
        capture.next_frame(Duration::from_millis(200)).unwrap_err(),
        CaptureError::Stopped
    );
    capture.stop().unwrap();
    let mut slow =
        WindowCaptureSession::start_with_interval(fixture.handle, Duration::from_secs(4)).unwrap();
    // Even the supported 0.25 FPS setting must deliver its first source frame promptly.
    let _ = slow.next_frame(Duration::from_secs(2)).unwrap();
    slow.stop().unwrap();
    // Below 1 FPS the two-buffer pool must keep flowing between copies. Otherwise
    // each copy reads a surface queued right after the previous one, as old as
    // the interval, and the 1 s freshness limit rejects every later frame.
    let mut low_fps =
        WindowCaptureSession::start_with_interval(fixture.handle, Duration::from_millis(1500))
            .unwrap();
    let mut previous = low_fps.next_frame(Duration::from_secs(2)).unwrap().sequence;
    for _ in 0..2 {
        let frame = low_fps
            .next_frame(Duration::from_secs(4))
            .expect("Low-FPS capture must keep delivering fresh frames");
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(frame.sequence > previous);
        assert!(
            now_ms.saturating_sub(frame.captured_at_ms) <= 400,
            "Low-FPS frame is {} ms old",
            now_ms.saturating_sub(frame.captured_at_ms)
        );
        previous = frame.sequence;
    }
    low_fps.stop().unwrap();
    let mut capture = WindowCaptureSession::start(fixture.handle).unwrap();
    let _ = capture.next_frame(Duration::from_secs(2)).unwrap();
    fixture.command(Command::Close);
    fixture.worker.take().unwrap().join().unwrap();
    assert_eq!(
        capture.next_frame(Duration::from_millis(200)).unwrap_err(),
        CaptureError::Closed
    );
    capture.stop().unwrap();
    assert!(matches!(
        WindowCaptureSession::start(fixture.handle),
        Err(CaptureError::Closed)
    ));
}
