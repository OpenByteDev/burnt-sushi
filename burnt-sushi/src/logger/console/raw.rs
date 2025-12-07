// Modified from https://github.com/Freaky/Compactor/blob/67a72255ee4e72ff86224cf812a4c8ea07f885a6/src/console.rs

// Helper functions for handling the Windows console from a GUI context.
//
// Windows subsystem applications must explicitly attach to an existing console
// before stdio works, and if not available, create their own if they wish to
// print anything.
//
// These functions enable that, primarily for the purposes of displaying Rust
// panics.

use winapi::um::consoleapi::AllocConsole;
use winapi::um::wincon::{ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetConsoleWindow};
use winapi::um::winuser::SW_HIDE;
use winapi::um::winuser::SW_SHOW;
use winapi::um::winuser::ShowWindow;

/// Check if we're attached to an existing Windows console
pub fn is_attached() -> bool {
    unsafe { !GetConsoleWindow().is_null() }
}

/// Try to attach to an existing Windows console, if necessary.
///
/// It's normally a no-brainer to call this - it just makes info! and friends
/// work as expected, without cluttering the screen with a console in the general
/// case.
pub fn attach() -> bool {
    if is_attached() {
        return true;
    }

    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 }
}

/// Try to allocate ourselves a new console.
pub fn alloc() -> bool {
    unsafe { AllocConsole() != 0 }
}

/// Free any allocated console, if any.
pub fn free() {
    unsafe { FreeConsole() };
}

pub fn showhide_console(show: bool) {
    let hwnd = unsafe { GetConsoleWindow() };
    if !hwnd.is_null() {
        unsafe {
            ShowWindow(hwnd, if show { SW_SHOW } else { SW_HIDE });
        }
    }
}

pub fn show_console() {
    showhide_console(true);
}
pub fn hide_console() {
    showhide_console(false);
}
