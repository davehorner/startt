use dashmap::DashMap;
// src/main.rs
#[cfg(not(feature = "uses_etw"))]
#[allow(unused_imports)]
#[cfg(feature = "uses_etw")]
use ferrisetw::parser::Parser;
#[cfg(feature = "uses_etw")]
use ferrisetw::trace::UserTrace;
#[cfg(feature = "uses_etw")]
use ferrisetw::{EventRecord, SchemaLocator};
use tts::Tts;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufReader, BufRead};
use std::process::{Command, Stdio};
use std::{env, thread};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use widestring::U16CString;
use winapi::shared::minwindef::FALSE;
use winapi::shared::windef::HWND;
use winapi::shared::windef::{HMONITOR, POINT, RECT};
// Window hook for automatic grid eviction on window destroy
use winapi::um::winuser::{SetWinEventHook, EVENT_OBJECT_DESTROY, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, UnhookWinEvent};
use winapi::shared::minwindef::{DWORD, UINT, WPARAM, LPARAM, LRESULT, BOOL, ULONG};
use std::os::raw::c_long;
use winapi::shared::windef::HWINEVENTHOOK;
use once_cell::sync::OnceCell;

use iceoryx2::prelude::*;

// Make the publisher globally accessible
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::GetProcessId;
use winapi::um::processthreadsapi::OpenProcess;
// use winapi::um::psapi::GetProcessImageFileNameW;
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next, TH32CS_SNAPPROCESS,
};
use winapi::um::winnt::HANDLE;
// use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use winapi::um::winuser::SWP_NOSIZE;
use winapi::um::winuser::SWP_NOZORDER;
use winapi::um::winuser::SetWindowPos;
use winapi::um::winuser::{
    EnumDisplayMonitors, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use winapi::um::winuser::{EnumWindows, GetWindowThreadProcessId};
use winapi::um::winuser::{
    GetWindowPlacement, SW_MINIMIZE, SW_RESTORE, ShowWindow, WINDOWPLACEMENT,
};

unsafe extern "system" {
    fn WaitForInputIdle(hProcess: HANDLE, dwMilliseconds: u32) -> u32;
}
static PROGRAM_START: Lazy<Instant> = Lazy::new(Instant::now);
// Use a thread-safe global OnceCell for grid state
static GRID_STATE_ONCE: OnceCell<Arc<Mutex<Option<GridState>>>> = OnceCell::new();

mod gui;
use gui::StarttApp;

fn get_parent_pid(pid: u32) -> Option<u32> {
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
fn find_hwnd_by_pid(pid: u32) -> Option<HWND> {
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


fn shake_window(hwnd: HWND, intensity: i32, duration_ms: u64) {
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
                hwnd,
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
                hwnd,
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

#[cfg(feature = "uses_etw")]
fn start_etw_process_tracker_with_schema(root_pid: u32, tracked_pids: Arc<Mutex<HashSet<u32>>>) {
    #[cfg(feature = "uses_etw")]
    {
        use ferrisetw::parser::Parser;
        let process_callback =
            move |record: &EventRecord, schema_locator: &SchemaLocator| match schema_locator
                .event_schema(record)
            {
                Ok(schema) => {
                    let event_id = record.event_id();
                    let parser = Parser::create(record, &schema);
                    let process_id: u32 = parser.try_parse("ProcessID").unwrap_or(0);
                    let parent_id: u32 = parser.try_parse("ParentID").unwrap_or(0);
                    let image_name: String = parser
                        .try_parse("ImageName")
                        .unwrap_or_else(|_| "N/A".to_string());

                    // Only print events for the root process or its children
                    if parent_id == root_pid {
                        if event_id == 1 {
                            println!(
                                "Process START: PID={}, PPID={}, ImageName={}",
                                process_id, parent_id, image_name
                            );
                            tracked_pids.lock().unwrap().insert(process_id);
                        } else if event_id == 2 {
                            let exit_code: u32 = parser.try_parse("ExitCode").unwrap_or(0);
                            println!(
                                "Process EXIT: PID={}, ExitCode={}, ImageName={}",
                                process_id, exit_code, image_name
                            );
                        }
                    }
                }
                Err(err) => println!("Error {:?}", err),
            };

        let process_provider =
            ferrisetw::provider::Provider::by_guid("22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716") // Microsoft-Windows-Kernel-Process
                .add_callback(process_callback)
                .build();

        // Generate a random trace name to avoid "AlreadyExist" error
        let random_trace_name = format!("MyTrace_{}", rand::random::<u32>());
        let (_user_trace, handle) = UserTrace::new()
            .named(random_trace_name)
            .enable(process_provider)
            .start()
            .unwrap();

        std::thread::spawn(move || {
            let status = <UserTrace as ferrisetw::trace::TraceTrait>::process_from_handle(handle);
            println!("Trace ended with status {:?}", status);
        });
    }
}

// Add this enum for grid placement mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridPlacementMode {
    FirstFree,
    Sequential,
}

// Update GridState to support both modes
#[derive(Clone)]
struct GridCell {
    hwnd: Option<HWND>,
    filled_at: Option<Instant>,
}

struct GridState {
    rows: u32,
    cols: u32,
    monitor: i32,
    next_cell: usize,
    monitor_rect: RECT,
    cells: Vec<GridCell>,
    reserved_cell: Option<usize>,
    filled_count: usize,
    hwnd_to_cell: DashMap<HWND, usize>,
    parent_cell_idx: Option<usize>,
    parent_hwnd: isize,
    launcher_pid: u32,
    launcher_hwnd: isize,
    desktop_hwnd: isize,
    retain_parent_focus: bool,
    retain_launcher_focus:bool,
    has_been_filled_at_some_point: bool,
    fit_grid: bool,
}

impl GridState {

fn with_grid_state<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut GridState) -> R,
{
    if let Some(grid_state_arc) = GRID_STATE_ONCE.get() {
        if let Ok(mut guard) = grid_state_arc.lock() {
            if let Some(grid_state) = guard.as_mut() {
                return Some(f(grid_state));
            }
        }
    }
    None
}
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut GridState) -> R,
    {
        let grid_state_arc = GRID_STATE_ONCE.get().expect("GRID_STATE_ONCE not set").clone();
        let mut guard = grid_state_arc.lock().unwrap();
        let grid_state = guard.as_mut().expect("GridState not initialized");
        f(grid_state)
    }


    /// Move the given HWND to the specified cell index, resizing if fit_grid is true.
    /// Handles console windows with shrinking logic.
    pub fn move_hwnd_to_cell(&self, hwnd: HWND, cell_idx: usize, fit_grid: bool) -> bool {
        use winapi::um::winuser::{GetWindowRect, SetWindowPos, SWP_NOZORDER, SWP_NOSIZE, SW_RESTORE, ShowWindow};
        use std::thread::sleep;
        use std::time::Duration;

        // Get cell geometry
        let (row, col) = (cell_idx / self.cols as usize, cell_idx % self.cols as usize);
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        let x = self.monitor_rect.left + col as i32 * cell_w;
        let y = self.monitor_rect.top + row as i32 * cell_h;

        // Get class name to check for console
        let mut class_name = [0u16; 256];
        let class_name_len = unsafe {
            winapi::um::winuser::GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32)
        };
        let class_name_str = if class_name_len > 0 {
            std::ffi::OsString::from_wide(&class_name[..class_name_len as usize])
                .to_string_lossy()
                .to_string()
        } else {
            String::from("<unknown>")
        };
        let is_console = class_name_str == "ConsoleWindowClass";

        // If minimized, restore first
        let mut placement: winapi::um::winuser::WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<winapi::um::winuser::WINDOWPLACEMENT>() as u32;
        if unsafe { winapi::um::winuser::GetWindowPlacement(hwnd, &mut placement) } != 0 {
            let was_minimized = placement.showCmd == winapi::um::winuser::SW_SHOWMINIMIZED as u32;
            if was_minimized {
                unsafe { ShowWindow(hwnd, SW_RESTORE); }
                sleep(Duration::from_millis(500));
            }
        }

        let mut success = false;
        if is_console && fit_grid {
            // Try shrinking height if needed
            let mut test_h = cell_h;
            let min_h = 100;
            while test_h >= min_h {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        cell_w,
                        test_h,
                        SWP_NOZORDER,
                    );
                }
                sleep(Duration::from_millis(100));
                let mut rect = unsafe { std::mem::zeroed() };
                if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
                    let actual_x = rect.left;
                    let actual_y = rect.top;
                    let actual_h = rect.bottom - rect.top;
                    if actual_x == x && actual_y == y && (actual_h - test_h).abs() < 8 {
                        success = true;
                        println!("Console window moved and resized to height {}", test_h);
                        break;
                    }
                }
                test_h -= 40;
            }
            if !success {
                println!("Warning: Could not fit console window to grid cell, even after shrinking.");
            }
        } else {
            unsafe {
                if fit_grid {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        cell_w,
                        cell_h,
                        SWP_NOZORDER,
                    );
                } else {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER,
                    );
                }
            }
            // Verify move
            let mut rect = unsafe { std::mem::zeroed() };
            if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
                let actual_x = rect.left;
                let actual_y = rect.top;
                if actual_x == x && actual_y == y {
                    success = true;
                } else {
                    println!(
                        "Warning: HWND {:?} did not move to expected position (wanted: {},{} got: {},{})",
                        hwnd, x, y, actual_x, actual_y
                    );
                }
            }
        }
        success
    }

    /// Closes any visible, top-level, non-desktop windows at the center of each grid cell.
    pub fn ensure_clean_desktop(&self) {
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        for (idx, _cell) in self.cells.iter().enumerate() {
            let row = idx / self.cols as usize;
            let col = idx % self.cols as usize;
            let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
            let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
            let pt = winapi::shared::windef::POINT { x, y };
            let hwnd_at_center = unsafe { winapi::um::winuser::WindowFromPoint(pt) };

            // Only close if it's a real window and NOT the desktop
            if self.is_real_window(hwnd_at_center, false) {
                             println!(
                    "Hiding window at cell {} center ({}, {}): HWND = {:?}",
                    idx, x, y, hwnd_at_center
                );
                                unsafe {
                    winapi::um::winuser::ShowWindow(hwnd_at_center, winapi::um::winuser::SW_HIDE);
                }
                // println!(
                //     "Closing window at cell {} center ({}, {}): HWND = {:?}",
                //     idx, x, y, hwnd_at_center
                // );
                // unsafe {
                //     winapi::um::winuser::PostMessageW(
                //         hwnd_at_center,
                //         winapi::um::winuser::WM_CLOSE,
                //         0,
                //         0,
                //     );
                // }
            }
            let corners = [
                (0, 0), // top-left
                (0, self.cols as usize - 1), // top-right
                (self.rows as usize - 1, 0), // bottom-left
                (self.rows as usize - 1, self.cols as usize - 1), // bottom-right
            ];
            for (row, col) in corners {
                let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
                let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
                let pt = winapi::shared::windef::POINT { x, y };
                let hwnd_at_corner = unsafe { winapi::um::winuser::WindowFromPoint(pt) };
                if self.is_real_window(hwnd_at_corner, false) {
                    println!(
                        "Hiding window at corner ({}, {}) HWND = {:?}",
                        x, y, hwnd_at_corner
                    );
                    unsafe {
                        winapi::um::winuser::ShowWindow(hwnd_at_corner, winapi::um::winuser::SW_HIDE);
                    }
                }
            }
        }
    }
        /// Prints which grid cells have the desktop window at their center.
    pub fn print_desktop_cells(&self) {
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        for (idx, cell) in self.cells.iter().enumerate() {
            let row = idx / self.cols as usize;
            let col = idx % self.cols as usize;
            let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
            let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
            let pt = winapi::shared::windef::POINT { x, y };
            let hwnd_at_center = unsafe { winapi::um::winuser::WindowFromPoint(pt) };
            let is_desktop = hwnd_at_center as isize == self.desktop_hwnd;

            // Only print if it's a real window (visible, top-level, not desktop unless allowed)
            if !self.is_real_window(hwnd_at_center, true) {
                continue;
            }

            // Get window title
            let mut title = [0u16; 256];
            let title_len = unsafe {
                winapi::um::winuser::GetWindowTextW(hwnd_at_center, title.as_mut_ptr(), title.len() as i32)
            };
            let title_str = if title_len > 0 {
                std::ffi::OsString::from_wide(&title[..title_len as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                String::from("<no title>")
            };

            // Get class name
            let mut class_name = [0u16; 256];
            let class_name_len = unsafe {
                winapi::um::winuser::GetClassNameW(hwnd_at_center, class_name.as_mut_ptr(), class_name.len() as i32)
            };
            let class_name_str = if class_name_len > 0 {
                std::ffi::OsString::from_wide(&class_name[..class_name_len as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                String::from("<unknown>")
            };

            // Get PID
            let mut pid: u32 = 0;
            unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd_at_center, &mut pid) };

            // Get running time if this cell is occupied
            let running_secs = cell.filled_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);

            println!(
                "Cell {} center ({}, {}): HWND = {:?}{} | Title: '{}' | Class: '{}' | PID: {} | Running: {}s",
                idx,
                x,
                y,
                hwnd_at_center,
                if is_desktop { " [DESKTOP]" } else { "" },
                title_str,
                class_name_str,
                pid,
                running_secs
            );
        }
    }

    /// Assigns a window to a grid cell, moves/resizes it, and updates the grid.
    /// Returns Some(cell_idx) if successful, None otherwise.
    pub fn assign_window_to_grid_cell(
        &mut self,
        hwnd: HWND,
        fit_grid: bool,
        placement_mode: GridPlacementMode,
        retain_parent_focus: bool,
        retain_launcher_focus:bool,
        timeout_secs: Option<u64>,
    ) -> Option<usize> {
        if let Some(&existing_idx) = self.hwnd_to_cell.get(&hwnd).as_deref() {
            // Already assigned, don't reassign or move
            println!("HWND {:?} is already assigned to cell {}, skipping assignment.", hwnd, existing_idx);
            return Some(existing_idx);
        }
        // Find the first available cell (empty or timed out), and check time/pixel constraints before moving
        let total_cells = self.cells.len();
        let mut selected_cell = None;
        let mut selected_coords = (0, 0);
        let mut selected_idx = None;

        // Get the window rect to calculate its width and height
        let mut rect = unsafe { std::mem::zeroed() };
        let (win_width, win_height) = if unsafe { winapi::um::winuser::GetWindowRect(hwnd, &mut rect) } != 0 {
            (rect.right - rect.left, rect.bottom - rect.top)
        } else {
            // Fallback to cell size if GetWindowRect fails
            let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
            let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
            (cell_w, cell_h)
        };

        // Try all cells (parent cell first if needed)
        let mut try_indices = Vec::new();
        if hwnd == self.parent_hwnd as HWND {
            if let Some(parent_cell_idx) = self.reserved_cell {
                try_indices.push(parent_cell_idx);
            }
        } else {
            for idx in 0..total_cells {
                if Some(idx) != self.reserved_cell {
                    try_indices.push(idx);
                }
            }
        }

        for idx in try_indices {
            if !self.is_cell_available(idx, timeout_secs) {
            continue;
            }
            // Compute cell coordinates
            let row = idx / self.cols as usize;
            let col = idx % self.cols as usize;
            let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
            let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
            let x = self.monitor_rect.left + col as i32 * cell_w;
            let y = self.monitor_rect.top + row as i32 * cell_h;
            let (cx, cy) = if fit_grid {
            (x, y)
            } else {
            let mut cx = x + (cell_w - win_width) / 2;
            let mut cy = y + (cell_h - win_height) / 2;
            let min_x = self.monitor_rect.left;
            let min_y = self.monitor_rect.top;
            let max_x = self.monitor_rect.right - win_width;
            let max_y = self.monitor_rect.bottom - win_height;
            cx = cx.clamp(min_x, max_x);
            cy = cy.clamp(min_y, max_y);
            (cx, cy)
            };

            // Check filled_at timeout if set
            let mut filled_time_ok = true;
            if let Some(timeout) = timeout_secs {
            if let Some(filled_at) = self.cells[idx].filled_at {
                let elapsed = Instant::now().duration_since(filled_at).as_secs();
                if elapsed < timeout {
                filled_time_ok = false;
                }
            }
            }
            // Check that cell_pixel_owner does not return a window in hwnd_to_cell
            let (_, _, pixel_owner_hwnd) = self.cell_pixel_owner(row as u32, col as u32).unwrap_or((idx, None, None));
            let mut pixel_owner_ok = true;
            if let Some(owner_hwnd) = pixel_owner_hwnd {
            if self.hwnd_to_cell.contains_key(&owner_hwnd) {
                pixel_owner_ok = false;
            }
            }
            if filled_time_ok && pixel_owner_ok {
            selected_cell = Some((cx, cy));
            selected_idx = Some(idx);
            break;
            }
        }

        let (cell_idx, new_x, new_y) = if let (Some(idx), Some((x, y))) = (selected_idx, selected_cell) {
            (idx, x, y)
        } else {
            // Fallback: use next_position as before (may evict/overlay if all cells are busy)
            let (fallback_idx, fallback_x, fallback_y) = self.next_position(
            win_width,
            win_height,
            fit_grid,
            placement_mode,
            );
            // eprintln!("Warning: All grid cells are busy or failed checks, using fallback cell {}", fallback_idx);
            // self.check_and_fix_grid_sync();
               // Only assign if the fallback cell is actually available and not reserved
        if self.cells.get(fallback_idx).map_or(false, |c| c.hwnd.is_none()) && Some(fallback_idx) != self.reserved_cell {
            (fallback_idx, fallback_x, fallback_y)
        } else {
            eprintln!("No available grid cell found for HWND {:?}, assignment failed.", hwnd);
            return None;
        }
        };

        // Move/resize as before...
        let min_x = self.monitor_rect.left;
        let min_y = self.monitor_rect.top;
        let max_x = self.monitor_rect.right
            - if fit_grid {
            (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32
            } else {
            win_width
            };
        let max_y = self.monitor_rect.bottom
            - if fit_grid {
            (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32
            } else {
            win_height
            };
        let new_x = new_x.clamp(min_x, max_x);
        let new_y = new_y.clamp(min_y, max_y);

        unsafe {
            if fit_grid {
                let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
                let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    new_x,
                    new_y,
                    cell_w,
                    cell_h,
                    SWP_NOZORDER,
                );
            } else {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    new_x,
                    new_y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
            }
        }
        // After moving/resizing a window (child or parent), add this:
        if retain_parent_focus {
            println!("DEBUG: Retaining parent focus after window move (parent_hwnd={:?})", self.parent_hwnd);
            unsafe { winapi::um::winuser::SetForegroundWindow(self.parent_hwnd as HWND); }
        }
        if retain_launcher_focus {
            // Set focus back to the launcher window (parent's parent)
            // Use the stored launcher_hwnd if available, otherwise fallback to GetConsoleWindow
            if self.launcher_hwnd != 0 {
            let launcher_hwnd = self.launcher_hwnd as HWND;
            if !launcher_hwnd.is_null() {
                println!(
                "DEBUG: Retaining launcher focus after window move (launcher_hwnd={:?})",
                launcher_hwnd
                );
                unsafe { winapi::um::winuser::SetForegroundWindow(launcher_hwnd); }
            } else {
                println!(
                "DEBUG: launcher_hwnd is set but null, skipping SetForegroundWindow"
                );
            }
            } else {
            let launcher_hwnd = unsafe { winapi::um::wincon::GetConsoleWindow() };
            if !launcher_hwnd.is_null() {
                println!(
                "DEBUG: Retaining launcher focus using console window (launcher_hwnd={:?})",
                launcher_hwnd
                );
                unsafe { winapi::um::winuser::SetForegroundWindow(launcher_hwnd); }
            } else {
                println!(
                "DEBUG: launcher_hwnd is zero and console window is null, cannot set focus"
                );
            }
            }
        }
        // After moving, verify the window is at the expected position
        let mut rect = unsafe { std::mem::zeroed() };
        let mut success = false;
        if unsafe { winapi::um::winuser::GetWindowRect(hwnd, &mut rect) } != 0 {
            let actual_x = rect.left;
            let actual_y = rect.top;
            if actual_x == new_x && actual_y == new_y {
            success = true;
            }
        }

        if success {
            // Only set the cell if it's not already mapped to another cell
            if !self.hwnd_to_cell.contains_key(&hwnd) {
                self.cells[cell_idx] = GridCell {
                    hwnd: Some(hwnd),
                    filled_at: Some(Instant::now()),
                };
                self.hwnd_to_cell.insert(hwnd, cell_idx);
            }
            if self.has_been_filled_at_some_point() {
                if let Some(timeout) = timeout_secs {
                    self.cells[cell_idx].start_eviction_timer(cell_idx, timeout);
                }
            }
            Some(cell_idx)
        } else {
            eprintln!(
            "Warning: HWND {:?} did not move to expected position (wanted: {},{}).",
            hwnd, new_x, new_y
            );
            None
        }
    }
    
    pub fn set_parent_cell_locked(self, parent_cell_idx: Option<usize>, parent_hwnd: HWND) {
        Self::with(|grid| {
            if let Some(idx) = parent_cell_idx {
                grid.cells[idx] = GridCell {
                    hwnd: Some(parent_hwnd),
                    filled_at: Some(Instant::now()),
                };
                grid.reserved_cell = Some(idx);
                self.move_hwnd_to_cell(parent_hwnd, idx, self.fit_grid);
                println!("Reserved parent cell {} for HWND {:?}", idx, parent_hwnd);
            }
        });
    }

    pub fn check_and_fix_grid_sync_locked() {
        Self::with(|grid| {
            grid.check_and_fix_grid_sync();
        });
    }
}

impl GridState {
    /// Returns true if the hwnd is visible, top-level, and (optionally) not the desktop.
    fn is_real_window(&self, hwnd: HWND, allow_desktop: bool) -> bool {
        if hwnd.is_null() {
            return false;
        }
        let is_visible = unsafe { winapi::um::winuser::IsWindowVisible(hwnd) != 0 };
        let is_top_level = unsafe { winapi::um::winuser::GetParent(hwnd) }.is_null();
        let is_desktop = hwnd as isize == self.desktop_hwnd;
        is_visible && is_top_level && (allow_desktop || !is_desktop)
    }

         /// Checks if the grid's cells and hwnd_to_cell map are in sync.
        /// Prints a warning and optionally corrects the map if out of sync.
        fn check_and_fix_grid_sync(self: &mut GridState) {
            use winapi::um::winuser::{WindowFromPoint};
            let mut map_errors = 0;
            let mut cell_errors = 0;

            println!("Grid cell occupancy BEFORE: {:?}", self.cells.iter().map(|c| c.hwnd).collect::<Vec<_>>());
            // 1. Check that every cell's hwnd is correctly mapped in hwnd_to_cell
            let mut to_insert = Vec::new();
            for (idx, cell) in self.cells.iter().enumerate() {
                if let Some(hwnd) = cell.hwnd {
                    match self.hwnd_to_cell.get(&hwnd) {
                        Some(mapped_idx) if *mapped_idx == idx => { /* OK */ }
                        Some(mapped_idx) => {
                            println!("Grid sync WARNING: HWND {:?} in cell {} but mapped to cell {:?} in map", hwnd, idx, mapped_idx);
                            map_errors += 1;
                        }
                        None => {
                            println!("Grid sync WARNING: HWND {:?} in cell {} but missing from map", hwnd, idx);
                            // Optionally fix:
                            to_insert.push((hwnd, idx));
                            cell_errors += 1;
                        }
                    }
                }
            }
            for (hwnd, idx) in to_insert {
                self.hwnd_to_cell.insert(hwnd, idx);
            }

            // 2. Check that every hwnd in the map is actually present in a cell
            let mut to_remove = Vec::new();
            for entry in self.hwnd_to_cell.iter() {
                let hwnd = *entry.key();
                let idx = *entry.value();
                if self.cells.get(idx).and_then(|c| c.hwnd).unwrap_or(ptr::null_mut()) != hwnd {
                    println!("Grid sync WARNING: HWND {:?} mapped to cell {} but not present in that cell", hwnd, idx);
                    // Optionally fix:
                    to_remove.push(hwnd);
                    map_errors += 1;
                }
            }
            for hwnd in to_remove {
                self.hwnd_to_cell.remove(&hwnd);
            }

            // 3. (Optional) Pixel check: does the center pixel of each cell belong to the mapped HWND?
            for (idx, cell) in self.cells.iter().enumerate() {
                if let Some(hwnd) = cell.hwnd {
                    let row = idx / self.cols as usize;
                    let col = idx % self.cols as usize;
                    let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
                    let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
                    let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
                    let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
                    let pt = POINT { x, y };
                    let pixel_owner = unsafe { WindowFromPoint(pt) };
                    // Only warn if the pixel is owned by a non-desktop, non-this window
                    if pixel_owner != hwnd && pixel_owner as isize != self.desktop_hwnd {
                        println!(
                            "Grid sync WARNING: Cell {} HWND {:?} does not own pixel ({}, {}) (owned by {:?})",
                            idx, hwnd, x, y, pixel_owner
                        );
                    }
                }
            }

            if map_errors == 0 && cell_errors == 0 {
                println!("Grid sync: OK");
            } else {
                println!("Grid sync: {} map errors, {} cell errors", map_errors, cell_errors);
                println!("Grid cell occupancy AFTER: {:?}", self.cells.iter().map(|c| c.hwnd).collect::<Vec<_>>());
            }
        }

    fn has_been_filled_at_some_point(&self) -> bool {
        return self.has_been_filled_at_some_point;
    }
    /// Set a cell for the parent window and mark it as reserved.
    fn set_parent_cell(&mut self, parent_cell_idx: Option<usize>, parent_hwnd: HWND) {
        if let Some(idx) = parent_cell_idx {
            self.cells[idx] = GridCell {
                hwnd: Some(parent_hwnd),
                filled_at: Some(Instant::now()),
            };
            self.reserved_cell = Some(idx);
            self.move_hwnd_to_cell(parent_hwnd, idx, self.fit_grid);
            println!("Reserved parent cell {} for HWND {:?}", idx, parent_hwnd);
        }
    }
}

// SAFETY: HWND is safe to send and share between threads on Windows.
unsafe impl Send for GridCell {}
unsafe impl Sync for GridCell {}
unsafe impl Send for GridState {}
unsafe impl Sync for GridState {}

impl GridState {

    pub fn set_has_been_filled_at_some_point(&mut self) {
        self.has_been_filled_at_some_point = true;
    }
        /// Returns a Vec<(cell_idx, HWND, Option<HWND>)> for each cell:
    /// - cell_idx: the cell index
    /// - cell_hwnd: the HWND assigned to the cell (may be None)
    /// - pixel_owner_hwnd: the HWND that actually owns the center pixel of the cell (may be None)
    pub fn cell_pixel_owners(&self) -> Vec<(usize, Option<HWND>, Option<HWND>)> {
        use winapi::um::winuser::WindowFromPoint;
        let mut result = Vec::with_capacity(self.cells.len());
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        for (idx, cell) in self.cells.iter().enumerate() {
            let row = idx / self.cols as usize;
            let col = idx % self.cols as usize;
            let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
            let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
            let pt = winapi::shared::windef::POINT { x, y };
            let hwnd = unsafe { WindowFromPoint(pt) };
            let pixel_owner_hwnd = if self.is_real_window(hwnd, true) { Some(hwnd) } else { None };
            result.push((idx, cell.hwnd, pixel_owner_hwnd));
        }
        result
    }
    /// Returns (cell_idx, cell_hwnd, pixel_owner_hwnd) for a specific cell position (row, col).
    pub fn cell_pixel_owner(&self, row: u32, col: u32) -> Option<(usize, Option<HWND>, Option<HWND>)> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        let idx = (row * self.cols + col) as usize;
        let cell = self.cells.get(idx)?;
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        let x = self.monitor_rect.left + col as i32 * cell_w + cell_w / 2;
        let y = self.monitor_rect.top + row as i32 * cell_h + cell_h / 2;
        let pt = winapi::shared::windef::POINT { x, y };
        let hwnd = unsafe { winapi::um::winuser::WindowFromPoint(pt) };
        let pixel_owner_hwnd = if self.is_real_window(hwnd, true) { Some(hwnd) } else { None };
        Some((idx, cell.hwnd, pixel_owner_hwnd))
    }

    fn is_cell_available(&mut self, idx: usize, timeout_secs: Option<u64>) -> bool {
        let cell = &mut self.cells[idx];
        
        // Use the DashMap (hwnd_to_cell) to check if this cell's hwnd is still mapped correctly
        if let Some(hwnd) = cell.hwnd {
            if let Some(mapped_idx) = self.hwnd_to_cell.get(&hwnd) {
                if *mapped_idx != idx {
                    // The hwnd is mapped to a different cell, so free this cell
                    *cell = GridCell { hwnd: None, filled_at: None };
                    return true;
                } else {
                    return false;
                }
            } else {
                // The hwnd is not in the map, so free this cell
                *cell = GridCell { hwnd: None, filled_at: None };
                return true;
            }
        }
        if cell.hwnd.is_none() {
            *cell = GridCell { hwnd: None, filled_at: None };
            return true;
        }
        // Check if the window is still valid
        if let Some(hwnd) = cell.hwnd {
            unsafe {
                if winapi::um::winuser::IsWindow(hwnd) == 0 || winapi::um::winuser::IsWindowVisible(hwnd) == 0 {
                    // Window is gone or not visible, cell is available
                    *cell = GridCell { hwnd: None, filled_at: None };
                    return true;
                }
            }
        }
        // Check timeout
        if let (Some(timeout), Some(filled_at)) = (timeout_secs, cell.filled_at) {
            let elapsed = Instant::now().duration_since(filled_at).as_secs();
            if elapsed >= timeout {
                *cell = GridCell { hwnd: None, filled_at: None };
                return true;
            }
        }
        false
    }
    // Returns (cell_idx, x, y)
    fn next_position(
        &mut self,
        win_width: i32,
        win_height: i32,
        fit_grid: bool,
        placement_mode: GridPlacementMode,
    ) -> (usize, i32, i32) {
        let total_cells = (self.rows * self.cols) as usize;
        let cell = match placement_mode {
            GridPlacementMode::Sequential => {
                let mut c = self.next_cell % total_cells;
                // Skip reserved cell if needed
                if let Some(reserved) = self.reserved_cell {
                    if c == reserved {
                        self.next_cell += 1;
                        c = self.next_cell % total_cells;
                    }
                }
                // Use c before incrementing next_cell!
                self.next_cell += 1;
                c
            }
            GridPlacementMode::FirstFree => {
                let non_reserved = |idx: usize| Some(idx) != self.reserved_cell;
                // Always pick the lowest-index empty, non-reserved cell
                if let Some(idx) = self.cells.iter().enumerate().find(|(idx, c)| c.hwnd.is_none() && non_reserved(*idx)).map(|(idx, _)| idx) {
                    idx
                } else {
                    // Fallback: all non-reserved cells are filled, pick any non-reserved cell
                    let fallback = (0..total_cells).find(|idx| non_reserved(*idx)).unwrap_or(0);
                    eprintln!("Warning: All grid cells are full, using fallback cell {}", fallback);
                    self.set_has_been_filled_at_some_point();
                    fallback
                }
            }
        };
        let row = cell / self.cols as usize;
        let col = cell % self.cols as usize;
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        let x = self.monitor_rect.left + col as i32 * cell_w;
        let y = self.monitor_rect.top + row as i32 * cell_h;

        if fit_grid {
            (cell, x, y)
        } else {
            let mut cx = x + (cell_w - win_width) / 2;
            let mut cy = y + (cell_h - win_height) / 2;
            let min_x = self.monitor_rect.left;
            let min_y = self.monitor_rect.top;
            let max_x = self.monitor_rect.right - win_width;
            let max_y = self.monitor_rect.bottom - win_height;
            cx = cx.clamp(min_x, max_x);
            cy = cy.clamp(min_y, max_y);
            (cell, cx, cy)
        }
    }
}

