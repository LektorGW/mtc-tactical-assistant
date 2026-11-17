//! Visible test-only HUD. Captures can target this fixture without any game interaction.
#[cfg(windows)]
mod fixture {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::{Duration, Instant};
    use windows::{
        core::w,
        Win32::{
            Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::*,
            System::LibraryLoader::GetModuleHandleW,
            UI::{HiDpi::*, WindowsAndMessaging::*},
        },
    };
    static MULTI_HUD: AtomicBool = AtomicBool::new(false);
    static AUTO_HUD: AtomicBool = AtomicBool::new(false);
    static SAME_COLOR: AtomicBool = AtomicBool::new(false);
    static TICK: AtomicU32 = AtomicU32::new(0);

    unsafe fn draw_text(dc: HDC, text: &str, x: i32, y: i32, size: i32) {
        let font = CreateFontW(
            size,
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            ANSI_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let text: Vec<u16> = text.encode_utf16().collect();
        let _ = TextOutW(dc, x, y, &text);
        SelectObject(dc, old);
        let _ = DeleteObject(HGDIOBJ(font.0));
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TIMER => {
                TICK.fetch_add(1, Ordering::AcqRel);
                let _ = InvalidateRect(Some(window), None, false);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(window, &mut paint);
                let mut rect = RECT::default();
                let _ = GetClientRect(window, &mut rect);
                FillRect(dc, &rect, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                SetTextColor(dc, COLORREF(0));
                SetBkColor(dc, COLORREF(0xffffff));
                draw_text(
                    dc,
                    "27:42",
                    if AUTO_HUD.load(Ordering::Acquire) {
                        400
                    } else {
                        40
                    },
                    50,
                    64,
                );
                if MULTI_HUD.load(Ordering::Acquire) {
                    if AUTO_HUD.load(Ordering::Acquire) {
                        draw_text(dc, "Heading: 138", 40, 130, 38);
                        draw_text(dc, "Vehicle: BM-21 Grad", 40, 180, 28);
                        draw_text(dc, "Ammo: 9M22/M21 HE-Frag", 40, 215, 24);
                        draw_text(dc, "Map: Arctic Airbase", 40, 465, 28);
                        draw_text(dc, "Speed: 12", 40, 510, 28);
                        draw_text(dc, "Altitude: 42", 40, 555, 28);
                    } else {
                        draw_text(dc, "138", 40, 130, 48);
                        draw_text(dc, "BM-21 Grad", 40, 190, 32);
                    }
                    let map = CreateSolidBrush(COLORREF(0xd0d0d0));
                    FillRect(
                        dc,
                        &RECT {
                            left: 270,
                            top: 240,
                            right: 620,
                            bottom: 440,
                        },
                        map,
                    );
                    let _ = DeleteObject(HGDIOBJ(map.0));
                    // Deterministic textured terrain for map-view matching tests.
                    for row in 0..10 {
                        for column in 0..14 {
                            let gray =
                                40 + ((row * 37 + column * 61 + row * column * 7) % 170) as u32;
                            let brush =
                                CreateSolidBrush(COLORREF(gray | (gray << 8) | (gray << 16)));
                            FillRect(
                                dc,
                                &RECT {
                                    left: 270 + column * 25,
                                    top: 240 + row * 20,
                                    right: 270 + (column + 1) * 25,
                                    bottom: 240 + (row + 1) * 20,
                                },
                                brush,
                            );
                            let _ = DeleteObject(HGDIOBJ(brush.0));
                        }
                    }
                    let own = CreateSolidBrush(COLORREF(0x00ff00));
                    FillRect(
                        dc,
                        &RECT {
                            left: 310,
                            top: 270,
                            right: 322,
                            bottom: 282,
                        },
                        own,
                    );
                    let _ = DeleteObject(HGDIOBJ(own.0));
                    let target =
                        CreateSolidBrush(COLORREF(if SAME_COLOR.load(Ordering::Acquire) {
                            0x00ff00
                        } else {
                            0x0000ff
                        }));
                    FillRect(
                        dc,
                        &RECT {
                            left: 500,
                            top: 370,
                            right: 512,
                            bottom: 382,
                        },
                        target,
                    );
                    let _ = DeleteObject(HGDIOBJ(target.0));
                    if SAME_COLOR.load(Ordering::Acquire) {
                        // Only the opt-in selection fixture adds a third matching
                        // component; baseline HUD tests retain their original pixels.
                        let distractor = CreateSolidBrush(COLORREF(0x00ff00));
                        FillRect(
                            dc,
                            &RECT {
                                left: 410,
                                top: 310,
                                right: 422,
                                bottom: 322,
                            },
                            distractor,
                        );
                        let _ = DeleteObject(HGDIOBJ(distractor.0));
                    }
                }
                // Changes only an irrelevant corner, ensuring the fixture also tests new WGC frames.
                let pulse = CreateSolidBrush(COLORREF(
                    if TICK.load(Ordering::Acquire).is_multiple_of(2) {
                        0
                    } else {
                        0xffffff
                    },
                ));
                FillRect(
                    dc,
                    &RECT {
                        left: 5,
                        top: rect.bottom - 15,
                        right: 15,
                        bottom: rect.bottom - 5,
                    },
                    pulse,
                );
                let _ = DeleteObject(HGDIOBJ(pulse.0));
                let _ = EndPaint(window, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    pub fn run() {
        unsafe {
            let args: Vec<String> = std::env::args().collect();
            if args.iter().any(|arg| arg == "--inspect-overlay") {
                let window = FindWindowW(None, w!("MTC Tactical Overlay")).unwrap();
                let style = GetWindowLongPtrW(window, GWL_EXSTYLE) as u32;
                println!(
                    "{{\"clickThrough\":{},\"layered\":{}}}",
                    style & WS_EX_TRANSPARENT.0 != 0,
                    style & WS_EX_LAYERED.0 != 0
                );
                return;
            }
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let auto = args.iter().any(|arg| arg == "--auto-hud");
            let multi = auto || args.iter().any(|arg| arg == "--multi-hud");
            let passive = args.iter().any(|arg| arg == "--passive");
            MULTI_HUD.store(multi, Ordering::Release);
            AUTO_HUD.store(auto, Ordering::Release);
            SAME_COLOR.store(
                args.iter().any(|arg| arg == "--same-color"),
                Ordering::Release,
            );
            let module = GetModuleHandleW(None).unwrap();
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.into(),
                lpszClassName: w!("MtcHudFixture"),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let window = CreateWindowExW(
                if passive {
                    WS_EX_TOPMOST | WS_EX_NOACTIVATE
                } else {
                    WS_EX_TOPMOST
                },
                class.lpszClassName,
                w!("MTC HUD fixture"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                80,
                80,
                if auto {
                    1000
                } else if multi {
                    680
                } else {
                    480
                },
                if auto {
                    700
                } else if multi {
                    520
                } else {
                    260
                },
                None,
                None,
                Some(module.into()),
                None,
            )
            .unwrap();
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            let _ = UpdateWindow(window);
            SetTimer(Some(window), 1, 100, None);
            println!("{}", window.0 as i64);
            std::io::stdout().flush().unwrap();
            let seconds = args
                .windows(2)
                .find(|pair| pair[0] == "--seconds")
                .and_then(|pair| pair[1].parse::<u64>().ok())
                .unwrap_or(45);
            let deadline = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < deadline && IsWindow(Some(window)).as_bool() {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if IsWindow(Some(window)).as_bool() {
                let _ = KillTimer(Some(window), 1);
                let _ = DestroyWindow(window);
            }
            let _ = UnregisterClassW(class.lpszClassName, Some(module.into()));
        }
    }
}
#[cfg(windows)]
fn main() {
    fixture::run();
}
#[cfg(not(windows))]
fn main() {
    panic!("Windows-only HUD fixture");
}
