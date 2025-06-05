use once_cell::sync::OnceCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use winapi::shared::minwindef::{DWORD, FILETIME};
use winapi::shared::windef::HWND;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::{GetProcessTimes, OpenProcess};
use winapi::um::psapi::GetProcessImageFileNameW;
use winapi::um::winnt::{HANDLE, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use winapi::um::winuser::{EnumWindows, GetWindowThreadProcessId};

pub mod cli;
pub mod gui;
pub mod hwnd;

static INITIAL_HWND_SET: OnceCell<HashSet<isize>> = OnceCell::new();

pub fn snapshot_initial_hwnds() {
    let mut hwnd_set = HashSet::new();
    unsafe extern "system" fn enum_proc(
        hwnd: HWND,
        lparam: winapi::shared::minwindef::LPARAM,
    ) -> i32 {
        unsafe {
            let set = &mut *(lparam as *mut HashSet<isize>);
            set.insert(hwnd as isize);
            1
        }
    }
    unsafe {
        EnumWindows(
            Some(enum_proc),
            &mut hwnd_set as *mut _ as winapi::shared::minwindef::LPARAM,
        );
    }
    INITIAL_HWND_SET.set(hwnd_set).ok();
}

pub fn is_hwnd_new(hwnd: HWND) -> bool {
    if let Some(hwnd_set) = INITIAL_HWND_SET.get() {
        !hwnd_set.contains(&(hwnd as isize))
    } else {
        false
    }
}

// Example usage: find the oldest matching GUI app(s)
// Usage: find_oldest_recent_apps(&file.to_string_lossy(), 1)
// Returns the oldest (least recent) matching app(s)
pub fn find_oldest_recent_apps(
    program_name: &str,
    num_oldest: usize,
    parent_pid: Option<DWORD>,
    launching_pid: Option<DWORD>,
) -> Vec<(HWND, u32, String, (i32, i32, i32, i32))> {
    unsafe {
        struct EnumData {
            windows: Vec<(HWND, u32, u64, String, (i32, i32, i32, i32))>,
            target_program_name: String,
            launching_pid: Option<DWORD>,
            parent_time: Option<u64>,
            launching_time: Option<u64>,
        }

        extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
            let data = unsafe { &mut *(lparam as *mut EnumData) };

            if !is_hwnd_new(hwnd) {
                // Skip if hwnd existed at program start
                return 1; // Continue enumeration
            }

            // Check if the window has the WS_VISIBLE style
            let style = unsafe {
                winapi::um::winuser::GetWindowLongW(hwnd, winapi::um::winuser::GWL_STYLE)
            };
            if (style & winapi::um::winuser::WS_VISIBLE as i32) == 0 {
                return 1;
            }

            // Check if the window is a top-level window
            let parent_hwnd = unsafe { winapi::um::winuser::GetParent(hwnd) };
            if !parent_hwnd.is_null() {
                // skip non-top-level
            }

            // Get the process ID for the window
            let mut process_id = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut process_id);
            }

            // Skip if process_id matches launching_pid
            if let Some(launching_pid) = data.launching_pid {
                if process_id == launching_pid {
                    return 1;
                }
            }

            // Open the process to get its creation time and executable name
            let process_handle =
                unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id) };
            struct CleanupHandle {
                handle: HANDLE,
            }
            impl Drop for CleanupHandle {
                fn drop(&mut self) {
                    if !self.handle.is_null() {
                        unsafe { CloseHandle(self.handle) };
                    }
                }
            }
            let _cleanup = CleanupHandle {
                handle: process_handle,
            };
            if process_handle.is_null() {
                return 1;
            }

            // Get the executable name
            let mut exe_path = [0u16; 260];
            let exe_len = unsafe {
                GetProcessImageFileNameW(
                    process_handle,
                    exe_path.as_mut_ptr(),
                    exe_path.len() as u32,
                )
            };
            if exe_len == 0 {
                return 1;
            }
            let exe_name = OsString::from_wide(&exe_path[..exe_len as usize])
                .to_string_lossy()
                .to_string();

            // Get the class name of the window
            let mut class_name = [0u16; 256];
            let class_name_len = unsafe {
                winapi::um::winuser::GetClassNameW(
                    hwnd,
                    class_name.as_mut_ptr(),
                    class_name.len() as i32,
                )
            };
            let class_name_str = if class_name_len > 0 {
                OsString::from_wide(&class_name[..class_name_len as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                String::new()
            };

            // Check if the executable name contains the target program name
            if !exe_name
                .to_ascii_lowercase()
                .contains(&data.target_program_name.to_ascii_lowercase())
            {
                // If the target program name has no extension, try adding .exe or .com
                if !data.target_program_name.contains('.') {
                    let exe_name_with_ext = format!("{}.exe", data.target_program_name);
                    let com_name_with_ext = format!("{}.com", data.target_program_name);

                    if exe_name
                        .to_ascii_lowercase()
                        .contains(&exe_name_with_ext.to_ascii_lowercase())
                        || exe_name
                            .to_ascii_lowercase()
                            .contains(&com_name_with_ext.to_ascii_lowercase())
                    {
                        // match
                    } else {
                        return 1;
                    }
                } else {
                    return 1;
                }
            }

            // Get the process creation time
            let mut creation_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };

            let success = unsafe {
                GetProcessTimes(
                    process_handle,
                    &mut creation_time,
                    &mut exit_time,
                    &mut kernel_time,
                    &mut user_time,
                )
            };
            if success == 0 {
                return 1;
            }

            // Convert creation time to UNIX timestamp
            let creation_time_unix = filetime_to_unix_time(creation_time);

            // Filter out if creation_time_unix < parent_time or < launching_time
            if let Some(parent_time) = data.parent_time {
                if creation_time_unix < parent_time {
                    return 1;
                }
            }
            if let Some(launching_time) = data.launching_time {
                if creation_time_unix < launching_time {
                    return 1;
                }
            }

            // Get the window bounds
            let mut rect = unsafe { std::mem::zeroed() };
            if unsafe { winapi::um::winuser::GetWindowRect(hwnd, &mut rect) } == 0 {
                return 1;
            }
            let bounds = (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            );
            if bounds.2 == 0 || bounds.3 == 0 {
                let parent_hwnd = unsafe { winapi::um::winuser::GetParent(hwnd) };
                if !parent_hwnd.is_null() {
                    let mut parent_rect = unsafe { std::mem::zeroed() };
                    if unsafe { winapi::um::winuser::GetWindowRect(parent_hwnd, &mut parent_rect) }
                        != 0
                    {
                        let parent_bounds = (
                            parent_rect.left,
                            parent_rect.top,
                            parent_rect.right - parent_rect.left,
                            parent_rect.bottom - parent_rect.top,
                        );
                        data.windows.push((
                            parent_hwnd,
                            process_id,
                            creation_time_unix,
                            class_name_str,
                            parent_bounds,
                        ));
                    }
                }
                return 1;
            }
            data.windows
                .push((hwnd, process_id, creation_time_unix, class_name_str, bounds));
            1
        }

        // Helper to get process creation time by pid
        fn get_process_creation_time(pid: DWORD) -> Option<u64> {
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
                if handle.is_null() {
                    return None;
                }
                let mut creation_time = FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                };
                let mut exit_time = FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                };
                let mut kernel_time = FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                };
                let mut user_time = FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                };
                let ok = GetProcessTimes(
                    handle,
                    &mut creation_time,
                    &mut exit_time,
                    &mut kernel_time,
                    &mut user_time,
                );
                CloseHandle(handle);
                if ok == 0 {
                    None
                } else {
                    Some(filetime_to_unix_time(creation_time))
                }
            }
        }

        let program_name = if program_name.starts_with('"') && program_name.ends_with('"') {
            program_name
                .trim_matches('"')
                .rsplit('\\')
                .next()
                .unwrap_or(program_name)
                .to_string()
        } else {
            program_name
                .rsplit('\\')
                .next()
                .unwrap_or(program_name)
                .to_string()
        };

        let parent_time = parent_pid.and_then(get_process_creation_time);
        let launching_time = launching_pid.and_then(get_process_creation_time);

        let mut data = EnumData {
            windows: Vec::new(),
            target_program_name: program_name.to_string(),
            launching_pid,
            parent_time,
            launching_time,
        };

        EnumWindows(Some(enum_windows_proc), &mut data as *mut _ as isize);

        // Sort windows by creation time (oldest first)
        data.windows
            .sort_by_key(|&(_, _, creation_time, _, _)| creation_time);

        // Return the top `num_oldest` windows
        data.windows
            .into_iter()
            .take(num_oldest)
            .map(|(hwnd, pid, _, class_name, bounds)| (hwnd, pid, class_name, bounds))
            .collect::<Vec<_>>()
    }
}