// Helper to get monitor RECT by index (0 = primary)
fn get_monitor_rect(monitor_index: i32, use_full_area: bool) -> RECT {
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
// Helper function for parsing grid argument
fn parse_grid_arg(grid_str: &str) -> (u32, u32, i32) {
    let (rc, m) = if let Some(idx) = grid_str.find('m') {
        (&grid_str[..idx], Some(&grid_str[idx + 1..]))
    } else {
        (grid_str, None)
    };
    let parts: Vec<&str> = rc.split('x').collect();
    if parts.len() != 2 {
        panic!(
            "Grid argument must be in the form ROWSxCOLS or ROWSxCOLSmDISPLAY, got '{}'",
            grid_str
        );
    }
    let rows = parts[0]
        .parse::<u32>()
        .expect("Invalid ROWS in grid argument");
    let cols = parts[1]
        .parse::<u32>()
        .expect("Invalid COLS in grid argument");
    let monitor = m.and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    (rows, cols, monitor)
}

            use uiautomation::UIElement;
            use windows::core::*;
            use windows::Win32::UI::Accessibility::*;

pub struct MyEventHandler {}

impl MyEventHandler {
    pub fn new() -> Self {
        Self {}
    }
}

use std::sync::mpsc::Sender;
static mut HOOK_SENDER: Option<Sender<usize>> = None;
fn main() -> windows::core::Result<()> {

    // Launch egui window on the main thread
    // Only launch egui window if --gui is present in the command line arguments
    if env::args().any(|arg| arg == "--gui") {

std::thread::spawn(move || {
        let window = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let automation = uiautomation::UIAutomation::new().unwrap();
        let element = automation.element_from_handle(window.into()).unwrap();
        let rect = element.get_bounding_rectangle().unwrap();
        println!("Window Rect = {}", rect);
});




        // 1. Create the channel and publisher thread
// Move publisher creation and usage into the spawned thread to avoid sharing across threads
let (tx, rx) = mpsc::channel::<usize>();

unsafe { HOOK_SENDER = Some(tx.clone()); }
    // --- UIAutomation integration ---
 

std::thread::spawn(move || {
    let node = NodeBuilder::new().create::<ipc::Service>().expect("Failed to create node");
    let service = node
        .service_builder(&"My/Funk/ServiceName".try_into().expect("Failed to parse service name"))
        .publish_subscribe::<usize>()
        .open_or_create()
        .expect("Failed to open or create service");
    let publisher = service.publisher_builder().create().expect("Failed to create publisher");

    while let Ok(val) = rx.recv() {
        if let Ok(sample) = publisher.loan_uninit() {
            println!("Sending value: {}", val);
            let sample = sample.write_payload(val);
            let _ = sample.send();
        }
    }
});

        if let Some(value) = gui::fun_name() {
            value?;
            return Ok(());
        }
    }

    {
        // Start a thread to listen for messages from iceoryx2 and print them
        std::thread::spawn(|| {
            use iceoryx2::prelude::*;

            const CYCLE_TIME: Duration = Duration::from_secs(1);

            let node = match NodeBuilder::new().create::<ipc::Service>() {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Failed to create node: {:?}", e);
                    return;
                }
            };

            let service = match node.service_builder(&"My/Funk/ServiceName".try_into().unwrap())
                .publish_subscribe::<usize>()
                .open_or_create()
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to open or create service: {:?}", e);
                    return;
                }
            };

            let subscriber = match service.subscriber_builder().create() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to create subscriber: {:?}", e);
                    return;
                }
            };
            let mut tts = Tts::default().unwrap();
            while node.wait(CYCLE_TIME).is_ok() {
                while let Ok(Some(sample)) = subscriber.receive() {
                    println!("received: {:?}", *sample);
                    // For usize, just print the value directly
                    // Only speak if the sample (HWND as usize) is present in the grid
                    GridState::with_grid_state(|grid| {
                        println!("Checking if sample {:?} is in grid {}", *sample, grid.hwnd_to_cell.len());
                        let hwnd = *sample as winapi::shared::windef::HWND;
                        if grid.hwnd_to_cell.contains_key(&hwnd) {
                            println!("TTS: {}", *sample);
                            let _ = tts.speak(&sample.to_string(), false);
                        }
                    });
                    // If you want to use TTS for numbers, convert to string
                    // let _ = tts.speak(&sample.to_string(), false);
                }
            }
            std::process::exit(0);
        });
    }

    // Shared set for all tracked PIDs (parent + children)
    let running = Arc::new(AtomicBool::new(true));
    let grid_state_arc = Arc::new(Mutex::new(None::<GridState>));
    let _ = GRID_STATE_ONCE.set(grid_state_arc.clone());
    #[derive(Debug, Clone)]
    struct GridConfig {
        rows: u32,
        cols: u32,
        monitor: i32,
    }
    let mut grid: Option<GridConfig> = None;
    let mut follow_children = false;
    let mut follow_forver = false;
    let mut positional_args = Vec::new();
    let mut timeout_secs: Option<u64> = None;
    let mut hwnd_start_times: HashMap<HWND, Instant> = HashMap::new();
    let mut flash_topmost_ms: u64 = 10; // default to 10ms
    let mut should_hide_title_bar = false;
    let mut should_hide_border = false;
    let mut args = env::args_os().skip(1).peekable();
    let mut shake_duration: u64 = 500; // default 2000ms
    let mut fit_grid = false;
    let mut reserve_parent_cell = false;
    let mut assign_parent_cell: Option<(u32, u32, Option<i32>)> = None;
    let mut hide_taskbar = false;
    let mut show_taskbar = false;
    let mut debug_chrome = false;
    let mut grid_placement_mode = GridPlacementMode::FirstFree; // default is FirstFree (use --grid-placement=sequential for sequential)
    let mut retain_parent_focus = false;
    let mut retain_launcher_focus = false;
    let mut keep_open = false; // <-- Add this near your other flags
    while let Some(arg) = args.next() {
        let arg_str = arg.to_string_lossy();
        if arg_str == "-f" || arg_str == "--follow" {
            follow_children = true;
        } else if arg_str == "-F" || arg_str == "--follow-forver" {
            follow_children = true;
            follow_forver = true;
        } else if arg_str == "-ko" || arg_str == "--keep-open" {   // <-- Add this
        keep_open = true;
    } else if arg_str == "-hT" || arg_str == "--hide-title-bar" {
            should_hide_title_bar = true;
        } else if arg_str == "--show-taskbar" || arg_str == "-stb" {
            show_taskbar = true;
        } else if arg_str == "--debug-chrome" || arg_str == "-dbg" {
            debug_chrome = true;
        } else if arg_str == "-g" || arg_str == "--grid" {
            let grid_arg = args
                .next()
                .expect("Expected ROWSxCOLS or ROWSxCOLSmDISPLAY# after -g/--grid");
            let grid_str = grid_arg.to_string_lossy();
            let (rows, cols, monitor) = parse_grid_arg(&grid_str);
            grid = Some(GridConfig { rows, cols, monitor });
            println!("Grid set to {}x{} on monitor {}", rows, cols, monitor);
        } else if arg_str == "--hide-taskbar" || arg_str == "-htb" {
            hide_taskbar = true;
        } else if arg_str.starts_with("-g") && arg_str.len() > 2 {
            // Support -g2x2 or -g2x2m1
            let grid_str = &arg_str[2..];
            let (rows, cols, monitor) = parse_grid_arg(grid_str);
            grid = Some(GridConfig { rows, cols, monitor });
            println!("Grid set to {}x{} on monitor {}", rows, cols, monitor);
        } else if arg_str == "-t" || arg_str == "--timeout" {
            let t_arg = args
                .next()
                .expect("Expected number of seconds after -t/--timeout");
            timeout_secs = Some(
                t_arg
                    .to_string_lossy()
                    .parse()
                    .expect("Invalid timeout value"),
            );
        } else if arg_str == "-T" || arg_str == "--flash-topmost" {
            // Accept an optional value (milliseconds)
            if let Some(val) = args.peek() {
                if let Ok(ms) = val.to_string_lossy().parse::<u64>() {
                    args.next(); // consume
                    flash_topmost_ms = ms;
                } else {
                    flash_topmost_ms = 10; // default
                }
            } else {
                flash_topmost_ms = 10; // default
            }
        } else if arg_str == "-hB" || arg_str == "--hide-border" {
            should_hide_border = true;
        } else if arg_str == "--shake-duration" || arg_str == "-sd" {
            let dur_arg = args
                .next()
                .expect("Expected milliseconds after --shake-duration/-sd");
            shake_duration = dur_arg
                .to_string_lossy()
                .parse()
                .expect("Invalid shake duration value");
        } else if arg_str == "--fit-grid" || arg_str == "-fg" {
            fit_grid = true;
        } else if arg_str == "--reserve-parent-cell" || arg_str == "-rpc" {
            reserve_parent_cell = true;
        } else if arg_str == "--assign-parent-cell" || arg_str == "-apc" {
            if let Some(cell_arg) = args.peek() {
                let cell_str = cell_arg.to_string_lossy().to_string();
                if cell_str.contains('x') {
                    args.next(); // consume
                    let (rc, m) = if let Some(idx) = cell_str.find('m') {
                        (&cell_str[..idx], Some(&cell_str[idx + 1..]))
                    } else {
                        (cell_str.as_str(), None)
                    };
                    let parts: Vec<&str> = rc.split('x').collect();
                    if parts.len() == 2 {
                        if let (Ok(row), Ok(col)) = (parts[0].parse(), parts[1].parse()) {
                            let monitor = m.and_then(|s| s.parse::<i32>().ok());
                            assign_parent_cell = Some((row, col, monitor));
                        }
                    }
                } else {
                    assign_parent_cell = Some((0, 0, None));
                }
            } else {
                assign_parent_cell = Some((0, 0, None));
            }
        } else if arg_str.starts_with("--grid-placement=") {
            let mode = arg_str.split('=').nth(1).unwrap_or("firstfree");
            grid_placement_mode = match mode.to_ascii_lowercase().as_str() {
                "sequential" => GridPlacementMode::Sequential,
                _ => GridPlacementMode::FirstFree,
            };
        } else if arg_str == "--retain-parent-focus" || arg_str == "-rpf" {
            retain_parent_focus = true;
        } else if arg_str == "--retain-launcher-focus" || arg_str == "-rlf" {
            retain_launcher_focus = true;
        } else {
            positional_args.push(arg);
            // Push the rest as positional args
            positional_args.extend(args);
            break;
        }
    }
    println!("Arguments: {:?}", positional_args);
    if debug_chrome {
        let mut did_mutate = false;
        for arg in positional_args.iter_mut() {
            let s = arg.to_string_lossy();
            if s.starts_with("http://") || s.starts_with("https://") {
                let new_arg = format!("debugchrome://{}", &s);
                *arg = OsString::from(new_arg);
                did_mutate = true;
            }
        }
        if did_mutate {
            println!("Debug Chrome rewrite: {:?}", positional_args);
        }
    }
    let mut args = positional_args.into_iter();
    let mut file = args
        .next()
        .expect("Usage: startt [-f] [-g ROWSxCOLS or ROWSxCOLSmDISPLAY#] <executable|document|URL> [args...]");
    if let Some(GridConfig { monitor, .. }) = grid {
        if hide_taskbar {
            println!("Hiding taskbar on monitor {}", monitor);
            hide_taskbar_on_monitor(monitor);
        }
        if show_taskbar {
            println!("Showing taskbar on monitor {}", monitor);
            show_taskbar_on_monitor(monitor);
        }
    }
    // Reconstruct the parameter string (everything after the first token)
    let mut params = args
        .map(|a| {
            let s = a.to_string_lossy();
            if s.contains(' ') {
                format!("\"{}\"", s)
            } else {
                s.into()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Check if the first argument is a URL
    let binding = file.clone();
    let file_str = binding.to_string_lossy();
    if file_str.starts_with("http://") || file_str.starts_with("https://") {
        // Query the Windows registry for the protocol handler
        use winreg::RegKey;
        use winreg::enums::*;

        let protocol = if file_str.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
        let protocol_key = hkcr.open_subkey(format!(r"{}\shell\open\command", protocol))?;
        let handler: String = protocol_key.get_value("")?;

        // Extract the executable path from the registry value
        let handler_path = if handler.starts_with('"') {
            // If the path is quoted, extract the part within quotes
            handler.split('"').nth(1).unwrap_or_default()
        } else {
            // Otherwise, take the first whitespace-separated token
            handler.split_whitespace().next().unwrap_or_default()
        };
        println!("Protocol handler for {}: {:?}", protocol, handler_path);
        println!("url {:?}", file_str);

        file = handler_path.into();
        params = file_str.to_string();
    }
    // Convert both strings to wide (UTF-16) null-terminated
    let file_w = U16CString::from_os_str(file.clone()).map_err(|e| windows::core::Error::new(windows::core::HRESULT(0), format!("{:?}", e)))?;
    let params_w = if params.is_empty() {
            None
        } else {
            Some(U16CString::from_str(&params).map_err(|e| windows::core::Error::new(windows::core::HRESULT(0), format!("{:?}", e)))?)
        };

    // Prepare to collect shake thread handles
    let mut shake_handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    // Launch the process
    let mut sei = winapi::um::shellapi::SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<winapi::um::shellapi::SHELLEXECUTEINFOW>() as u32,
        fMask: winapi::um::shellapi::SEE_MASK_NOCLOSEPROCESS,
        hwnd: ptr::null_mut(),
        lpVerb: ptr::null(),
        lpFile: file_w.as_ptr(),
        lpParameters: params_w.as_ref().map(|s| s.as_ptr()).unwrap_or(ptr::null()),
        lpDirectory: ptr::null(),
        nShow: winapi::um::winuser::SW_SHOWNORMAL,
        hInstApp: ptr::null_mut(),
        lpIDList: ptr::null_mut(),
        lpClass: ptr::null(),
        hkeyClass: ptr::null_mut(),
        dwHotKey: 0,
        hProcess: ptr::null_mut(),
        hMonitor: ptr::null_mut(),
    };
    unsafe {
        if winapi::um::shellapi::ShellExecuteExW(&mut sei) == 0 {
            return Err(windows::core::Error::from(std::io::Error::last_os_error()));
        }

        let mut active_windows: Vec<(HWND, u32, String, (i32, i32, i32, i32))> = Vec::new();
        let mut staged_windows: VecDeque<(HWND, u32, String, (i32, i32, i32, i32))> = VecDeque::new();
        // Get the PID of the process that launched us
        let launching_pid = get_parent_pid(std::process::id()).unwrap_or(0);
        println!("Launching PID (parent of this process): {}", launching_pid);
        let parent_pid = GetProcessId(sei.hProcess);
        let parent_hwnd = Arc::new(Mutex::new(None::<isize>));
        // After launching the process and getting parent_pid:
        let tracked_pids = Arc::new(Mutex::new(HashSet::new()));

        // Check for admin rights before starting ETW
        #[cfg(feature = "uses_etw")]
        if !is_admin::is_admin() {
            println!("Not running as administrator. ETW process tracking will be disabled.");
        } else {
            #[cfg(feature = "uses_etw")]
            start_etw_process_tracker_with_schema(parent_pid, tracked_pids.clone());
        }
        // Ctrl+C handler
        {
            let running = running.clone();
            let tracked_pids_for_ctrlc = tracked_pids.clone();
            let parent_hwnd_for_ctrlc = parent_hwnd.clone();
            ctrlc::set_handler(move || {
               let hwnd_opt = {
                    let guard = parent_hwnd_for_ctrlc.lock().unwrap();
                    *guard
                };
                println!(
                    "Ctrl+C reached for parent HWND {:?}, sending WM_CLOSE",
                    hwnd_opt
                );
                unsafe {
                    if let Some(hwnd_isize) = hwnd_opt {
                        let hwnd = hwnd_isize as HWND;
                        // Send WM_CLOSE to the parent window
                        winapi::um::winuser::SendMessageW(
                            hwnd,
                            winapi::um::winuser::WM_CLOSE,
                            0,
                            0,
                        );
                    }
                }

                println!("\nCtrl+C pressed! Killing all child processes...");
                running.store(false, Ordering::SeqCst);
                let mut child_pids = startt::get_child_pids(parent_pid);
                let etw_pids: Vec<u32> = tracked_pids_for_ctrlc
                    .lock()
                    .unwrap()
                    .iter()
                    .copied()
                    .collect();
                for pid in etw_pids {
                    if !child_pids.contains(&pid) {
                        child_pids.push(pid);
                    }
                }
                println!("Child PIDs (snapshot + ETW): {:?}", child_pids);
                for pid in child_pids {
                    // Try to kill the process
                    let handle = OpenProcess(winapi::um::winnt::PROCESS_TERMINATE, 0, pid);
                    if !handle.is_null() {
                        winapi::um::processthreadsapi::TerminateProcess(handle, 1);
                        CloseHandle(handle);
                        println!("Terminated PID {}", pid);
                    }
                }
                std::process::exit(0);
            }).map_err(|e| windows::core::Error::new(windows::core::HRESULT(0), format!("{:?}", e)))?;
        }

        println!("Launched PID = {}", parent_pid);
        println!("Launched HWND = {:?}", sei.hwnd);
        println!("Launched file = {:?}", file);
        println!("Launching: file={:?} params={:?}", file, params);
        WaitForInputIdle(sei.hProcess, winapi::um::winbase::INFINITE);
        sleep(Duration::from_millis(1000));
        let mut gui = if follow_children {
            startt::find_oldest_recent_apps(
                &file.to_string_lossy(),
                1,
                Some(parent_pid),
                Some(launching_pid),
            )
        } else {
            startt::find_most_recent_gui_apps(
                &file.to_string_lossy(),
                1,
                Some(parent_pid),
                Some(launching_pid),
            )
        };

        if follow_children {
            let handle = OpenProcess(winapi::um::winnt::SYNCHRONIZE, 0, parent_pid);
            if !handle.is_null() {
                let wait_result = winapi::um::synchapi::WaitForSingleObject(handle, 0);
                if wait_result != winapi::um::winbase::WAIT_OBJECT_0 {
                    // Parent is still alive, try to find its HWND
                    println!(
                        "Parent process {} is still alive. Searching for HWND...",
                        parent_pid
                    );
                    if let Some(hwnd) = find_hwnd_by_pid(parent_pid) {
                        {
                            let mut phwnd = parent_hwnd.lock().unwrap();
                            *phwnd = Some(hwnd as isize);
                        }
                        let mut class_name = [0u16; 256];
                        let class_name_len = winapi::um::winuser::GetClassNameW(
                            hwnd,
                            class_name.as_mut_ptr(),
                            class_name.len() as i32,
                        );
                        let class_name_str = if class_name_len > 0 {
                            OsString::from_wide(&class_name[..class_name_len as usize])
                                .to_string_lossy()
                                .to_string()
                        } else {
                            String::from("<unknown parent class>")
                        };
                        println!(
                            "Found parent HWND {:?} for PID {} with class name: {}",
                            hwnd, parent_pid, class_name_str
                        );
                        // Set gui to contain the found parent_hwnd so later logic works as expected
                        // Get the real bounds for the found parent_hwnd
                        let mut rect = std::mem::zeroed();
                        if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                            let bounds = (
                                rect.left,
                                rect.top,
                                rect.right - rect.left,
                                rect.bottom - rect.top,
                            );
                            gui = vec![(hwnd, parent_pid, class_name_str.clone(), bounds)];
                        } else {
                            // Fallback: use zero bounds if GetWindowRect fails
                            gui = vec![(hwnd, parent_pid, class_name_str.clone(), (0, 0, 0, 0))];
                        }
                    }
                } else {
                    println!("Parent process {} has terminated. Exiting.", parent_pid);
                    gui = startt::find_most_recent_gui_apps(
                        &file.to_string_lossy(),
                        1,
                        Some(parent_pid),
                        Some(launching_pid),
                    );
                }
                CloseHandle(handle);
            } else {
                println!("Parent process {} has terminated. Exiting.", parent_pid);
                gui = startt::find_most_recent_gui_apps(
                    &file.to_string_lossy(),
                    1,
                    Some(parent_pid),
                    Some(launching_pid),
                );
            }
        }
        // Create grid state if needed



        
        // let mut grid_state: Option<GridState> = None;
       
        // --- Parent window(s) ---
        // Extract grid config early to avoid moving grid
        let (grid_rows, grid_cols, grid_monitor) = if let Some(ref g) = grid {
            (g.rows, g.cols, g.monitor)
        } else {
            (1, 1, 0)
        };

        for (i, (hwnd, pid, class_name, bounds)) in gui.clone().into_iter().enumerate() {
            // class_name here is a String
            let is_console = class_name == "ConsoleWindowClass";

            println!(
                "{}. HWND = {:?}, PID = {}, Class = {}, Bounds = {:?}",
                i + 1,
                hwnd,
                pid,
                class_name,
                bounds
            );

            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;

            if GetWindowPlacement(hwnd, &mut placement) != 0 {
                let was_minimized =
                    placement.showCmd == winapi::um::winuser::SW_SHOWMINIMIZED.try_into().unwrap();
                if was_minimized {
                    println!("Window {:?} is minimized. Restoring...", hwnd);
                    ShowWindow(hwnd, SW_RESTORE);
                    sleep(Duration::from_millis(500));
                }
                // Remove border/title bar first
                if should_hide_border {
                    println!("Hiding border for HWND {:?}", hwnd);
                    hide_window_border(hwnd);
                }
                if should_hide_title_bar {
                    println!("Hiding title bar for HWND {:?}", hwnd);
                    hide_window_title_bar(hwnd);
                }
        if i == 0 {
            // Properly initialize grid_state if grid is enabled and grid_state is None
            if grid.is_some() && grid_state_arc.lock().unwrap().is_none() {
                let rows = grid_rows;
                let cols = grid_cols;
                let monitor = grid_monitor;
                println!("Creating grid_state with monitor: {}", monitor);
                let monitor_rect = get_monitor_rect(monitor, hide_taskbar);
                let reserved_cell = if reserve_parent_cell {
                    assign_parent_cell.map(|(r, c, _)| (r * cols + c) as usize)
                } else {
                    None
                };
                let mut g = GridState {
                    rows,
                    cols,
                    monitor,
                    next_cell: 0,
                    monitor_rect,
                    cells: vec![GridCell { hwnd: None, filled_at: None }; (rows * cols) as usize],
                    reserved_cell,
                    filled_count: 0,
                    hwnd_to_cell: DashMap::new(),
                    parent_cell_idx: None,
                    parent_hwnd: *parent_hwnd.lock().unwrap().as_ref().unwrap_or(&0),
                    launcher_pid: launching_pid,
                    launcher_hwnd: find_hwnd_by_pid(launching_pid).map_or(0, |hwnd| hwnd as isize),
                    retain_parent_focus,
                    retain_launcher_focus,
                    desktop_hwnd: unsafe { winapi::um::winuser::GetDesktopWindow() as isize },
                    has_been_filled_at_some_point: false,
                    fit_grid: fit_grid,
                };
                // Store the grid state in the global Arc<Mutex<Option<GridState>>>
                *grid_state_arc.lock().unwrap() = Some(g);
            
                // Now you can safely call with/set_parent_cell
                GridState::with(|g| {
                    g.ensure_clean_desktop(); g.print_desktop_cells();
                    g.set_parent_cell(reserved_cell, hwnd);
                    // let result = g.assign_window_to_grid_cell(
                    //     hwnd,
                    //     fit_grid,
                    //     grid_placement_mode,
                    //     retain_parent_focus,
                    //     retain_launcher_focus,
                    //     timeout_secs,
                    // );
                    // match result {
                    //     Some(cell_idx) => {
                    //         println!("Assigned HWND {:?} to grid cell {}", hwnd, cell_idx);
                    //     }
                    //     None => {
                    //         println!("Failed to assign HWND {:?} to grid", hwnd);
                    //     }
                    // }
                });
                install_window_destroy_hook(grid_state_arc.clone());
                continue;

                // grid_state = Some(g);
            }
            let rows = grid_rows;
            let cols = grid_cols;
            let monitor = grid_monitor;

            if let Some(ref grid_state) = *grid_state_arc.lock().unwrap() {
                println!(
                    "Grid enabled: {}x{} on monitor {} (rect: left={}, top={}, right={}, bottom={})",
                    rows,
                    cols,
                    monitor,
                    grid_state.monitor_rect.left,
                    grid_state.monitor_rect.top,
                    grid_state.monitor_rect.right,
                    grid_state.monitor_rect.bottom
                );
            }
            // This is the parent window, assign to the specified cell if requested
            let (parent_row, parent_col, parent_monitor_opt) =
                assign_parent_cell.unwrap_or((0, 0, None));
            let parent_monitor = parent_monitor_opt
                .or_else(|| { Some(grid_monitor)})
                .unwrap_or(0);
            println!(
                "Assigning parent: parent_monitor_opt={:?}, grid_state.monitor={}, using hide_taskbar={}",
                parent_monitor_opt,
                grid_monitor,
                hide_taskbar
            );


            // Only use full area if not a console window; consoles can't do that *shrug*
            let use_full_area = if is_console { false } else { hide_taskbar };
            let monitor_rect = get_monitor_rect(parent_monitor, use_full_area);
            // Minimize locking by only locking once and reusing the values
            let (cols, rows) = (grid_cols, grid_rows);
            let cell_w = (monitor_rect.right - monitor_rect.left) / cols as i32;
            let cell_h = (monitor_rect.bottom - monitor_rect.top) / rows as i32;
            let new_x = monitor_rect.left + parent_col as i32 * cell_w;
            let new_y = monitor_rect.top + parent_row as i32 * cell_h;

            // Only add to active_windows if grid is enabled and there is space
            let grid_state = grid_state_arc.lock().unwrap();
            let grid_slots = (rows as usize) * (cols as usize);

            if grid_state.is_some() && active_windows.len() < grid_slots {
                if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                    // Compute reserved cell index and coordinates directly
                    let parent_cell_idx = (parent_row * grid_state.cols + parent_col) as usize;
                    let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
                        / grid_state.cols as i32;
                    let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
                        / grid_state.rows as i32;
                    let new_x = grid_state.monitor_rect.left + parent_col as i32 * cell_w;
                    let new_y = grid_state.monitor_rect.top + parent_row as i32 * cell_h;

                    //grid_state.hwnd_to_cell.insert(hwnd, parent_cell_idx);
                    // Move/resize window as before

                    if is_console && fit_grid {
                        let mut test_h = cell_h;
                        let min_h = 100;
                        let mut success = false;
                        while test_h >= min_h {
                            SetWindowPos(
                                hwnd,
                                std::ptr::null_mut(),
                                new_x,
                                new_y,
                                cell_w,
                                test_h,
                                SWP_NOZORDER,
                            );
                            sleep(Duration::from_millis(100));
                            let mut rect = std::mem::zeroed();
                            if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                                let actual_x = rect.left;
                                let actual_y = rect.top;
                                let actual_h = rect.bottom - rect.top;
                                if actual_x == new_x && actual_y == new_y && (actual_h - test_h).abs() < 8 {
                                    success = true;
                                    println!("Console window moved and resized to height {}", test_h);
                                    break;
                                }
                            }
                            test_h -= 40;
                        }
                        if !success {
                            println!("Warning: Could not fit console window to grid cell, even after shrinking.");
                        }
                    } else {
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            new_x,
                            new_y,
                            if fit_grid && !is_console { cell_w } else { 0 },
                            if fit_grid && !is_console { cell_h } else { 0 },
                            if fit_grid && !is_console {
                                SWP_NOZORDER
                            } else {
                                SWP_NOSIZE | SWP_NOZORDER
                            },
                        );
                    }

                    grid_state.set_parent_cell(Some(parent_cell_idx), hwnd);
                    // Mark the reserved cell as occupied
                    grid_state.cells[parent_cell_idx] = GridCell {
                        hwnd: Some(hwnd),
                        filled_at: Some(Instant::now()),
                    };
                    println!("Reserved parent cell {} for HWND {:?}", parent_cell_idx, hwnd);
                }
                if reserve_parent_cell {
                    if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                        let (parent_row, parent_col, _) = assign_parent_cell.unwrap_or((0, 0, None));
                        let parent_cell_idx = (parent_row * grid_state.cols + parent_col) as usize;
                        grid_state.cells[parent_cell_idx] = GridCell {
                            hwnd: Some(hwnd),
                            filled_at: Some(Instant::now()),
                        };
                        println!("Reserved parent cell {} for HWND {:?}", parent_cell_idx, hwnd);
                    }
                }
                active_windows.push((hwnd, pid, class_name.clone(), bounds));
                hwnd_start_times.insert(hwnd, Instant::now());
            } else {
                // Stage the parent window if grid is full
                println!("Staging parent HWND {:?} (PID: {}) for later grid placement", hwnd, pid);
                staged_windows.push_back((hwnd, pid, class_name.clone(), bounds));
                // Optionally, place behind frontmost window:
                SetWindowPos(
                    hwnd,
                    winapi::um::winuser::HWND_BOTTOM,
                    0, 0, 0, 0,
                    winapi::um::winuser::SWP_NOMOVE | SWP_NOSIZE,
                );
            }

            if keep_open {
    let parent_hwnd_val = {
        let phwnd = parent_hwnd.lock().unwrap();
        *phwnd
    };
    if let Some(hwnd_isize) = parent_hwnd_val {
        let hwnd = hwnd_isize as HWND;
        let hwnd_val = hwnd as isize;
        std::thread::spawn(move || {
            use winapi::um::winuser::{GetMessageW, TranslateMessage, DispatchMessageW, MSG, WM_CLOSE};
            let hwnd = hwnd_val as winapi::shared::windef::HWND;
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            loop {
                let ret = unsafe { GetMessageW(&mut msg, hwnd, 0, 0) };
                if ret <= 0 {
                    break;
                }
                if msg.message == WM_CLOSE {
                    println!("Intercepted WM_CLOSE for parent window, --keep-open is set, ignoring.");
                    continue; // Ignore the close message
                }
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    }
}
        } else {
                    // Move to grid cell if grid is enabled
                    if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                        // Now get the new window rect (frame may have changed)
                        let mut rect = std::mem::zeroed();
                        if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                            let win_width = rect.right - rect.left;
                            let win_height = rect.bottom - rect.top;
                            let (cell_idx, mut new_x, mut new_y) =
                                                            grid_state.next_position(win_width, win_height, fit_grid, grid_placement_mode);

                            // Before moving, check if the window is already at the correct position and size
                            let mut needs_move = true;
                            if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                                let current_x = rect.left;
                                let current_y = rect.top;
                                let current_w = rect.right - rect.left;
                                let current_h = rect.bottom - rect.top;
                                let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
                                    / grid_state.cols as i32;
                                let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
                                    / grid_state.rows as i32;
                                if current_x == new_x && current_y == new_y && current_w == cell_w && current_h == cell_h {
                                    needs_move = false;
                                }
                            }
                            if needs_move {
                                // Clamp as before
                                let min_x = grid_state.monitor_rect.left;
                                let min_y = grid_state.monitor_rect.top;
                                let max_x = grid_state.monitor_rect.right
                                    - if fit_grid {
                                        (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
                                            / grid_state.cols as i32
                                    } else {
                                        win_width
                                    };
                                let max_y = grid_state.monitor_rect.bottom
                                    - if fit_grid {
                                        (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
                                            / grid_state.rows as i32
                                    } else {
                                        win_height
                                    };
                                new_x = new_x.clamp(min_x, max_x);
                                new_y = new_y.clamp(min_y, max_y);

                                // Move/resize as before...
                                if fit_grid && !is_console {
                                    let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
                                        / grid_state.cols as i32;
                                    let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
                                        / grid_state.rows as i32;
                                    println!(
                                        "Resizing and moving HWND {:?} to grid cell: ({}, {}) size=({}, {})",
                                        hwnd, new_x, new_y, cell_w, cell_h
                                    );
                                    SetWindowPos(
                                        hwnd,
                                        std::ptr::null_mut(),
                                        new_x,
                                        new_y,
                                        cell_w,
                                        cell_h,
                    SWP_NOZORDER,
                                    );
                                } else {
                                    println!(
                                        "Moving child HWND {:?} to grid cell: ({}, {}) size=({}, {})",
                                        hwnd, new_x, new_y, win_width, win_height
                                    );
                                    SetWindowPos(
                                        hwnd,
                                        std::ptr::null_mut(),
                                        new_x,
                                        new_y,
                                        0,
                                        0,
                    SWP_NOSIZE | SWP_NOZORDER,
                                    );
                                }
                                // After moving, verify the window is at the expected position
                                let mut success = false;
                                if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                                    let actual_x = rect.left;
                                    let actual_y = rect.top;
                                    if actual_x == new_x && actual_y == new_y {
                                        success = true;
                                    }
                                }
                                if !success {
                                    println!("Warning: Could not fit console window to grid cell, even after moving.");
                                }
                            }
                        } else {
                            println!("Warning: Could not get window rect for HWND {:?}", hwnd);
                        }
                    }
                }

                {
                    let mut phwnd = parent_hwnd.lock().unwrap();
                    *phwnd = Some(hwnd as isize);
                }
                if should_hide_border {
                    println!("Hiding border for HWND {:?}", hwnd);
                    hide_window_border(hwnd);
                }
                if should_hide_title_bar {
                    println!("Hiding title bar for HWND {:?}", hwnd);
                    hide_window_title_bar(hwnd);
                }
                if flash_topmost_ms > 0 {
                    println!("Flashing HWND {:?} as topmost for {} ms...", hwnd, flash_topmost_ms);
                    flash_topmost(hwnd, flash_topmost_ms);
                }
                // Shake the window in a non-blocking way (spawn a thread)
                let hwnd_copy = hwnd as isize;
                // Spawn the shake thread and collect the JoinHandle
                let shake_handle = std::thread::spawn(move || {
                    let hwnd = hwnd_copy as HWND;
                    shake_window(hwnd, 10, shake_duration);
                });
                shake_handles.push(shake_handle);

                if was_minimized {
                    println!("Re-minimizing window: {:?}", hwnd);
                    ShowWindow(hwnd, SW_MINIMIZE);
                }
                // if !hwnd_start_times.contains_key(&hwnd) {
                //     hwnd_start_times.insert(hwnd, Instant::now());  // maybe a --time-out-all in the future?
                // }
            } else {
                eprintln!("Failed to get window placement for HWND {:?}", hwnd);
            }
        }

        if gui.is_empty() {
            // Find the HWND using the real PID
            if let Some(hwnd) = find_hwnd_by_pid(parent_pid) {
                println!("Found HWND = {:?}", hwnd);
                if should_hide_border {
                    println!("Hiding border for HWND {:?}", hwnd);
                    hide_window_border(hwnd);
                }
                if should_hide_title_bar {
                    println!("Hiding title bar for HWND {:?}", hwnd);
                    hide_window_title_bar(hwnd);
                }
if flash_topmost_ms > 0 {
    println!("Flashing HWND {:?} as topmost for {} ms...", hwnd, flash_topmost_ms);
    flash_topmost(hwnd, flash_topmost_ms);
}
                // Shake the window in a non-blocking way (spawn a thread)
                let hwnd_copy = hwnd as isize;
                let shake_handle = std::thread::spawn(move || {
                    let hwnd = hwnd_copy as HWND;
                    shake_window(hwnd, 10, shake_duration);
                });
                shake_handles.push(shake_handle);
                {
                    let mut phwnd = parent_hwnd.lock().unwrap();
                    *phwnd = Some(hwnd as isize);
                }
            } else {
                eprintln!("Failed to find HWND for PID {}", parent_pid);
                // Do not return early, just continue
            }
        }

        // Track which child HWNDs we've already shaken to avoid repeats
        let shaken_hwnds = Arc::new(Mutex::new(HashSet::<HWND>::new()));
        // Track HWNDs that failed to shake (e.g., GetWindowRect failed)
        let mut failed_hwnds: HashMap<isize, u32> = HashMap::new();
        const MAX_HWND_RETRIES: u32 = 3;
        let mut failed_pids: HashSet<u32> = HashSet::new();
        let mut last_child_pids: Vec<u32> = Vec::new();
        let mut last_occupancy: Option<Vec<Option<HWND>>> = None;

        // --- Child windows in follow_children loop ---
        while follow_children && running.load(Ordering::SeqCst) {
            // // Check if the parent process is still running by opening with minimal rights and waiting for its exit
            // let process_handle = unsafe { OpenProcess(winapi::um::winnt::SYNCHRONIZE, 0, parent_pid) };
            // if process_handle.is_null() {
            //     println!("Parent process {} has terminated. Exiting.", parent_pid);
            //     break;
            // }
            // // WaitForSingleObject returns WAIT_OBJECT_0 if the process has exited
            // let wait_result = unsafe { winapi::um::synchapi::WaitForSingleObject(process_handle, 0) };
            // if wait_result == winapi::um::winbase::WAIT_OBJECT_0 {
            //     println!("Parent process {} has terminated. Exiting.", parent_pid);
            //     unsafe { CloseHandle(process_handle); }
            //     break;
            // }
            // unsafe { CloseHandle(process_handle); }

            // let child_pids = get_child_pids(parent_pid);
            // println!("Child PIDs: {:?}", child_pids);


            // Use a HashSet to avoid duplicates and for faster lookup
            let mut child_pids: HashSet<u32> = startt::get_child_pids(parent_pid).into_iter().collect();
            let etw_pids = tracked_pids.lock().unwrap();
            child_pids.extend(etw_pids.iter().copied());

            // Only print if changed
            let mut child_pids_vec: Vec<u32> = child_pids.iter().copied().collect();
            child_pids_vec.sort_unstable();
            if child_pids_vec != last_child_pids {
                println!("Child PIDs (snapshot + ETW): {:?}", child_pids_vec);
                last_child_pids = child_pids_vec.clone();
            }

            if !follow_forver {
                // Check if any tracked process is still running OR parent HWND is still valid
                let mut any_alive = false;
                let mut all_pids = vec![parent_pid];
                all_pids.extend(child_pids.iter().copied());
                for pid in all_pids {
                    let handle = OpenProcess(winapi::um::winnt::SYNCHRONIZE, 0, pid);
                    if !handle.is_null() {
                        let wait_result = winapi::um::synchapi::WaitForSingleObject(handle, 0);
                        CloseHandle(handle);
                        if wait_result != winapi::um::winbase::WAIT_OBJECT_0 {
                            any_alive = true;
                            break;
                        }
                    }
                }
                // Also check if the parent HWND is still valid
                let parent_hwnd_val = {
                    let phwnd = parent_hwnd.lock().unwrap();
                    *phwnd
                };
                let parent_hwnd_alive = if let Some(hwnd_isize) = parent_hwnd_val {
                    let hwnd = hwnd_isize as HWND;
                    unsafe { winapi::um::winuser::IsWindow(hwnd) != 0 }
                } else {
                    false
                };
                println!(
                    "[DEBUG] any_alive: {}, parent_hwnd_alive: {}, !any_alive && !parent_hwnd_alive: {}",
                    any_alive, parent_hwnd_alive, !any_alive && !parent_hwnd_alive
                );
                if !any_alive && !parent_hwnd_alive {
                    println!("All tracked processes and parent window have terminated. Exiting.");
                    std::process::exit(0);
                }
            }
println!("Enumerating child windows for PIDs: {:?}", child_pids);
            // Use grid_state's DashMap to track HWNDs and their cell indices
            let mut hwnd_pid_map: Vec<(HWND, u32)> = Vec::new();
            extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
                let (child_pids_ptr, hwnd_pid_map_ptr) = unsafe { &mut *(lparam as *mut (&HashSet<u32>, &mut Vec<(HWND, u32)>)) };
                let mut process_id = 0;
                unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
                if child_pids_ptr.contains(&process_id) {
                    hwnd_pid_map_ptr.push((hwnd, process_id));
                }
                1
            }
            let mut hwnd_pid_map_inner = Vec::new();
            let mut data = (&child_pids, &mut hwnd_pid_map_inner);
            unsafe {
                EnumWindows(Some(enum_windows_proc), &mut data as *mut _ as isize);
            }
            hwnd_pid_map = hwnd_pid_map_inner;

            println!("hwnd_pid_map: {:?}", hwnd_pid_map);

            GridState::with_grid_state(|g| g.print_desktop_cells());

            // for (hwnd, pid) in &hwnd_pid_map {
            //     let mut class_name = [0u16; 256];
            //     let class_name_len = winapi::um::winuser::GetClassNameW(
            //                         *hwnd,
            //                         class_name.as_mut_ptr(),
            //                         class_name.len() as i32,
            //                     );
            //     let class_name_str = if class_name_len > 0 {
            //         OsString::from_wide(&class_name[..class_name_len as usize])
            //             .to_string_lossy()
            //             .to_string()
            //     } else {
            //         String::from("<unknown>")
            //     };
            //     let parent = unsafe { winapi::um::winuser::GetParent(*hwnd) };
            //     println!("Enumerated HWND {:?} (PID: {}) class: {} parent: {:?}", hwnd, pid, class_name_str, parent);
            // }

            for (hwnd, pid) in hwnd_pid_map.clone() {
                let mut title = [0u16; 256];
                let title_len = unsafe {
                    winapi::um::winuser::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32)
                };
                let title_str = if title_len > 0 {
                    OsString::from_wide(&title[..title_len as usize])
                        .to_string_lossy()
                        .to_string()
                } else {
                    String::from("<no title>")
                };
                let msg = format!("PID {} HWND {:?} title: {}", pid, hwnd, title_str);


                    // timeout hwnd_start_times
                    // Only perform eviction if all non-reserved cells are occupied (no None in occupancy)
                    if let Some(timeout) = timeout_secs {
                        let now = Instant::now();
                        let program_start = *PROGRAM_START;

                        if now.duration_since(program_start).as_secs() < timeout {
                            // Not alive long enough, skip eviction
                            continue;
                        }

                        if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                            let occupancy: Vec<Option<HWND>> = grid_state.cells.iter().map(|c| c.hwnd).collect();
                            if last_occupancy.as_ref() != Some(&occupancy) {
                                println!("Grid cell occupancy: {:?}", occupancy);
                                last_occupancy = Some(occupancy);
                            }
                            if unsafe { winapi::um::winuser::IsWindow(hwnd) } == 0 || unsafe { winapi::um::winuser::IsWindowVisible(hwnd) } == 0 {
                                continue;
                            }
                            if grid_state.has_been_filled_at_some_point() {
                                for (idx, cell) in grid_state.cells.iter_mut().enumerate() {
                                    if let (Some(hwnd), Some(filled_at)) = (cell.hwnd, cell.filled_at) {
                                        let elapsed = now.duration_since(filled_at).as_secs();
                                        if elapsed >= timeout {
                                            // Don't close the reserved/parent cell
                                            if Some(idx) == grid_state.reserved_cell {
                                                continue;
                                            }
                                            println!("Evicting HWND {:?} from cell {} due to timeout (periodic check)", hwnd, idx);
                                            unsafe {
                                                winapi::um::winuser::PostMessageW(
                                                    hwnd,
                                                    winapi::um::winuser::WM_CLOSE,
                                                    0,
                                                    0,
                                                );
                                            }
                                            *cell = GridCell { hwnd: None, filled_at: None };
                                            break; // one at a time.
                                        }
                                    }
                                }
                            } else {
                                 println!("Eviction skipped: open cells exist in grid. {} of {}", grid_state.filled_count, grid_state.cells.len());
                            }
                        }
                    }
                // Print HWND info: class name and window type (top-level/child)
                let mut class_name = [0u16; 256];
                let class_name_len = winapi::um::winuser::GetClassNameW(
                    hwnd,
                    class_name.as_mut_ptr(),
                    class_name.len() as i32,
                );
                let class_name_str = if class_name_len > 0 {
                    OsString::from_wide(&class_name[..class_name_len as usize])
                        .to_string_lossy()
                        .to_string()
                } else {
                    String::from("<unknown>")
                };
                                let mut title = [0u16; 256];
                let title_len = unsafe {
                    winapi::um::winuser::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32)
                };
                let title_str = if title_len > 0 {
                    OsString::from_wide(&title[..title_len as usize])
                        .to_string_lossy()
                        .to_string()
                } else {
                    String::from("<no title>")
                };

                let is_console = class_name_str == "ConsoleWindowClass";
                // Skip windows with class name "NVOpenGLPbuffer" or starting with "wgpu Device Class"
                if class_name_str == "NVOpenGLPbuffer"
                    || class_name_str.starts_with("wgpu Device Class")
                    || class_name_str.eq_ignore_ascii_case("MSCTFIME UI")
                    || class_name_str.eq_ignore_ascii_case("Default IME")
                    || class_name_str.starts_with("temp_d3d_window_")
                    || class_name_str == "Winit Thread Event Target"
                {
                    // println!(
                    //     "Skipping HWND {:?} (PID: {}) due to class name: {}",
                    //     hwnd, pid, class_name_str
                    // );
                    continue;
                }
                if unsafe { winapi::um::winuser::IsWindow(hwnd) } == 0 {
                    continue;
                }
                if unsafe { winapi::um::winuser::IsWindowVisible(hwnd) } == 0 {
                    continue;
                }
                if unsafe { winapi::um::winuser::GetParent(hwnd) } != std::ptr::null_mut() {
                    continue;
                }

                    if let Some(fail_count) = failed_hwnds.get(&(hwnd as isize)) {
                        if *fail_count >= MAX_HWND_RETRIES {
                            println!(
                                "Skipping HWND {:?} (PID: {}) because it failed {} times (max retries reached) {}",
                                hwnd, pid, fail_count, title_str
                            );
                            continue;
                        } else {
                            println!(
                                "Retrying HWND {:?} (PID: {}) - attempt {}/{}",
                                hwnd, pid, fail_count + 1, MAX_HWND_RETRIES
                            );
                        }
                    }
                    if failed_pids.contains(&pid) {
                        continue;
                    }

                let this_parent_hwnd = winapi::um::winuser::GetParent(hwnd);
                if let Ok(phwnd_guard) = parent_hwnd.lock() {
                    if let Some(parent_hwnd_isize) = *phwnd_guard {
                        if hwnd == parent_hwnd_isize as HWND {
                            println!("skipping parent");
                            // Skip the parent window so it is not moved again
                            continue;
                        }
                    }
                } 
                let window_type = if this_parent_hwnd.is_null() {
                    "Top-level"
                } else {
                    // println!(
                    //     "Skipping child HWND {:?} (PID: {}) with parent HWND {:?}",
                    //     hwnd, pid, this_parent_hwnd
                    // );
                    continue;
                };

                // let mut rect = std::mem::zeroed();
                // if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) == 0 {
                //     if !failed_hwnds.contains_key(&(hwnd as isize)) {
                //         eprintln!(
                //             "Failed to get window rect for HWND {:?} (PID: {})",
                //             hwnd, pid
                //         );
                //         // --- PATCH START: Evict from grid if present ---
                //         if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                //             if let Some(idx) = grid_state.hwnd_to_cell.remove(&hwnd).map(|(_, idx)| idx) {
                //                     println!("Evicted HWND {:?} from grid cell {}", hwnd, idx);
                //                 grid_state.cells[idx] = GridCell { hwnd: None, filled_at: None };
                //             }
                //         }
                //         // --- PATCH END ---
                //     }
                //     *failed_hwnds.entry(hwnd as isize).or_insert(0) += 1;
                //     continue;
                // }

                // println!(
                //     "Shaking child HWND {:?} (PID: {}) at rect: left={}, top={}, right={}, bottom={} | Class: {} | Type: {}",
                //     hwnd,
                //     pid,
                //     rect.left,
                //     rect.top,
                //     rect.right,
                //     rect.bottom,
                //     class_name_str,
                //     window_type
                // );
                // Only now do we move to a grid cell and shake
                GridState::with_grid_state(|g| {
                    let result = g.assign_window_to_grid_cell(
                        hwnd,
                        fit_grid,
                        grid_placement_mode,
                        retain_parent_focus,
                        retain_launcher_focus,
                        timeout_secs,
                    );
                    match result {
                        Some(cell_idx) => {
                            println!("Assigned HWND {:?} to grid cell {}", hwnd, cell_idx);
                        }
                        None => {
                            println!("Failed to assign HWND {:?} to grid", hwnd);
                        }
                    }
                });
                continue;

                // if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {





