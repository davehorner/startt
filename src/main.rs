// src/main.rs
#[cfg(not(feature = "uses_etw"))]
#[allow(unused_imports)]
#[cfg(feature = "uses_etw")]
use ferrisetw::parser::Parser;
#[cfg(feature = "uses_etw")]
use ferrisetw::trace::UserTrace;
#[cfg(feature = "uses_etw")]
use ferrisetw::{EventRecord, SchemaLocator};
use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use widestring::U16CString;
use winapi::shared::minwindef::FALSE;
use winapi::shared::windef::HWND;
use winapi::shared::windef::{HMONITOR, POINT, RECT};
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

#[allow(dead_code)]
fn get_child_pids(parent_pid: u32) -> Vec<u32> {
    let mut child_pids = Vec::new();
    let mut stack = vec![parent_pid];

    unsafe {
        // Take a snapshot of all processes
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            eprintln!("Failed to create process snapshot");
            return child_pids;
        }

        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        // Collect all process entries into a Vec for easier traversal
        let mut all_entries = Vec::new();
        if Process32First(snapshot, &mut entry) != FALSE {
            loop {
                all_entries.push(entry);
                if Process32Next(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        }
        CloseHandle(snapshot);

        // Use a stack for DFS to collect all descendants
        while let Some(pid) = stack.pop() {
            for e in all_entries.iter() {
                if e.th32ParentProcessID == pid && !child_pids.contains(&e.th32ProcessID) {
                    child_pids.push(e.th32ProcessID);
                    stack.push(e.th32ProcessID);
                }
            }
        }
    }

    child_pids
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

// Add this struct for grid state:
struct GridState {
    rows: u32,
    cols: u32,
    monitor: i32,
    next_cell: usize,
    monitor_rect: RECT,
}

impl GridState {
    fn next_position(&mut self, win_width: i32, win_height: i32) -> (i32, i32) {
        let total_cells = (self.rows * self.cols) as usize;
        let cell = self.next_cell % total_cells;
        self.next_cell += 1;
        let row = cell / self.cols as usize;
        let col = cell % self.cols as usize;
        let cell_w = (self.monitor_rect.right - self.monitor_rect.left) / self.cols as i32;
        let cell_h = (self.monitor_rect.bottom - self.monitor_rect.top) / self.rows as i32;
        let x = self.monitor_rect.left + col as i32 * cell_w + (cell_w - win_width) / 2;
        let y = self.monitor_rect.top + row as i32 * cell_h + (cell_h - win_height) / 2;
        (x, y)
    }
}

// Helper to get monitor RECT by index (0 = primary)
fn get_monitor_rect(monitor_index: i32) -> RECT {
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
            let (target, found, rect_arc, count): &mut (
                i32,
                Arc<Mutex<bool>>,
                Arc<Mutex<RECT>>,
                Arc<Mutex<i32>>,
            ) = &mut *(lparam as *mut (i32, Arc<Mutex<bool>>, Arc<Mutex<RECT>>, Arc<Mutex<i32>>));
            let mut idx = count.lock().unwrap();
            if *idx == *target {
                let mut mi: MONITORINFO = std::mem::zeroed();
                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(hmonitor, &mut mi) != 0 {
                    let mut r = rect_arc.lock().unwrap();
                    *r = mi.rcWork;
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
            mi.rcWork
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Shared set for all tracked PIDs (parent + children)
    let running = Arc::new(AtomicBool::new(true));

    let mut grid: Option<(u32, u32, i32)> = None;
    let mut follow_children = false;
    let mut positional_args = Vec::new();

    let mut args = env::args_os().skip(1).peekable();
    while let Some(arg) = args.next() {
        let arg_str = arg.to_string_lossy();
        if arg_str == "-f" || arg_str == "--follow" {
            follow_children = true;
        } else if arg_str == "-g" || arg_str == "--grid" {
            let grid_arg = args
                .next()
                .expect("Expected ROWSxCOLS or ROWSxCOLSmDISPLAY# after -g/--grid");
            let grid_str = grid_arg.to_string_lossy();
            let (rows, cols, monitor) = parse_grid_arg(&grid_str);
            grid = Some((rows, cols, monitor));
            println!("Grid set to {}x{} on monitor {}", rows, cols, monitor);
        } else if arg_str.starts_with("-g") && arg_str.len() > 2 {
            // Support -g2x2 or -g2x2m1
            let grid_str = &arg_str[2..];
            let (rows, cols, monitor) = parse_grid_arg(grid_str);
            grid = Some((rows, cols, monitor));
            println!("Grid set to {}x{} on monitor {}", rows, cols, monitor);
        } else {
            positional_args.push(arg);
            // Push the rest as positional args
            positional_args.extend(args);
            break;
        }
    }
    println!("Arguments: {:?}", positional_args);
    let mut args = positional_args.into_iter();
    let mut file = args
        .next()
        .expect("Usage: startt [-f] [-g ROWSxCOLSmDISPLAY#] <executable|document|URL> [args...]");

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
    let file_w = U16CString::from_os_str(file.clone())?;
    let params_w = if params.is_empty() {
        None
    } else {
        Some(U16CString::from_str(&params)?)
    };

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
            return Err(Box::new(std::io::Error::last_os_error()));
        }
        // Get the PID of the process that launched us
        let launching_pid = get_parent_pid(std::process::id()).unwrap_or(0);
        println!("Launching PID (parent of this process): {}", launching_pid);
        let parent_pid = GetProcessId(sei.hProcess);
        let mut parent_hwnd = None;
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
            ctrlc::set_handler(move || {
                println!("\nCtrl+C pressed! Killing all child processes...");
                running.store(false, Ordering::SeqCst);
                let mut child_pids = get_child_pids(parent_pid);
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
            })?;
        }

        println!("Launched PID = {}", parent_pid);
        println!("Launched HWND = {:?}", sei.hwnd);
        println!("Launched file = {:?}", file);
        println!("Launching: file={:?} params={:?}", file, params);
        WaitForInputIdle(sei.hProcess, winapi::um::winbase::INFINITE);
        sleep(Duration::from_millis(1000));
        let mut gui = startt::find_oldest_recent_apps(
            &file.to_string_lossy(),
            1,
            Some(parent_pid),
            Some(launching_pid),
        );
        // If parent_hwnd is not in gui, check if parent is alive and use that hwnd
        if parent_hwnd.is_none() {
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
                        println!("Using parent HWND found by PID: {:?}", hwnd);
                        parent_hwnd = Some(hwnd);
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
                            gui = vec![(hwnd, parent_pid, String::from("parent"), bounds)];
                        } else {
                            // Fallback: use zero bounds if GetWindowRect fails
                            gui = vec![(hwnd, parent_pid, String::from("parent"), (0, 0, 0, 0))];
                        }
                    }
                } else {
                    println!("Parent process {} has terminated. Exiting.", parent_pid);
                }
                CloseHandle(handle);
            } else {
                println!("Parent process {} has terminated. Exiting.", parent_pid);
            }
        }
        // Create grid state if needed
        let mut grid_state: Option<GridState> = grid.map(|(rows, cols, monitor)| {
            let monitor_rect = get_monitor_rect(monitor);
            GridState {
                rows,
                cols,
                monitor,
                next_cell: 0,
                monitor_rect,
            }
        });
        if let Some(ref grid_state) = grid_state {
            println!(
                "Grid enabled: {}x{} on monitor {} (rect: left={}, top={}, right={}, bottom={})",
                grid_state.rows,
                grid_state.cols,
                grid_state.monitor,
                grid_state.monitor_rect.left,
                grid_state.monitor_rect.top,
                grid_state.monitor_rect.right,
                grid_state.monitor_rect.bottom
            );
        }
        // --- Parent window(s) ---
        for (i, (hwnd, pid, class_name, bounds)) in gui.clone().into_iter().enumerate() {
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

                // Move to grid cell if grid is enabled
                if let Some(ref mut grid_state) = grid_state {
                    let (win_width, win_height) = (bounds.2, bounds.3);
                    let (new_x, new_y) = grid_state.next_position(win_width, win_height);
                    println!(
                        "Moving HWND {:?} to grid cell: ({}, {}) size=({}, {})",
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

                parent_hwnd = Some(hwnd);
                println!("Shaking window: {:?}", hwnd);
                // Shake the window in a non-blocking way (spawn a thread)
                let hwnd_copy = hwnd as isize;
                std::thread::spawn(move || {
                    let hwnd = hwnd_copy as HWND;
                    shake_window(hwnd, 10, 2000);
                });

                if was_minimized {
                    println!("Re-minimizing window: {:?}", hwnd);
                    ShowWindow(hwnd, SW_MINIMIZE);
                }
            } else {
                eprintln!("Failed to get window placement for HWND {:?}", hwnd);
            }
        }

        if gui.is_empty() {
            // Find the HWND using the real PID
            let hwnd = find_hwnd_by_pid(parent_pid).ok_or_else(|| {
                eprintln!("Failed to find HWND for PID {}", parent_pid);
                std::io::Error::new(std::io::ErrorKind::NotFound, "HWND not found")
            })?;
            println!("Found HWND = {:?}", hwnd);
            // Shake the window in a non-blocking way (spawn a thread)
            let hwnd_copy = hwnd as isize;
            std::thread::spawn(move || {
                let hwnd = hwnd_copy as HWND;
                shake_window(hwnd, 10, 2000);
            });
            parent_hwnd = Some(hwnd);
        }

        // Track which child HWNDs we've already shaken to avoid repeats
        let mut shaken_hwnds = HashSet::new();
        // Track HWNDs that failed to shake (e.g., GetWindowRect failed)
        let mut failed_hwnds = HashSet::new();
        let mut failed_pids: HashSet<u32> = HashSet::new();

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
            let mut child_pids = get_child_pids(parent_pid);
            let etw_pids: Vec<u32> = tracked_pids.lock().unwrap().iter().copied().collect();
            for pid in etw_pids {
                if !child_pids.contains(&pid) {
                    child_pids.push(pid);
                }
            }
            println!("Child PIDs (snapshot + ETW): {:?}", child_pids);
            // Check if any tracked process is still running
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
            if !any_alive {
                println!("All tracked processes have terminated. Exiting.");
                break;
            }

            let mut _new_hwnds: Vec<HWND> = Vec::new();
            #[allow(unused_assignments)]
            let mut hwnd_pid_map = Vec::new(); // Track (HWND, PID) pairs
            extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
                let (child_pids, hwnds, hwnd_pid_map): &mut (
                    &Vec<u32>,
                    Vec<HWND>,
                    Vec<(HWND, u32)>,
                ) = unsafe { &mut *(lparam as *mut (&Vec<u32>, Vec<HWND>, Vec<(HWND, u32)>)) };
                let mut process_id = 0;
                unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
                if child_pids.contains(&process_id) {
                    hwnds.push(hwnd);
                    hwnd_pid_map.push((hwnd, process_id));
                }
                1
            }
            let hwnds = Vec::new();
            let hwnd_pid_map_inner = Vec::new();
            let mut data = (&child_pids, hwnds, hwnd_pid_map_inner);
            EnumWindows(Some(enum_windows_proc), &mut data as *mut _ as isize);
            _new_hwnds = data.1;
            hwnd_pid_map = data.2;

            for (hwnd, pid) in hwnd_pid_map {
                if shaken_hwnds.contains(&hwnd)
                    || failed_hwnds.contains(&hwnd)
                    || failed_pids.contains(&pid)
                {
                    continue;
                }
                let mut rect = std::mem::zeroed();
                if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) == 0 {
                    eprintln!(
                        "Failed to get window rect for HWND {:?} (PID: {})",
                        hwnd, pid
                    );
                    failed_hwnds.insert(hwnd);
                    failed_pids.insert(pid); // Mark this PID as failed
                    continue;
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
                // Skip windows with class name "NVOpenGLPbuffer" or starting with "wgpu Device Class"
                if class_name_str == "NVOpenGLPbuffer"
                    || class_name_str.starts_with("wgpu Device Class")
                {
                    println!(
                        "Skipping HWND {:?} (PID: {}) due to class name: {}",
                        hwnd, pid, class_name_str
                    );
                    continue;
                }
                let this_parent_hwnd = winapi::um::winuser::GetParent(hwnd);
                if Some(hwnd) == parent_hwnd {
                    println!("skipping parent");
                    // Skip the parent window so it is not moved again
                    continue;
                }
                let window_type = if this_parent_hwnd.is_null() {
                    "Top-level"
                } else {
                    println!(
                        "Skipping child HWND {:?} (PID: {}) with parent HWND {:?}",
                        hwnd, pid, this_parent_hwnd
                    );
                    continue;
                };
                println!(
                    "Shaking child HWND {:?} (PID: {}) at rect: left={}, top={}, right={}, bottom={} | Class: {} | Type: {}",
                    hwnd,
                    pid,
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    class_name_str,
                    window_type
                );
                // Only now do we move to a grid cell and shake
                if let Some(ref mut grid_state) = grid_state {
                    let win_width = rect.right - rect.left;
                    let win_height = rect.bottom - rect.top;
                    let (new_x, new_y) = grid_state.next_position(win_width, win_height);
                    println![
                        "Moving child HWND {:?} to grid cell: ({}, {}) size=({}, {})",
                        hwnd, new_x, new_y, win_width, win_height
                    ];
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

                // Shake the window in a non-blocking way (spawn a thread)
                let hwnd_copy = hwnd as isize;
                std::thread::spawn(move || {
                    let hwnd = hwnd_copy as HWND;
                    shake_window(hwnd, 10, 2000);
                });
                shaken_hwnds.insert(hwnd);
            }

            sleep(Duration::from_millis(2000));
        }

        // After the follow_children loop, restore/show the parent window
        if let Some(parent_hwnd) = gui.first().map(|(hwnd, _, _, _)| *hwnd) {
            println!(
                "Restoring and bringing parent HWND {:?} to front",
                parent_hwnd
            );
            ShowWindow(parent_hwnd, winapi::um::winuser::SW_SHOWNORMAL);
            winapi::um::winuser::SetForegroundWindow(parent_hwnd);
        }

        winapi::um::handleapi::CloseHandle(sei.hProcess);
    }

    Ok(())
}
