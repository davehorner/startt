use winapi::um::winuser::{SetWinEventHook, EVENT_OBJECT_DESTROY};
use dashmap::DashMap;
//use std::sync::mpsc::{self, Receiver};
use crossbeam_channel::{unbounded ,Receiver, Sender};
use serde::{Serialize, Deserialize};
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
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};
use widestring::U16CString;
use winapi::shared::minwindef::FALSE;
use winapi::shared::windef::HWND;
use winapi::shared::windef::{HMONITOR, POINT, RECT};
// Window hook for automatic grid eviction on window destroy
use winapi::um::winuser::{WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, UnhookWinEvent};
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


#[derive(Serialize, Deserialize)]
pub(crate) struct StarttApp {
    message: String,
     #[serde(skip)]
    output_lines_rx: Option<Receiver<String>>,
     #[serde(skip)]
    output_lines: Vec<String>,
     #[serde(skip)]
    pending_cmd: Option<Vec<String>>, // <-- Add this
     #[serde(skip)]
    child: Arc<Mutex<Option<std::process::Child>>>,
     #[serde(skip)]
    output_mode: Arc<Mutex<OutputMode>>,
     #[serde(skip)]
    last_scroll_interaction: Arc<Mutex<Option<Instant>>>,
     #[serde(skip)]
    stick_to_bottom: Arc<Mutex<bool>>,
     #[serde(skip)]
    force_scroll_jump: Option<OutputMode>,
}

// Manual Default implementation because Instant does not implement Default
impl Default for StarttApp {
    fn default() -> Self {
        Self {
            output_lines_rx: None,
            output_lines: Vec::new(),
            pending_cmd: None,
            child: Arc::new(Mutex::new(None)),
            output_mode: Arc::new(Mutex::new(OutputMode::default())),
            last_scroll_interaction: Arc::new(Mutex::new(Some(Instant::now()))),
            stick_to_bottom: Arc::new(Mutex::new(true)),
            force_scroll_jump: None,
            message: String::from("Hello from egui!"),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Default)]
enum OutputMode {
    FollowBottom,
    #[default]
    Reverse,
}


impl eframe::App for StarttApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Drain output lines from the channel (lock free)
        if let Some(rx) = &self.output_lines_rx {
            while let Ok(line) = rx.try_recv() {
                self.output_lines.push(line);
            }
        }