//                     // let win_width = rect.right - rect.left;
//                     // let win_height = rect.bottom - rect.top;

//                     // Find the first available cell (empty or timed out)
//                     // Avoid borrowing grid_state mutably inside the closure
//                     let cell_indices: Vec<usize> = (0..grid_state.cells.len()).collect();
//                                        let mut found_idx = None;
//                     for idx in cell_indices {
//                         if grid_state.is_cell_available(idx, timeout_secs) && Some(idx) != grid_state.reserved_cell {
//                             found_idx = Some(idx);
//                             break;
//                         }
//                     }

//                     // --- PATCH START: Use next_position only once and use its result for both cell index and coordinates ---
//                     let (cell_idx, new_x, new_y) = if let Some(idx) = found_idx {
//                         // Use the found available cell and compute its coordinates
//                         let row = idx / grid_state.cols as usize;
//                         let col = idx % grid_state.cols as usize;
//                         let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left) / grid_state.cols as i32;
//                         let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top) / grid_state.rows as i32;
//                         let x = grid_state.monitor_rect.left + col as i32 * cell_w;
//                         let y = grid_state.monitor_rect.top + row as i32 * cell_h;
//                         if fit_grid {
//                             (idx, x, y)
//                         } else {
//                             let mut cx = x + (cell_w - win_width) / 2;
//                             let mut cy = y + (cell_h - win_height) / 2;
//                             let min_x = grid_state.monitor_rect.left;
//                             let min_y = grid_state.monitor_rect.top;
//                             let max_x = grid_state.monitor_rect.right - win_width;
//                             let max_y = grid_state.monitor_rect.bottom - win_height;
//                             cx = cx.clamp(min_x, max_x);
//                             cy = cy.clamp(min_y, max_y);
//                             (idx, cx, cy)
//                         }
//                     } else {
//                         // Fallback: use next_position as before (may evict/overlay if all cells are busy)
//                         let (fallback_idx, fallback_x, fallback_y) = grid_state.next_position(
//                             win_width,
//                             win_height,
//                             fit_grid,
//                             placement_mode,
//                         );
//                         eprintln!("Warning: All grid cells are busy, using fallback cell {}", fallback_idx);
//                         grid_state.check_and_fix_grid_sync();
//                         (fallback_idx, fallback_x, fallback_y)
//                     };
//                     // --- PATCH END ---

