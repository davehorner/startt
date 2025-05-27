// src/main.rs
#[cfg(not(feature = "uses_etw"))]
#[allow(unused_imports)]
#[cfg(feature = "uses_etw")]
use ferrisetw::parser::Parser;
#[cfg(feature = "uses_etw")]
use ferrisetw::trace::UserTrace;
#[cfg(feature = "uses_etw")]
use ferrisetw::{EventRecord, SchemaLocator};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use widestring::U16CString;
use winapi::shared::minwindef::FALSE;
use winapi::shared::windef::HWND;
use winapi::shared::windef::{HMONITOR, POINT, RECT};
// Window hook for automatic grid eviction on window destroy

// Make the publisher globally accessible
use winapi::um::handleapi::CloseHandle;
// use winapi::um::psapi::GetProcessImageFileNameW;
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next, TH32CS_SNAPPROCESS,
};
use winapi::um::winnt::HANDLE;
// use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use winapi::um::winuser::SWP_NOSIZE;
use winapi::um::winuser::SWP_NOZORDER;
use winapi::um::winuser::SetWindowPos;
use winapi::um::winuser::ShowWindow;
use winapi::um::winuser::{
    EnumDisplayMonitors, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use winapi::um::winuser::{EnumWindows, GetWindowThreadProcessId};

unsafe extern "system" {
    fn WaitForInputIdle(hProcess: HANDLE, dwMilliseconds: u32) -> u32;
}

pub fn flash_topmost(hwnd: HWND, duration_ms: u64) {
    use winapi::um::winuser::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos,
    };
    unsafe {
        // Set topmost
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        // Wait for the duration
        std::thread::sleep(std::time::Duration::from_millis(duration_ms));
        // Restore to not topmost
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

pub fn hide_window_title_bar(hwnd: HWND) {
    use winapi::um::winuser::{
        GWL_STYLE, GetWindowLongW, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongW, SetWindowPos, WS_CAPTION, WS_SYSMENU,
    };
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        // Only clear WS_CAPTION and WS_SYSMENU bits, leave others untouched
        let new_style = style & !(WS_CAPTION as i32 | WS_SYSMENU as i32);
        if new_style != style {
            SetWindowLongW(hwnd, GWL_STYLE, new_style);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
}

pub fn hide_window_border(hwnd: HWND) {
    use winapi::um::winuser::{
        GWL_STYLE, GetWindowLongW, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongW, SetWindowPos, WS_BORDER, WS_THICKFRAME,
    };
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        // Only clear WS_THICKFRAME and WS_BORDER bits, leave others untouched
        let new_style = style & !(WS_THICKFRAME as i32 | WS_BORDER as i32);
        if new_style != style {
            SetWindowLongW(hwnd, GWL_STYLE, new_style);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
}

use winapi::um::winuser::{FindWindowExW, SW_HIDE, SW_SHOW};

pub fn hide_taskbar_on_monitor(monitor_index: i32) {
    unsafe {
        let class_name = if monitor_index == 0 {
            U16CString::from_str("Shell_TrayWnd").unwrap()
        } else {
            U16CString::from_str("Shell_SecondaryTrayWnd").unwrap()
        };
        // Try to find the taskbar window for the given monitor
        let mut hwnd = FindWindowExW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            class_name.as_ptr(),
            std::ptr::null(),
        );
        // For secondary monitors, there may be multiple taskbars, so iterate
        let mut count = 0;
        while !hwnd.is_null() {
            // Optionally, check which monitor this taskbar is on
            // For now, just hide all found for this class
            ShowWindow(hwnd, SW_HIDE);
            count += 1;
            hwnd = FindWindowExW(
                std::ptr::null_mut(),
                hwnd,
                class_name.as_ptr(),
                std::ptr::null(),
            );
        }
        println!(
            "Tried to hide {} taskbar window(s) for monitor {}",
            count, monitor_index
        );
    }
}

pub fn show_taskbar_on_monitor(monitor_index: i32) {
    unsafe {
        let class_name = if monitor_index == 0 {
            U16CString::from_str("Shell_TrayWnd").unwrap()
        } else {
            U16CString::from_str("Shell_SecondaryTrayWnd").unwrap()
        };
        let mut hwnd = FindWindowExW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            class_name.as_ptr(),
            std::ptr::null(),
        );
        let mut count = 0;
        while !hwnd.is_null() {
            ShowWindow(hwnd, SW_SHOW);
            count += 1;
            hwnd = FindWindowExW(
                std::ptr::null_mut(),
                hwnd,
                class_name.as_ptr(),
                std::ptr::null(),
            );
        }
        println!(
            "Tried to show {} taskbar window(s) for monitor {}",
            count, monitor_index
        );
    }
}

// Helper to get monitor RECT by index (0 = primary)
pub fn get_monitor_rect(monitor_index: i32, use_full_area: bool) -> RECT {
    let rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let found = Arc::new(Mutex::new(false));
    let rect_arc = Arc::new(Mutex::new(rect));
    let count = Arc::new(Mutex::new(0));
    unsafe extern "system" fn enum_monitor_proc(
        hmonitor: HMONITOR,
        _hdc: winapi::shared::windef::HDC,
        _lprc: *mut RECT,
        lparam: winapi::shared::minwindef::LPARAM,
    ) -> i32 {
        unsafe {
            let (target, found, rect_arc, count, use_full_area): &mut (
                i32,
                Arc<Mutex<bool>>,
                Arc<Mutex<RECT>>,
                Arc<Mutex<i32>>,
                bool,
            ) = &mut *(lparam
                as *mut (
                    i32,
                    Arc<Mutex<bool>>,
                    Arc<Mutex<RECT>>,
                    Arc<Mutex<i32>>,
                    bool,
                ));
            let mut idx = count.lock().unwrap();
            if *idx == *target {
                let mut mi: MONITORINFO = std::mem::zeroed();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(hmonitor, &mut mi) != 0 {
                    let mut r = rect_arc.lock().unwrap();
                    *r = if *use_full_area {
                        mi.rcMonitor
                    } else {
                        mi.rcWork
                    };
                    let mut f = found.lock().unwrap();
                    *f = true;
                }
                return 0; // stop
            }
            *idx += 1;
            1 // continue
        }
    }
    let mut tuple = (
        monitor_index,
        found.clone(),
        rect_arc.clone(),
        count.clone(),
        use_full_area,
    );
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(enum_monitor_proc),
            &mut tuple as *mut _ as isize,
        );
    }
    if *found.lock().unwrap() {
        *rect_arc.lock().unwrap()
    } else {
        // fallback to primary monitor
        let pt = POINT { x: 0, y: 0 };
        let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY) };
        let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(hmon, &mut mi) } != 0 {
            if use_full_area {
                mi.rcMonitor
            } else {
                mi.rcWork
            }
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        }
    }
}

