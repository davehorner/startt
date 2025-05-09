// src/main.rs
use std::env;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::thread::sleep;
use std::time::Duration;
use widestring::U16CString;
use winapi::shared::minwindef::{FALSE, FILETIME};
use winapi::shared::windef::HWND;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::GetProcessId;
use winapi::um::processthreadsapi::{GetProcessTimes, OpenProcess};
use winapi::um::psapi::GetProcessImageFileNameW;
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, PROCESSENTRY32, Process32First, Process32Next, TH32CS_SNAPPROCESS,
};
use winapi::um::winnt::HANDLE;
use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use winapi::um::winuser::SWP_NOSIZE;
use winapi::um::winuser::SWP_NOZORDER;
use winapi::um::winuser::SetWindowPos;
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
        if snapshot == ptr::null_mut() {
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

    unsafe {
        // Take a snapshot of all processes
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == ptr::null_mut() {
            eprintln!("Failed to create process snapshot");
            return child_pids;
        }

        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        // Iterate through all processes
        if Process32First(snapshot, &mut entry) != FALSE {
            loop {
                if entry.th32ParentProcessID == parent_pid {
                    child_pids.push(entry.th32ProcessID);
                }

                if Process32Next(snapshot, &mut entry) == FALSE {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
    }

    child_pids
}

fn shake_window(hwnd: HWND, intensity: i32, duration_ms: u64) {
    unsafe {
        // Bring the window to the front
        winapi::um::winuser::SetForegroundWindow(hwnd);

        // Get the current position of the window
        let mut rect = std::mem::zeroed();
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

fn filetime_to_unix_time(ft: FILETIME) -> u64 {
    let high = (ft.dwHighDateTime as u64) << 32;
    let low = ft.dwLowDateTime as u64;
    // FILETIME is in 100-nanosecond intervals since January 1, 1601 (UTC)
    // Convert to seconds since UNIX epoch (January 1, 1970)
    (high | low) / 10_000_000 - 11_644_473_600
}

fn find_most_recent_gui_apps(
    program_name: &str,
    num_recent: usize,
) -> Vec<(HWND, u32, String, (i32, i32, i32, i32))> {
    unsafe {
        struct EnumData {
            windows: Vec<(HWND, u32, u64, String, (i32, i32, i32, i32))>,
            target_program_name: String,
        }

        extern "system" fn enum_windows_proc(hwnd: HWND, lparam: isize) -> i32 {
            let data = unsafe { &mut *(lparam as *mut EnumData) };

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
                .unwrap_or(&program_name)
                .to_string()
        } else {
            program_name
                .rsplit('\\')
                .next()
                .unwrap_or(&program_name)
                .to_string()
        };

        let mut data = EnumData {
            windows: Vec::new(),
            target_program_name: program_name.to_string(),
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let mut file = args
        .next()
        .expect("Usage: startt <executable|document|URL> [args...]");

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
        Some(U16CString::from_str(params)?)
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

        let parent_pid = GetProcessId(sei.hProcess);
        println!("Launched PID = {}", parent_pid);

        WaitForInputIdle(sei.hProcess, winapi::um::winbase::INFINITE);
        sleep(Duration::from_millis(2000));
        let gui = find_most_recent_gui_apps(&file.to_string_lossy(), 1);

        for (i, (hwnd, pid, class_name, bounds)) in gui.clone().into_iter().enumerate() {
            println!(
                "{}. HWND = {:?}, PID = {}, Class = {}, Bounds = {:?}",
                i + 1,
                hwnd,
                pid,
                class_name,
                bounds
            );

            // Check if the window is minimized
            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;

            if GetWindowPlacement(hwnd, &mut placement) != 0 {
                let was_minimized =
                    placement.showCmd == winapi::um::winuser::SW_SHOWMINIMIZED.try_into().unwrap();
                if was_minimized {
                    println!("Window {:?} is minimized. Restoring...", hwnd);
                    // Restore the window
                    ShowWindow(hwnd, SW_RESTORE);
                    // Wait briefly to ensure the window is restored
                    sleep(Duration::from_millis(500));
                }

                // Get screen dimensions
                let screen_width =
                    winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CXSCREEN);
                let screen_height =
                    winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CYSCREEN);

                // Save the original position of the window
                let original_position = (bounds.0, bounds.1);

                // Calculate new position to center the window with a 10% border around
                let border_x = (screen_width as f32 * 0.1) as i32;
                let border_y = (screen_height as f32 * 0.1) as i32;
                let new_x = border_x + (screen_width - 2 * border_x - bounds.2) / 2;
                let new_y = border_y + (screen_height - 2 * border_y - bounds.3) / 2;

                // Move the window to the calculated position
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    new_x,
                    new_y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );

                // Verify if the window actually moved
                let mut rect = std::mem::zeroed();
                if winapi::um::winuser::GetWindowRect(hwnd, &mut rect) == 0 {
                    eprintln!("Failed to get window rect for HWND {:?}", hwnd);
                    continue; // Skip this window if we can't get its rect
                }

                if rect.left != new_x || rect.top != new_y {
                    println!(
                        "Window {:?} did not move to the expected position. Skipping.",
                        hwnd
                    );
                    continue; // Skip this window if it didn't move
                }

                println!("Moved window to center with border: {:?}", hwnd);

                // Shake the window
                println!("Shaking window: {:?}", hwnd);
                shake_window(hwnd, 10, 2000);

                // Restore the original position
                println!("Restoring window to original position: {:?}", hwnd);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    original_position.0,
                    original_position.1,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );

                // If the window was minimized, minimize it again
                if was_minimized {
                    println!("Re-minimizing window: {:?}", hwnd);
                    ShowWindow(hwnd, SW_MINIMIZE);
                }
            }
            // // Get child PIDs
            // let child_pids = get_child_pids(parent_pid);
            // println!("Child PIDs: {:?}", child_pids);

            // // Optionally, find HWNDs for child processes
            // for child_pid in child_pids {
            //     if let Some(hwnd) = find_hwnd_by_pid(child_pid) {
            //         println!("Found HWND for child PID {}: {:?}", child_pid, hwnd);
            //     }
            // }
        }
        if gui.is_empty() {
            // Find the HWND using the real PID
            let hwnd = find_hwnd_by_pid(parent_pid).ok_or_else(|| {
                eprintln!("Failed to find HWND for PID {}", parent_pid);
                std::io::Error::new(std::io::ErrorKind::NotFound, "HWND not found")
            })?;
            println!("Found HWND = {:?}", hwnd);
            shake_window(hwnd, 10, 2000);
        }
        winapi::um::handleapi::CloseHandle(sei.hProcess);
    }

    Ok(())
}