//                     // --- PATCH START: Only assign to the cell if it is empty ---
//                     if grid_state.cells[cell_idx].hwnd.is_none() {
//                         grid_state.cells[cell_idx] = GridCell {
//                             hwnd: Some(hwnd),
//                             filled_at: Some(Instant::now()),
//                         };
//                         // Start eviction timer if needed
//                         if grid_state.has_been_filled_at_some_point() {
//                             if let Some(timeout) = timeout_secs {
//                                 grid_state.cells[cell_idx].start_eviction_timer(cell_idx, timeout);
//                             }
//                         } else {
//                             println!("Cell {} was occupied after move, skipping assignment.", cell_idx);
//                                     }
//                     }
//                     // --- PATCH END ---

//                     // Clamp as before
//                     let min_x = grid_state.monitor_rect.left;
//                     let min_y = grid_state.monitor_rect.top;
//                     let max_x = grid_state.monitor_rect.right
//                         - if fit_grid {
//                             (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
//                                 / grid_state.cols as i32
//                         } else {
//                             win_width
//                         };
//                     let max_y = grid_state.monitor_rect.bottom
//                         - if fit_grid {
//                             (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
//                                 / grid_state.rows as i32
//                         } else {
//                             win_height
//                         };
//                     let new_x = new_x.clamp(min_x, max_x);
//                     let new_y = new_y.clamp(min_y, max_y);