pub fn get_parent_pid(pid: u32) -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            eprintln!("Failed to create process snapshot");
            return None;
        }

        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        if Process32First(snapshot, &mut entry) != FALSE {
            loop {
                if entry.th32ProcessID == pid {
                    CloseHandle(snapshot);
                    return Some(entry.th32ParentProcessID);
                }

                if Process32Next(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }

    None
}
pub fn find_hwnd_by_pid(pid: u32) -> Option<HWND> {
    struct EnumData {
        target_pid: u32,
        hwnd: HWND,
    }

    extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
        let data = unsafe { &mut *(lparam as *mut EnumData) };
        let mut process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process_id);

            // Check if the process ID matches the target PID or its parent PID
            if process_id == data.target_pid {
                data.hwnd = hwnd;
                return 0; // Stop enumeration
            }

            // Optionally, retrieve the parent process ID and check it
            let parent_pid = get_parent_pid(process_id);
            if let Some(ppid) = parent_pid {
                if ppid == data.target_pid {
                    data.hwnd = hwnd;
                    return 0; // Stop enumeration
                }
            }
        }
        if process_id == data.target_pid {
            data.hwnd = hwnd;
            return 0; // Stop enumeration
        }
        1 // Continue enumeration
    }

    let mut data = EnumData {
        target_pid: pid,
        hwnd: ptr::null_mut(),
    };

    unsafe {
        EnumWindows(Some(enum_windows_proc), &mut data as *mut _ as isize);
    }

    if !data.hwnd.is_null() {
        Some(data.hwnd)
    } else {
        None
    }
}

pub fn shake_window(hwnd: HWND, intensity: i32, duration_ms: u64) {
    unsafe {
        // Bring the window to the front
        winapi::um::winuser::SetForegroundWindow(hwnd);

        // Get the current position of the window
        let mut rect: RECT = std::mem::zeroed();
        if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) == 0 {
            eprintln!("Failed to get window rect");
            return;
        }

        let original_x = rect.left;
        let original_y = rect.top;

        let mut elapsed = 0;
        let step_duration = 50; // Shake step duration in milliseconds

        while elapsed < duration_ms {
            // Move the window left
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                original_x - intensity,
                original_y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            sleep(Duration::from_millis(step_duration));
            elapsed += step_duration;

            // Move the window right
            SetWindowPos(
                hwnd as HWND,
                std::ptr::null_mut(),
                original_x + intensity,
                original_y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            sleep(Duration::from_millis(step_duration));
            elapsed += step_duration;

            // Move the window up
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                original_x,
                original_y - intensity,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            sleep(Duration::from_millis(step_duration));
            elapsed += step_duration;

            // Move the window down
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                original_x,
                original_y + intensity,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            sleep(Duration::from_millis(step_duration));
            elapsed += step_duration;
        }

        // Restore the original position
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            original_x,
            original_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}