// Converts a Windows FILETIME to a Unix timestamp (seconds since 1970-01-01)
pub fn filetime_to_unix_time(ft: FILETIME) -> u64 {
    // FILETIME is in 100-nanosecond intervals since January 1, 1601 (UTC)
    // UNIX epoch is January 1, 1970
    let windows_to_unix_epoch: u64 = 11644473600; // seconds
    let ticks = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
    let seconds = ticks / 10_000_000;
    if seconds < windows_to_unix_epoch {
        0
    } else {
        seconds - windows_to_unix_epoch
    }
}

pub fn find_most_recent_gui_apps(
    program_name: &str,
    num_recent: usize,
    parent_pid: Option<DWORD>,
    _launching_pid: Option<DWORD>,
) -> Vec<(HWND, u32, String, (i32, i32, i32, i32))> {
    unsafe {
        struct EnumData {
            windows: Vec<(HWND, u32, u64, String, (i32, i32, i32, i32))>,
            target_program_name: String,
            parent_pid: Option<DWORD>,
        }

        extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
            let data = unsafe { &mut *(lparam as *mut EnumData) };

            if !is_hwnd_new(hwnd) {
                // Skip if hwnd existed at program start
                return 1; // Continue enumeration
            }
            // println!("Enumerating HWND: {:?}", hwnd);

            // // Check if the window is visible
            // if unsafe { IsWindowVisible(hwnd) } == 0 {
            //     println!("HWND {:?} is not visible. Skipping.", hwnd);
            //     return 1; // Continue enumeration
            // }

            // Check if the window has the WS_VISIBLE style
            let style = unsafe {
                winapi::um::winuser::GetWindowLongW(hwnd, winapi::um::winuser::GWL_STYLE)
            };
            if (style & winapi::um::winuser::WS_VISIBLE as i32) == 0 {
                // println!("HWND {:?} does not have WS_VISIBLE style. Skipping.", hwnd);
                return 1; // Continue enumeration
            }

            // Check if the window is a top-level window
            let parent_hwnd = unsafe { winapi::um::winuser::GetParent(hwnd) };
            if !parent_hwnd.is_null() {
                println!("HWND {:?} is not a top-level window. Skipping.", hwnd);
                // return 1; // Skip non-top-level windows
            }

            // Get the process ID for the window
            let mut process_id = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut process_id);
            }
            // println!("HWND {:?} belongs to process ID: {}", hwnd, process_id);

            // Open the process to get its creation time and executable name
            let process_handle =
                unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id) };
            let _cleanup = CleanupHandle {
                handle: process_handle,
            };

            struct CleanupHandle {
                handle: HANDLE,
            }

            //oh rust, how I love thee...
            impl Drop for CleanupHandle {
                fn drop(&mut self) {
                    // println!("Cleaning up handle: {:?}", self.handle);
                    if !self.handle.is_null() {
                        unsafe { CloseHandle(self.handle) };
                    }
                }
            }
            if process_handle.is_null() {
                // println!("Failed to open process for PID {}. Skipping HWND {:?}.", process_id, hwnd);
                return 1; // Continue enumeration
            }

            // Get the executable name
            let mut exe_path = [0u16; 260];
            let exe_len = unsafe {
                GetProcessImageFileNameW(
                    process_handle,
                    exe_path.as_mut_ptr(),
                    exe_path.len() as u32,
                )
            };

            if exe_len == 0 {
                // println!("Failed to get executable name for PID {}. Skipping HWND {:?}.", process_id, hwnd);
                return 1; // Continue enumeration
            }

            let exe_name = OsString::from_wide(&exe_path[..exe_len as usize])
                .to_string_lossy()
                .to_string();
            // println!("Executable name for PID {}: {}", process_id, exe_name);

            // Get the class name of the window
            let mut class_name = [0u16; 256];
            let class_name_len = unsafe {
                winapi::um::winuser::GetClassNameW(
                    hwnd,
                    class_name.as_mut_ptr(),
                    class_name.len() as i32,
                )
            };

            let class_name_str = if class_name_len > 0 {
                OsString::from_wide(&class_name[..class_name_len as usize])
                    .to_string_lossy()
                    .to_string()
            } else {
                // eprintln!("Failed to get class name for HWND {:?}", hwnd);
                String::new()
            };
            // println!("Class name for HWND {:?}: {}", hwnd, class_name_str);

            // If the executable name is "cmd.exe", check the command-line arguments
            if exe_name.to_ascii_lowercase().ends_with("cmd.exe") {
                println!("Executable name is cmd.exe. Checking command-line arguments...");
                let mut cmdline = [0u16; 32768];
                let cmdline_len = unsafe {
                    {
                        let cmdline_ptr = winapi::um::processenv::GetCommandLineW();
                        if cmdline_ptr.is_null() {
                            0
                        } else {
                            let mut len = 0;
                            while *cmdline_ptr.add(len) != 0 {
                                cmdline[len] = *cmdline_ptr.add(len);
                                len += 1;
                            }
                            len as u32
                        }
                    }
                };

                if cmdline_len > 0 {
                    let cmdline_str = OsString::from_wide(&cmdline[..cmdline_len as usize])
                        .to_string_lossy()
                        .to_string();

                    // Check if the command-line arguments contain the target program name
                    if !cmdline_str
                        .to_ascii_lowercase()
                        .contains(&data.target_program_name.to_ascii_lowercase())
                    {
                        return 1; // Continue enumeration
                    }

                    println!("Command-line arguments for cmd.exe: {}", cmdline_str);
                } else {
                    // If we can't retrieve the command-line arguments, skip this window
                    return 1; // Continue enumeration
                }
            }

            // Check if the executable name contains the target program name
            if !exe_name
                .to_ascii_lowercase()
                .contains(&data.target_program_name.to_ascii_lowercase())
            {
                // If the target program name has no extension, try adding .exe or .com
                if !data.target_program_name.contains('.') {
                    let exe_name_with_ext = format!("{}.exe", data.target_program_name);
                    let com_name_with_ext = format!("{}.com", data.target_program_name);

                    if exe_name
                        .to_ascii_lowercase()
                        .contains(&exe_name_with_ext.to_ascii_lowercase())
                        || exe_name
                            .to_ascii_lowercase()
                            .contains(&com_name_with_ext.to_ascii_lowercase())
                    {
                        println!(
                            "Executable name '{}' matches target with extension '{}'.",
                            exe_name, data.target_program_name
                        );

                        if let Some(cmdline_str) = get_cmdline_for_pid(process_id) {
                            println!(
                                "Command-line arguments for PID {}: {}",
                                process_id, cmdline_str
                            );
                        }
                    } else {
                        return 1; // Continue enumeration
                    }
                } else {
                    return 1; // Continue enumeration
                }
                return 1; // Continue enumeration
            } else {
                println!(
                    "Executable name '{}' matches target '{}'.",
                    exe_name, data.target_program_name
                );

                // Get the command line of the target process, not the current process

                if let Some(cmdline_str) = get_cmdline_for_pid(process_id) {
                    println!(
                        "Command-line arguments for PID {}: {}",
                        process_id, cmdline_str
                    );
                }

                // Filter by command-line arguments: keep only if it contains "--w-pool-manager" and our parent_pid
                if let Some(cmdline_str) = get_cmdline_for_pid(process_id) {
                    if cmdline_str.contains("--w-pool-manager") {
                        if let Some(parent_pid) = data.parent_pid {
                            if cmdline_str.contains(&parent_pid.to_string()) {
                                println!(
                                    "PID {} has --w-pool-manager and parent_pid {} in command line: {}",
                                    process_id, parent_pid, cmdline_str
                                );
                                // } else {
                                //     // Does not contain parent_pid, skip this window
                                //     return 1;
                            }
                        } else {
                            // No parent_pid specified, just keep if --w-pool-manager is present
                            println!(
                                "PID {} has --w-pool-manager in command line: {}",
                                process_id, cmdline_str
                            );
                        }
                    }
                }
            }

            // Get the process creation time
            let mut creation_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exit_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user_time = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };

            let success = unsafe {
                GetProcessTimes(
                    process_handle,
                    &mut creation_time,
                    &mut exit_time,
                    &mut kernel_time,
                    &mut user_time,
                )
            };

            if success == 0 {
                println!(
                    "Failed to get process times for PID {}. Skipping HWND {:?}.",
                    process_id, hwnd
                );
                return 1; // Continue enumeration
            }

            // Convert creation time to UNIX timestamp
            let creation_time_unix = filetime_to_unix_time(creation_time);
            println!(
                "Creation time for PID {}: {}",
                process_id, creation_time_unix
            );

            // Get the window bounds
            let mut rect = unsafe { std::mem::zeroed() };
            if unsafe { winapi::um::winuser::GetWindowRect(hwnd, &mut rect) } == 0 {
                eprintln!("Failed to get window rect for HWND {:?}", hwnd);
                return 1; // Continue enumeration
            }

            let bounds = (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            );
            println!("Bounds for HWND {:?}: {:?}", hwnd, bounds);
            // If the bounds are zero, try to get the parent window
            if bounds.2 == 0 || bounds.3 == 0 {
                let parent_hwnd = unsafe { winapi::um::winuser::GetParent(hwnd) };
                if !parent_hwnd.is_null() {
                    println!(
                        "Bounds are zero for HWND {:?}. Using parent HWND {:?} instead.",
                        hwnd, parent_hwnd
                    );

                    // Get the bounds of the parent window
                    let mut parent_rect = unsafe { std::mem::zeroed() };
                    if unsafe { winapi::um::winuser::GetWindowRect(parent_hwnd, &mut parent_rect) }
                        != 0
                    {
                        let parent_bounds = (
                            parent_rect.left,
                            parent_rect.top,
                            parent_rect.right - parent_rect.left,
                            parent_rect.bottom - parent_rect.top,
                        );
                        println!(
                            "Bounds for parent HWND {:?}: {:?}",
                            parent_hwnd, parent_bounds
                        );

                        // Add the parent window to the list instead
                        data.windows.push((
                            parent_hwnd,
                            process_id,
                            creation_time_unix,
                            class_name_str,
                            parent_bounds,
                        ));
                    } else {
                        eprintln!(
                            "Failed to get bounds for parent HWND {:?}. Skipping.",
                            parent_hwnd
                        );
                    }
                } else {
                    eprintln!("No parent HWND found for HWND {:?}. Skipping.", hwnd);
                }
                return 1; // Continue enumeration
            }
            // Add the window to the list
            data.windows
                .push((hwnd, process_id, creation_time_unix, class_name_str, bounds));

            1 // Continue enumeration
        }

        let program_name = if program_name.starts_with('"') && program_name.ends_with('"') {
            program_name
                .trim_matches('"')
                .rsplit('\\')
                .next()
                .unwrap_or(program_name)
                .to_string()
        } else {
            program_name
                .rsplit('\\')
                .next()
                .unwrap_or(program_name)
                .to_string()
        };

        let mut data = EnumData {
            windows: Vec::new(),
            target_program_name: program_name.to_string(),
            parent_pid: parent_pid,
        };

        println!("Starting enumeration for program name: {}", program_name);
        EnumWindows(Some(enum_windows_proc), &mut data as *mut _ as isize);

        // Sort windows by creation time (most recent first)
        println!("Sorting windows by creation time...");
        data.windows
            .sort_by_key(|&(_, _, creation_time, _, _)| std::cmp::Reverse(creation_time));

        // Return the top `num_recent` windows
        let result = data
            .windows
            .into_iter()
            .take(num_recent)
            .map(|(hwnd, pid, _, class_name, bounds)| (hwnd, pid, class_name, bounds))
            .collect::<Vec<_>>();

        println!(
            "Found {} recent GUI apps matching '{}': {:?}",
            result.len(),
            program_name,
            result
        );
        result
    }
}

