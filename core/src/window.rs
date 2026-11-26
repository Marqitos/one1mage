use crate::renderer::{CLOSE_BUTTON_RADIUS_DIP, Direct2DRenderer, IMAGE_MARGIN_DIP};
use std::{ffi::c_void, io, path::Path};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, ScreenToClient},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::WM_MOUSELEAVE,
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetProcessDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
                HCURSOR, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT,
                HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, IDC_HAND, IsZoomed, LoadCursorW,
                MSG, PostMessageW, PostQuitMessage, RegisterClassW, SIZE_MINIMIZED,
                SW_SHOWMAXIMIZED, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetWindowLongPtrW,
                SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
                WM_ERASEBKGND, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCCREATE,
                WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SIZE, WNDCLASSW,
                WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::{Error, HSTRING, w},
};

const INITIAL_WIDTH: i32 = 1280;
const INITIAL_HEIGHT: i32 = 800;
const RESIZE_BORDER_DIP: f32 = 8.0;
const TITLE_BAR_HEIGHT_DIP: f32 = CLOSE_BUTTON_RADIUS_DIP;

struct AppWindow {
    renderer: Option<Direct2DRenderer>,
    arrow_cursor: HCURSOR,
    hand_cursor: HCURSOR,
    dpi: u32,
    visible: bool,
    close_button_hovered: bool,
    tracking_mouse_leave: bool,
    last_reported_size: Option<(u32, u32, u32)>,
    failure: Option<String>,
}

impl AppWindow {
    fn update_size(&mut self, hwnd: HWND) -> windows::core::Result<()> {
        let (width, height) = client_size(hwnd)?;
        self.dpi = unsafe { GetDpiForWindow(hwnd) }.max(1);
        if width > 0
           && height > 0
           && let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height, self.dpi, self.close_button_hovered)?;
        }
        if self.visible {
            self.report_size(width, height);
        }
        Ok(())
    }

    fn report_size(&mut self, width: u32, height: u32) {
        let size = (width, height, self.dpi);
        if self.last_reported_size == Some(size) {
            return;
        }
        self.last_reported_size = Some(size);

        let ((width_dip, height_dip), (image_width_dip, image_height_dip)) =
            image_area_dimensions(width, height, self.dpi);
        let pixel_scale = self.dpi.max(1) as f32 / 96.0;
        println!(
            "Tamaño de ventana (área cliente): {width}x{height} px ({width_dip:.1}x{height_dip:.1} DIPs); área máxima para imagen (márgenes de 80 DIPs, incluye la zona superior): {:.0}x{:.0} px ({image_width_dip:.1}x{image_height_dip:.1} DIPs)",
            image_width_dip * pixel_scale,
            image_height_dip * pixel_scale,
        );
    }

    fn record_failure(&mut self, error: impl std::fmt::Display) {
        if self.failure.is_none() {
            self.failure = Some(error.to_string());
        }
        unsafe {
            PostQuitMessage(1);
        }
    }
}

pub(crate) fn show(path: &Path, title: &str) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
    }
    let image = image::open(path)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("no se pudo decodificar '{}': {error}", path.display()),
            )
        })?
        .into_rgba8();
    let module = unsafe { GetModuleHandleW(None)? };
    let instance = HINSTANCE(module.0);
    let class_name = w!("One1mageWindowClass");
    let arrow_cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let hand_cursor = unsafe { LoadCursorW(None, IDC_HAND)? };
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: arrow_cursor,
        lpszClassName: class_name,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(Error::from_win32().into());
    }

    let mut state = Box::new(AppWindow {
        renderer: None,
        arrow_cursor,
        hand_cursor,
        dpi: 96,
        visible: false,
        close_button_hovered: false,
        tracking_mouse_leave: false,
        last_reported_size: None,
        failure: None,
    });
    let state_ptr = (&mut *state as *mut AppWindow).cast::<c_void>();
    let window_title = HSTRING::from(title);
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            class_name,
            &window_title,
            WS_OVERLAPPEDWINDOW,
            windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            windows::Win32::UI::WindowsAndMessaging::CW_USEDEFAULT,
            INITIAL_WIDTH,
            INITIAL_HEIGHT,
            None,
            None,
            instance,
            Some(state_ptr.cast_const()),
        )?
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(1);
    let (width, height) = client_size(hwnd)?;
    let renderer = Direct2DRenderer::new(hwnd, width.max(1), height.max(1), dpi, image)?;
    renderer.render(false)?;
    state.renderer = Some(renderer);
    state.dpi = dpi;
    state.visible = true;

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
    }
    state.update_size(hwnd)?;

    let mut message = MSG::default();

    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            return Err(Error::from_win32().into());
        }
        if result.0 == 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    if let Some(error) = state.failure.take() {
        Err(error.into())
    } else {
        Ok(())
    }
}