//                     // Move/resize as before...
//                     if fit_grid && !is_console {
//                         let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
//                             / grid_state.cols as i32;
//                         let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
//                             / grid_state.rows as i32;
//                         println!(
//                             "Resizing and moving HWND {:?} to grid cell: ({}, {}) size=({}, {})",
//                             hwnd, new_x, new_y, cell_w, cell_h
//                         );
//                         SetWindowPos(
//                             hwnd,
//                             std::ptr::null_mut(),
//                             new_x,
//                             new_y,
//                             cell_w,
//                             cell_h,
//                             SWP_NOZORDER,
//                         );
//                     } else {
//                         println!(
//                             "Moving child HWND {:?} to grid cell: ({}, {}) size=({}, {})",
//                             hwnd, new_x, new_y, win_width, win_height
//                         );
//                         SetWindowPos(
//                             hwnd,
//                             std::ptr::null_mut(),
//                             new_x,
//                             new_y,
//                             0,
//                             0,
//                             SWP_NOSIZE | SWP_NOZORDER,
//                         );
//                     }
//                     // After moving and verifying the window:
//                     let mut rect = std::mem::zeroed();
//                     if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
//                         let actual_x = rect.left;
//                         let actual_y = rect.top;
//                         // Only assign after move/verify!
// if actual_x == new_x && actual_y == new_y {
//     if grid_state.cells[cell_idx].hwnd.is_none() {
//         grid_state.cells[cell_idx] = GridCell {
//             hwnd: Some(hwnd),
//             filled_at: Some(Instant::now()),
//         };
//         // Start eviction timer if needed
//         if grid_state.has_been_filled_at_some_point() {
//             if let Some(timeout) = timeout_secs {
//                 grid_state.cells[cell_idx].start_eviction_timer(cell_idx, timeout);
//             }
//         }
//     // } else {
//     //     println!("Cell {} was occupied after move, skipping assignment.", cell_idx);
//     // }
// } else {
//     println!(
//         "Warning: HWND {:?} did not move to expected position (wanted: {},{} got: {},{})",
//         hwnd, new_x, new_y, actual_x, actual_y
//     );
//     // Optionally: try the previous cell again by not incrementing filled_count
//     // (You may want to retry the assignment here)
// }
                //     } else {
                //         println!(
                //             "Warning: Could not verify position for HWND {:?}",
                //             hwnd
                //         );
                //         // Optionally: try the previous cell again by not incrementing filled_count
                //     }
                // }

                if should_hide_border {
                    println!("Hiding border for HWND {:?}", hwnd);
                    hide_window_border(hwnd);
                }
                if should_hide_title_bar {
                    println!("Hiding title bar for HWND {:?}", hwnd);
                    hide_window_title_bar(hwnd);
                }