pub fn kill_process_and_children(parent_pid: u32) {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::processthreadsapi::TerminateProcess;
    use winapi::um::winnt::PROCESS_TERMINATE;

    // 1. Get all child PIDs recursively
    let mut pids = get_child_pids(parent_pid);
    // 2. Add the parent itself
    pids.push(parent_pid);

    // 3. Kill each process
    for pid in pids {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                println!("Killing PID {}", pid);
                TerminateProcess(handle, 1);
                CloseHandle(handle);
            } else {
                println!("Failed to open PID {} for termination", pid);
            }
        }
    }
}

#[allow(dead_code)]
pub fn get_child_pids(parent_pid: u32) -> Vec<u32> {
    use winapi::shared::minwindef::FALSE;
    use winapi::um::handleapi::CloseHandle;

    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next, TH32CS_SNAPPROCESS,
    };

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

use winapi::shared::ntdef::UNICODE_STRING;

/// Get the command line of a process by PID using winapi and direct memory reading.
/// Returns None if not accessible.
pub fn get_cmdline_for_pid(pid: u32) -> Option<String> {
    unsafe {
        // Open the process
        let h_process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h_process.is_null() {
            return None;
        }

        // Get PROCESS_BASIC_INFORMATION (undocumented struct)
        #[repr(C)]
        struct PROCESS_BASIC_INFORMATION {
            Reserved1: *mut std::ffi::c_void,
            PebBaseAddress: *mut std::ffi::c_void,
            Reserved2: [*mut std::ffi::c_void; 2],
            UniqueProcessId: *mut std::ffi::c_void,
            Reserved3: *mut std::ffi::c_void,
        }

        // NtQueryInformationProcess is not in winapi, so we declare it manually
        type NtQueryInformationProcessType = unsafe extern "system" fn(
            winapi::um::winnt::HANDLE,
            u32,
            *mut std::ffi::c_void,
            u32,
            *mut u32,
        ) -> i32;

        let ntdll = winapi::um::libloaderapi::GetModuleHandleA(b"ntdll.dll\0".as_ptr() as _);
        if ntdll.is_null() {
            CloseHandle(h_process);
            return None;
        }
        let proc_addr = winapi::um::libloaderapi::GetProcAddress(
            ntdll,
            b"NtQueryInformationProcess\0".as_ptr() as _,
        );
        if proc_addr.is_null() {
            CloseHandle(h_process);
            return None;
        }
        let nt_query_information_process: NtQueryInformationProcessType =
            std::mem::transmute(proc_addr);

        let mut pbi: PROCESS_BASIC_INFORMATION = std::mem::zeroed();
        let mut return_len = 0u32;
        let status = nt_query_information_process(
            h_process,
            0, // ProcessBasicInformation
            &mut pbi as *mut _ as *mut _,
            std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut return_len,
        );
        if status != 0 {
            CloseHandle(h_process);
            return None;
        }

        // Read PEB
        #[repr(C)]
        struct PEB {
            Reserved1: [u8; 2],
            BeingDebugged: u8,
            Reserved2: [u8; 1],
            Reserved3: [*mut std::ffi::c_void; 2],
            Ldr: *mut std::ffi::c_void,
            ProcessParameters: *mut RTL_USER_PROCESS_PARAMETERS,
        }
        #[repr(C)]
        struct RTL_USER_PROCESS_PARAMETERS {
            Reserved1: [u8; 16],
            Reserved2: [*mut std::ffi::c_void; 10],
            ImagePathName: UNICODE_STRING,
            CommandLine: UNICODE_STRING,
        }

        let mut peb: PEB = std::mem::zeroed();
        let mut bytes_read = 0;
        if winapi::um::memoryapi::ReadProcessMemory(
            h_process,
            pbi.PebBaseAddress as *mut winapi::ctypes::c_void,
            &mut peb as *mut _ as *mut _,
            std::mem::size_of::<PEB>(),
            &mut bytes_read,
        ) == 0
        {
            CloseHandle(h_process);
            return None;
        }

        // Read RTL_USER_PROCESS_PARAMETERS
        let mut upp: RTL_USER_PROCESS_PARAMETERS = std::mem::zeroed();
        if ReadProcessMemory(
            h_process,
            peb.ProcessParameters as *mut _,
            &mut upp as *mut _ as *mut _,
            std::mem::size_of::<RTL_USER_PROCESS_PARAMETERS>(),
            &mut bytes_read,
        ) == 0
        {
            CloseHandle(h_process);
            return None;
        }

        // Read the command line UNICODE_STRING buffer
        let len = upp.CommandLine.Length as usize / 2;
        let mut buffer = vec![0u16; len];
        if ReadProcessMemory(
            h_process,
            upp.CommandLine.Buffer as *mut _,
            buffer.as_mut_ptr() as *mut _,
            upp.CommandLine.Length as usize,
            &mut bytes_read,
        ) == 0
        {
            CloseHandle(h_process);
            return None;
        }

        CloseHandle(h_process);
        Some(String::from_utf16_lossy(&buffer))
    }
}