fn client_size(hwnd: HWND) -> windows::core::Result<(u32, u32)> {
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut rect)?;
    }
    Ok((
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    ))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return LRESULT(1);
        }

        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppWindow;
        if state_ptr.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        match message {
            WM_NCCALCSIZE => LRESULT(0),
            WM_NCHITTEST => {
                if let Some(hit) = hit_test(hwnd, &*state_ptr, lparam) {
                    LRESULT(hit as isize)
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
            WM_SIZE => {
                if wparam.0 as u32 != SIZE_MINIMIZED
                    && let Err(error) = (&mut *state_ptr).update_size(hwnd)
                {
                    (&mut *state_ptr).record_failure(error);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let state = &mut *state_ptr;
                let hovered = close_button_contains(hwnd, state, lparam).unwrap_or(false);
                if hovered != state.close_button_hovered {
                    state.close_button_hovered = hovered;
                    if let Some(renderer) = state.renderer.as_ref()
                        && let Err(error) = renderer.render(hovered)
                    {
                        state.record_failure(error);
                    }
                }
                let cursor = if hovered {
                    state.hand_cursor
                } else {
                    state.arrow_cursor
                };
                let _ = SetCursor(cursor);
                if hovered && !state.tracking_mouse_leave {
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    if let Err(error) = TrackMouseEvent(&mut tracking) {
                        state.record_failure(error);
                    } else {
                        state.tracking_mouse_leave = true;
                    }
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let state = &mut *state_ptr;
                state.tracking_mouse_leave = false;
                if state.close_button_hovered {
                    state.close_button_hovered = false;
                    if let Some(renderer) = state.renderer.as_ref()
                        && let Err(error) = renderer.render(false)
                    {
                        state.record_failure(error);
                    }
                }
                LRESULT(0)
            }
            WM_SETCURSOR
                if lparam.0 as u16 as u32 == HTCLIENT && (&*state_ptr).close_button_hovered =>
            {
                let _ = SetCursor((&*state_ptr).hand_cursor);
                LRESULT(1)
            }
            WM_DPICHANGED => {
                let dpi = ((wparam.0 >> 16) & 0xffff) as u32;
                let suggested = *(lparam.0 as *const RECT);
                (&mut *state_ptr).dpi = dpi.max(1);
                if let Err(error) = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                ) {
                    (&mut *state_ptr).record_failure(error);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if close_button_contains(hwnd, &*state_ptr, lparam).unwrap_or(false) {
                    if let Err(error) = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) {
                        (&mut *state_ptr).record_failure(error);
                    }
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut paint);
                if let Some(renderer) = (&*state_ptr).renderer.as_ref()
                    && let Err(error) = renderer.render((&*state_ptr).close_button_hovered)
                {
                    (&mut *state_ptr).record_failure(error);
                }
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

unsafe fn hit_test(hwnd: HWND, state: &AppWindow, lparam: LPARAM) -> Option<u32> {
    let mut point = POINT {
        x: (lparam.0 as u16 as i16) as i32,
        y: ((lparam.0 >> 16) as u16 as i16) as i32,
    };
    if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
        return None;
    }
    let (width, height) = client_size(hwnd).ok()?;
    let dpi_scale = state.dpi.max(1) as f32 / 96.0;
    let x_dip = point.x as f32 / dpi_scale;
    let y_dip = point.y as f32 / dpi_scale;
    let width_dip = width as f32 / dpi_scale;
    if close_button_contains_dip(x_dip, y_dip, width_dip) {
        return Some(HTCLIENT);
    }

    if !unsafe { IsZoomed(hwnd) }.as_bool() {
        let border = (RESIZE_BORDER_DIP * dpi_scale).ceil() as i32;
        let left = point.x < border;
        let right = point.x >= width as i32 - border;
        let top = point.y < border;
        let bottom = point.y >= height as i32 - border;
        let edge = match (left, right, top, bottom) {
            (true, _, true, _) => Some(HTTOPLEFT),
            (_, true, true, _) => Some(HTTOPRIGHT),
            (true, _, _, true) => Some(HTBOTTOMLEFT),
            (_, true, _, true) => Some(HTBOTTOMRIGHT),
            (true, _, _, _) => Some(HTLEFT),
            (_, true, _, _) => Some(HTRIGHT),
            (_, _, true, _) => Some(HTTOP),
            (_, _, _, true) => Some(HTBOTTOM),
            _ => None,
        };
        if edge.is_some() {
            return edge;
        }
    }

    let title_bar_height_px = (TITLE_BAR_HEIGHT_DIP * dpi_scale).ceil() as i32;
    if point.y < title_bar_height_px {
        return Some(HTCAPTION);
    }
    Some(HTCLIENT)
}

fn image_area_dimensions(width: u32, height: u32, dpi: u32) -> ((f32, f32), (f32, f32)) {
    let dpi_scale = 96.0 / dpi.max(1) as f32;
    let width_dip = width as f32 * dpi_scale;
    let height_dip = height as f32 * dpi_scale;
    (
        (width_dip, height_dip),
        (
            (width_dip - IMAGE_MARGIN_DIP * 2.0).max(0.0),
            (height_dip - IMAGE_MARGIN_DIP * 2.0).max(0.0),
        ),
    )
}

unsafe fn close_button_contains(hwnd: HWND, state: &AppWindow, lparam: LPARAM) -> Option<bool> {
    let (width, _) = client_size(hwnd).ok()?;
    let dpi_scale = 96.0 / state.dpi.max(1) as f32;
    let x = (lparam.0 as u16 as i16) as f32 * dpi_scale;
    let y = ((lparam.0 >> 16) as u16 as i16) as f32 * dpi_scale;
    let width_dip = width as f32 * dpi_scale;
    Some(close_button_contains_dip(x, y, width_dip))
}

fn close_button_contains_dip(x: f32, y: f32, width: f32) -> bool {
    if x < width - CLOSE_BUTTON_RADIUS_DIP
        || x > width
        || !(0.0..=CLOSE_BUTTON_RADIUS_DIP).contains(&y)
    {
        return false;
    }
    let dx = width - x;
    dx * dx + y * y <= CLOSE_BUTTON_RADIUS_DIP * CLOSE_BUTTON_RADIUS_DIP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_area_keeps_margins_and_uses_the_full_window_height() {
        let ((width_dip, height_dip), image_area) = image_area_dimensions(1920, 1080, 144);

        assert_eq!((width_dip, height_dip), (1280.0, 720.0));
        assert_eq!(image_area, (1120.0, 560.0));
    }

    #[test]
    fn close_button_hit_area_is_the_visible_quarter_circle() {
        assert!(close_button_contains_dip(980.0, 20.0, 1000.0));
        assert!(close_button_contains_dip(1000.0, 50.0, 1000.0));
        assert!(!close_button_contains_dip(1000.0, 51.0, 1000.0));
        assert!(!close_button_contains_dip(949.0, 10.0, 1000.0));
    }
}
