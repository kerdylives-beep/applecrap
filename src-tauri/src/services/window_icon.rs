//! Sharp window icons on Windows.
//!
//! Tauri hands each window a single image from the icon file (the first one),
//! and Windows stretches it to whatever size the title bar, taskbar or Alt+Tab
//! wants, which blurs it. Loading the icon from the exe's resources at the
//! exact size Windows asks for picks the image drawn for that size instead.

/// The id tauri-build gives the app icon in the exe's resources.
#[cfg(windows)]
const APP_ICON_ID: u16 = 32512;

/// Gives `window` its title-bar and taskbar icons at the right sizes for the
/// monitor it is on. Call again when the window's scale factor changes.
#[cfg(windows)]
pub fn apply(window: &tauri::WebviewWindow) {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            System::LibraryLoader::GetModuleHandleW,
            UI::{
                HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
                WindowsAndMessaging::{
                    LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
                    LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, WM_SETICON,
                },
            },
        },
    };

    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(handle.0 as _);
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let dpi = GetDpiForWindow(hwnd);
        for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
            let size = GetSystemMetricsForDpi(metric, dpi);
            // MAKEINTRESOURCE: a resource id passed where a name would go.
            let id = PCWSTR(APP_ICON_ID as usize as *const u16);
            if let Ok(icon) = LoadImageW(Some(module.into()), id, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) {
                SendMessageW(hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
            }
        }
    }
}

#[cfg(not(windows))]
pub fn apply(_window: &tauri::WebviewWindow) {}

/// Applies the icons now and again whenever the window moves to a monitor
/// with a different scale.
pub fn keep_sharp(window: &tauri::WebviewWindow) {
    apply(window);
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::ScaleFactorChanged { .. } = event {
            apply(&handle);
        }
    });
}