if flash_topmost_ms > 0 {
    println!("Flashing HWND {:?} as topmost for {} ms...", hwnd, flash_topmost_ms);
    flash_topmost(hwnd, flash_topmost_ms);
}

       if shaken_hwnds.lock().unwrap().contains(&hwnd) {
                        println!("Skipping HWND {:?} (PID: {}) because it was already shaken {} ", hwnd, pid,title_str);
                        if let Some(ref gs) = *grid_state_arc.lock().unwrap() {
                            let occupancy: Vec<Option<HWND>> = gs.cells.iter().map(|c| c.hwnd).collect();
                            println!("Grid cell occupancy: {:?}", occupancy);
                        }
                        if title_str == "<no title>" {
                            println!("HWND {:?} (PID: {}) has no title, sending WM_CLOSE", hwnd, pid);
                            unsafe {
                                winapi::um::winuser::PostMessageW(
                                    hwnd,
                                    winapi::um::winuser::WM_CLOSE,
                                    0,
                                    0,
                                );
                            }
                            continue;
                        }
                        if unsafe { winapi::um::winuser::IsWindow(hwnd) } == 0 {
                            println!("HWND {:?} (PID: {}) is no longer valid, removing from shaken_hwnds", hwnd, pid);
                            shaken_hwnds.lock().unwrap().remove(&hwnd);
                            continue;
                        }
                        continue;
                    }
                // Shake the window in a non-blocking way (spawn a thread)
                // let hwnd_copy = hwnd as isize;
                // std::thread::spawn(move || {
                //     let hwnd = hwnd_copy as HWND;
                //     shake_window(hwnd, 10, shake_duration);
                // });
                let shaken_hwnds = {
                    let set = shaken_hwnds.lock().unwrap();
                    Arc::new(Mutex::new(set.iter().map(|h| *h as isize).collect::<HashSet<isize>>()))
                };
                let hwnd_copy = hwnd as isize;
                let shaken_hwnds_clone = Arc::clone(&shaken_hwnds);
                std::thread::spawn(move || {
                    let hwnd = hwnd_copy as HWND;
                    shake_window(hwnd, 10, shake_duration);
                    let mut set = shaken_hwnds_clone.lock().unwrap();
                    set.insert(hwnd_copy);
                });
                // No need to reassign shaken_hwnds; just use the Arc<Mutex<...>> as is.
                if !hwnd_start_times.contains_key(&hwnd) {
                    hwnd_start_times.insert(hwnd, Instant::now());
                }
            }
          std::thread::sleep(std::time::Duration::from_millis(2000)); 

            }

            // // When a window closes, free its cell:
            // active_windows.retain(|(hwnd, pid, _, _)| {
            //     let handle = OpenProcess(winapi::um::winnt::SYNCHRONIZE, 0, *pid);
            //     let still_running = if !handle.is_null() {
            //         let wait_result = unsafe { winapi::um::synchapi::WaitForSingleObject(handle, 0) };
            //         CloseHandle(handle);
            //         wait_result != winapi::um::winbase::WAIT_OBJECT_0
            //     } else {
            //         false };
            //     if !still_running {
            //         if let Some(ref mut grid_state) = grid_state {

            //             if let Some(hwnd) = grid_state.cells[idx].hwnd.take() {
            //                 grid_state.hwnd_to_cell.remove(&hwnd);
            //             }
            //             grid_state.cells[idx].hwnd = None;
            //             grid_state.cells[idx].filled_at = None;

            //         }
            //         println!("Grid window HWND {:?} (PID: {}) closed, freeing slot", hwnd, pid);
            //     }
            //     still_running
            // });

            // --- Promotion logic: when a grid window closes, promote staged window ---
            // Remove closed windows from active_windows and promote staged windows
            let mut grid_state = grid_state_arc.lock().unwrap();
            let grid_slots = {
                let grid_state = grid_state_arc.lock().unwrap();
                grid_state.as_ref().unwrap().rows * grid_state.as_ref().unwrap().cols
            } as usize;
            while active_windows.len() >= grid_slots && !staged_windows.is_empty() {
                // Only lock when you need to access grid_state
                let (parent_hwnd_val, reserved_cell) = {
                    let grid_state = grid_state_arc.lock().unwrap();
                    let parent_hwnd_val = {
                        let phwnd = parent_hwnd.lock().unwrap();
                        *phwnd
                    };
                    let reserved_cell = grid_state.as_ref().and_then(|g| g.reserved_cell);
                    (parent_hwnd_val, reserved_cell)
                };

                // Find the oldest non-parent, non-reserved window
                let oldest = active_windows.iter()
                    .filter(|(hwnd, _, _, _)| {
                        Some(*hwnd as isize) != parent_hwnd_val && // not parent
                        if let Some(ref grid_state) = grid_state.as_ref() {
                            // not in reserved cell
                            !grid_state.reserved_cell.map_or(false, |idx| grid_state.cells[idx].hwnd == Some(*hwnd))
                        } else { true }
                    })
                    .min_by_key(|(hwnd, _, _, _)| hwnd_start_times.get(hwnd).cloned().unwrap_or(Instant::now()));

                if let Some((evict_hwnd, evict_pid, _, _)) = oldest {
                    println!("Evicting oldest HWND {:?} (PID: {}) to make room in grid", evict_hwnd, evict_pid);
                    // Optionally, send WM_CLOSE or move it out of the grid
                    unsafe {
                        winapi::um::winuser::PostMessageW(
                            *evict_hwnd,
                            winapi::um::winuser::WM_CLOSE,
                            0,
                            0,
                        );
                    }
                    // Remove from grid_state and hwnd_start_times now, but defer removal from active_windows
                    if let Some(ref mut grid_state) = grid_state_arc.lock().unwrap().as_mut() {
                        if let Some(idx) = grid_state.hwnd_to_cell.remove(&evict_hwnd).map(|(_, idx)| idx) {
                            grid_state.cells[idx] = GridCell { hwnd: None, filled_at: None };
                        }
                    }
                    hwnd_start_times.remove(evict_hwnd);
                    // Defer removal from active_windows to after the borrow ends
                    let evict_hwnd_val = *evict_hwnd;
                    // Remove after the borrow ends
                    drop(oldest);
                    active_windows.retain(|(hwnd, _, _, _)| *hwnd != evict_hwnd_val);
                } else {
                    // No eligible window to evict
                    break;
                }
            }

            // Now promote as before, but only if there is a non-reserved, empty cell
            while active_windows.len() < grid_slots {
                let can_promote = if let Some(ref grid_state) = *grid_state {
                    grid_state.cells.iter().enumerate().any(|(idx, c)| {
                        c.hwnd.is_none() && Some(idx) != grid_state.reserved_cell
                    })
                } else {
                    false
                };
                if !can_promote {
                    break;
                }
                if let Some((hwnd, pid, class_name, bounds)) = staged_windows.pop_front() {
                    println!("Promoting staged HWND {:?} (PID: {}) to grid", hwnd, pid);
                    if let Some(ref mut grid_state_opt) = grid_state.as_mut() {
                        let grid_state = grid_state_opt;
                        let win_width = bounds.2;
                        let win_height = bounds.3;
                        let (cell_idx, new_x, new_y) = grid_state.next_position(
                            win_width,
                            win_height,
                            fit_grid,
                            grid_placement_mode,
                        );
                        // Move/resize window as before
                        if fit_grid && class_name != "ConsoleWindowClass" {
                            let cell_w = (grid_state.monitor_rect.right - grid_state.monitor_rect.left)
                                / grid_state.cols as i32;
                            let cell_h = (grid_state.monitor_rect.bottom - grid_state.monitor_rect.top)
                                / grid_state.rows as i32;
                            SetWindowPos(
                                hwnd,
                                std::ptr::null_mut(),
                                new_x,
                                new_y,
                                cell_w,
                                cell_h,
                                SWP_NOZORDER,
                            );
                        } else {
                            SetWindowPos(
                                hwnd,
                                std::ptr::null_mut(),
                                new_x,
                                new_y,
                                0,
                                0,
                                SWP_NOSIZE | SWP_NOZORDER,
                            );
                        }

                        // After moving, verify the window is at the expected position
                        let mut rect = std::mem::zeroed();
                        if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) != 0 {
                            let actual_x = rect.left;
                            let actual_y = rect.top;
                            if actual_x == new_x && actual_y == new_y {
                                println!("Successfully promoted HWND {:?} to grid cell {}", hwnd, cell_idx);
                                grid_state.cells[cell_idx] = GridCell {
                                    hwnd: Some(hwnd),
                                    filled_at: Some(Instant::now()),
                                }; // Mark cell as occupied
                                // if let Some(timeout) = timeout_secs {
                                //     grid_state.cells[cell_idx].start_eviction_timer(cell_idx, timeout);
                                // }
                            } else {
                                println!(
                                    "Warning: Promoted HWND {:?} did not move to expected position (wanted: {},{} got: {},{})",
                                    hwnd, new_x, new_y, actual_x, actual_y
                                );
                            }
                        } else {
                            println!(
                                "Warning: Could not verify position for promoted HWND {:?}",
                                hwnd
                            );
                        }
                    }
                    active_windows.push((hwnd, pid, class_name.clone(), bounds));
                    hwnd_start_times.insert(hwnd, Instant::now());
                } else {
                    break;
                }
            }
        }
        if !shake_handles.is_empty() {
            println!("Waiting for {} shake handles to finish...", shake_handles.len());
            for handle in shake_handles {
                let _ = handle.join();
            }
        }
        println!("Finished processing windows.");
        Ok(())
}

   
fn flash_topmost(hwnd: HWND, duration_ms: u64) {
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

fn hide_window_title_bar(hwnd: HWND) {
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

fn hide_window_border(hwnd: HWND) {
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
use once_cell::sync::Lazy;
use winapi::um::winuser::WindowFromPoint;

fn hide_taskbar_on_monitor(monitor_index: i32) {
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

fn show_taskbar_on_monitor(monitor_index: i32) {
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
impl GridCell {
    fn start_eviction_timer(&self, idx: usize, timeout: u64) {
        if let Some(hwnd) = self.hwnd {
            let hwnd_val = hwnd as isize;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(timeout));
                    let hwnd = hwnd_val as HWND;
                    println!("Evicting HWND {:?} from cell {} due to timeout (cell self-evict)", hwnd, idx);
                    unsafe {
                        winapi::um::winuser::PostMessageW(
                            hwnd,
                            winapi::um::winuser::WM_CLOSE,
                            0,
                            0,
                        );
                    }
                        //*cell = GridCell { hwnd: None, filled_at: None };
            });
        }
    }
}


unsafe extern "system" fn win_event_proc(
    _hWinEventHook: HWINEVENTHOOK,
    event: DWORD,
    hwnd: HWND,
    _idObject: c_long,
    _idChild: c_long,
    _dwEventThread: DWORD,
    _dwmsEventTime: DWORD,
) {
        println!("\n\n\n\nwin_event_proc: event={} hwnd={:?}", event, hwnd);
    if event == EVENT_OBJECT_DESTROY {
                if let Some(ref tx) = HOOK_SENDER {
                    println!("Sending HWND {:?} to channel", hwnd);
            let _ = tx.send(hwnd as usize);
        }
        // if 
        // GridState::with(|grid| {
        //     if let Some((_, idx)) = grid.hwnd_to_cell.remove(&hwnd) {
                println!("(HOOK) Window destroyed: HWND {:?}", hwnd);

                // Print title, class, pid, and process runtime
                let mut title = [0u16; 256];
                let title_len = unsafe {
                    winapi::um::winuser::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32)
                };
                let title_str = if title_len > 0 {
                    std::ffi::OsString::from_wide(&title[..title_len as usize])
                        .to_string_lossy()
                        .to_string()
                } else {
                    String::from("<no title>")
                };

                let mut class_name = [0u16; 256];
                let class_name_len = unsafe {
                    winapi::um::winuser::GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32)
                };
                let class_name_str = if class_name_len > 0 {
                    std::ffi::OsString::from_wide(&class_name[..class_name_len as usize])
                        .to_string_lossy()
                        .to_string()
                } else {
                    String::from("<unknown>")
                };

                let mut pid: u32 = 0;
                unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid) };

                // Try to get process runtime (since PROGRAM_START)
                let runtime = Instant::now().duration_since(*PROGRAM_START).as_secs();

                println!(
                    "(HOOK) Destroyed window info: Title: '{}', Class: '{}', PID: {}, Runtime: {}s",
                    title_str, class_name_str, pid, runtime
                );
                // grid.cells[idx] = GridCell { hwnd: None, filled_at: None };
        //     }
        // });
     }
}

