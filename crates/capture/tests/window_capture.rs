#![cfg(windows)]
use windows::{
    core::w,
    Win32::{
        Graphics::{Dwm::DwmFlush, Gdi::*},
        UI::WindowsAndMessaging::*,
    },
};
#[test]
fn captures_visible_window_and_rejects_destroyed_handle() {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            w!("STATIC"),
            w!("MTC capture test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            400,
            200,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let _ = UpdateWindow(hwnd);
        let dc = GetDC(Some(hwnd));
        PatBlt(dc, 0, 0, 300, 120, WHITENESS).unwrap();
        ReleaseDC(Some(hwnd), dc);
        DwmFlush().unwrap();
        let captured = mtc_capture::capture_window(hwnd.0 as i64).unwrap();
        assert!(captured.width() > 300 && captured.height() > 100);
        assert!(
            captured.get_pixel(100, 100).0[0] > 240,
            "Captured frame should include the test window's white area"
        );
        // Reading the cursor relative to the client area never moves it.
        let (x, y, inside) = mtc_capture::cursor_client_position(hwnd.0 as i64).unwrap();
        assert_eq!(
            inside,
            x >= 0 && y >= 0 && x < captured.width() as i32 && y < captured.height() as i32
        );
        DestroyWindow(hwnd).unwrap();
        assert!(mtc_capture::capture_window(hwnd.0 as i64).is_err());
        assert!(mtc_capture::cursor_client_position(hwnd.0 as i64).is_err());
    }
}