        // 2. If a new command is pending, spawn the process and output thread
        if let Some(args) = self.pending_cmd.take() {
            let (tx, rx) = unbounded();
            self.output_lines.clear();
            self.output_lines_rx = Some(rx);
            let child_arc = self.child.clone();

            std::thread::spawn({
                let ctx = ctx.clone(); // <-- Add this to move ctx into the thread
                move || {
                    let mut child = Command::new("startt")
                        .args(&args)
                        .current_dir(r"C:\w\demos\bevy")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .expect("Failed to launch startt");
                    let stdout = child.stdout.take().unwrap();
                    {
                        let mut child_lock = child_arc.lock().unwrap();
                        *child_lock = Some(child);
                    }
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            let _ = tx.send(line);
                            ctx.request_repaint(); // <-- Add this line!
                        }
                    }
                }
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hello from egui!");

            // Combo box for output mode (keep as before)
            let mut output_mode = self.output_mode.lock().unwrap();
            let mut mode_changed = false;
            egui::ComboBox::from_label("Output Mode")
                .selected_text(match *output_mode {
                    OutputMode::FollowBottom => "Follow bottom",
                    OutputMode::Reverse => "Reverse",
                })
                .show_ui(ui, |ui| {
                    mode_changed |= ui.selectable_value(&mut *output_mode, OutputMode::FollowBottom, "Follow bottom").clicked();
                    mode_changed |= ui.selectable_value(&mut *output_mode, OutputMode::Reverse, "Reverse").clicked();
                });
            drop(output_mode);

            if mode_changed {
                let mut stick_to_bottom = self.stick_to_bottom.lock().unwrap();
                let output_mode = self.output_mode.lock().unwrap();
                if *output_mode == OutputMode::FollowBottom {
                    *stick_to_bottom = true;
                    self.force_scroll_jump = Some(OutputMode::FollowBottom);
                } else {
                    *stick_to_bottom = false;
                    self.force_scroll_jump = Some(OutputMode::Reverse);
                }
                ctx.request_repaint();
            }

            let child_running = self.child.lock().unwrap().is_some();
               if child_running {
        if ui.button("Stop").clicked() {
                if let Ok(mut child_lock) = self.child.lock() {
                    if let Some(mut child) = child_lock.take() {
                        let pid = child.id();
                        startt::kill_process_and_children(pid);
                    }
                }
            }
        } else {
            // Button: set pending_cmd to trigger a new process
            if ui.button("Run Bevy Grid Demo").clicked() {
                
// self.pending_cmd = Some(vec![
//     "--follow".into(),
//     "--grid".into(), "5x5m1".into(),
//     "--fit-grid".into(),
//     "--timeout".into(), "5".into(),
//     "--hide-title-bar".into(),
//     "--flash-topmost".into(),
//     "--shake-duration".into(), "50".into(),
//     "--hide-taskbar".into(),
//     "--hide-border".into(),
//     "-rpf".into(),
//     "-rpc".into(),
//     "--keep-open".into(),
//     "cmd.exe".into(),
//     [
//     "/k".into(),
//     // Join the command and prompt as a single argument for /k:
//         "cargo-e",
//         "-f",
//         "--run-all",
//         "--run-at-a-time",
//         "27",
//         "& echo.",
//         "& echo Press any key to close...",
//         "& pause"
//     ].join(" ").into(),
// ]);
                self.pending_cmd = Some(vec![
                    "--follow".into(),
                    "--grid".into(), "5x5m1".into(),
                    "--fit-grid".into(),
                    "--timeout".into(), "5".into(),
                    "--hide-title-bar".into(),
                    "--flash-topmost".into(),
                    "--shake-duration".into(), "50".into(),
                    "--hide-taskbar".into(),
                    "--hide-border".into(),
                    "-rpf".into(),
                    "-rpc".into(),
                    "--assign-parent-cell".into(),"0x2".into(),
                    "--keep-open".into(),
                    "cargo-e".into(),
                    "-f".into(),
                    "--run-all".into(),
                    "--run-at-a-time".into(), "27".into(),
                ]);
            }
        }

            let mut stick_to_bottom = self.stick_to_bottom.lock().unwrap();
    let mut wheel_interacted = false;

    for event in &ctx.input(|i| i.raw.events.clone()) {
        use egui::Event;
        match event {
            Event::MouseWheel { .. } => {
                wheel_interacted = true;
                break;
            }
            _ => {}
        }
    }

    if wheel_interacted {
        *self.last_scroll_interaction.lock().unwrap() =Some(Instant::now());
        *stick_to_bottom = false;
    } else if self.last_scroll_interaction.lock().unwrap().unwrap_or(Instant::now()).elapsed() > Duration::from_secs(5) {
        if !*stick_to_bottom {
            *stick_to_bottom = true;
            ctx.request_repaint();
        }
    }
            // Display output
            let output_mode = self.output_mode.lock().unwrap();
            let display_text = if *output_mode == OutputMode::Reverse {
                let mut lines = self.output_lines.clone();
                lines.reverse();
                lines.join("\n")
            } else {
                self.output_lines.join("\n")
            };
            let force_scroll_jump = self.force_scroll_jump.take();
egui::ScrollArea::vertical()
    .stick_to_bottom(*stick_to_bottom)
    .show(ui, |ui| {
        if let Some(OutputMode::Reverse) = force_scroll_jump {
            // Place a dummy widget before the output and scroll to it (top)
            ui.add(egui::Label::new("")).scroll_to_me(Some(egui::Align::TOP));
        }
        ui.label(&display_text);
        if let Some(OutputMode::FollowBottom) = force_scroll_jump {
            // Place a dummy widget after the output and scroll to it (bottom)
            ui.add(egui::Label::new("")).scroll_to_me(Some(egui::Align::BOTTOM));
        }
    });
        });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut child_lock) = self.child.lock() {
            if let Some(mut child) = child_lock.take() {
                            let pid = child.id();
            startt::kill_process_and_children(pid);
            }
        }
    }

}

// --- Add this Drop implementation for StarttApp ---
impl Drop for StarttApp {
    fn drop(&mut self) {
        if let Ok(mut child_lock) = self.child.lock() {
            if let Some(mut child) = child_lock.take() {
                let _ = child.kill();
            }
        }
    }
}

pub(crate) fn fun_name() -> Option<std::result::Result<(), std::io::Error>> {
    let options = eframe::NativeOptions {
        persist_window: true, 
        persistence_path: Some(std::path::PathBuf::from("startt.json")),
        // app_id: Some("startt.egui.window".to_owned()), // <-- unique ID for your app
        ..Default::default()
    };
    let _hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_DESTROY,
            std::ptr::null_mut(),
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let _ = eframe::run_native(
        "egui window",
        options,
        Box::new(|_cc| Ok::<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>>(Box::new(StarttApp::default()))),
    );
    return Some(Ok(()));
    // egui/eframe will run its own message pump, so no need to spawn a message loop thread here.
    None
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
                if let Some(ref tx) = crate::HOOK_SENDER {
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
                let runtime = Instant::now().duration_since(*crate::PROGRAM_START).as_secs();

                println!(
                    "(HOOK) Destroyed window info: Title: '{}', Class: '{}', PID: {}, Runtime: {}s",
                    title_str, class_name_str, pid, runtime
                );
                // grid.cells[idx] = GridCell { hwnd: None, filled_at: None };
        //     }
        // });
     }
}