// Call this after creating your grid_state:
pub fn install_window_destroy_hook(_grid_state: Arc<Mutex<Option<GridState>>>) -> winapi::shared::windef::HWINEVENTHOOK {
    println!("Installing window destroy hook...");
    unsafe {
        let hook = SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_DESTROY,
            std::ptr::null_mut(),
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        // Start a message loop in a new thread so your main thread isn't blocked
        std::thread::spawn(move || {
            let mut msg = std::mem::zeroed();
            while winapi::um::winuser::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                winapi::um::winuser::TranslateMessage(&msg);
                winapi::um::winuser::DispatchMessageW(&msg);
            }
        });
        hook
    }
}

// pub fn kill_process_and_children(parent_pid: u32) {
//     use winapi::um::processthreadsapi::OpenProcess;
//     use winapi::um::winnt::PROCESS_TERMINATE;
//     use winapi::um::processthreadsapi::TerminateProcess;
//     use winapi::um::handleapi::CloseHandle;

//     // 1. Get all child PIDs recursively
//     let mut pids = get_child_pids(parent_pid);
//     // 2. Add the parent itself
//     pids.push(parent_pid);

//     // 3. Kill each process
//     for pid in pids {
//         unsafe {
//             let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
//             if !handle.is_null() {
//                 println!("Killing PID {}", pid);
//                 TerminateProcess(handle, 1);
//                 CloseHandle(handle);
//             } else {
//                 println!("Failed to open PID {} for termination", pid);
//             }
//         }
//     }
// }