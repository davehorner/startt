// src/gui.rs
use crossbeam_channel::{Receiver, unbounded};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
pub(crate) struct StarttApp {
    message: String,
    #[serde(skip)]
    pub cmdline: String,
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
            cmdline: String::new(),
            output_lines_rx: None,
            output_lines: Vec::new(),
            pending_cmd: None,
            child: Arc::new(Mutex::new(None)),
            output_mode: Arc::new(Mutex::new(OutputMode::default())),
            last_scroll_interaction: Arc::new(Mutex::new(Some(Instant::now()))),
            stick_to_bottom: Arc::new(Mutex::new(true)),
            force_scroll_jump: None,
            message: format!("startt v{}{}", env!("CARGO_PKG_VERSION"), {
                let (y, m, d) = (
                    option_env!("BUILD_YEAR"),
                    option_env!("BUILD_MONTH"),
                    option_env!("BUILD_DAY"),
                );
                if let (Some(y), Some(m), Some(d)) = (y, m, d) {
                    let build_date = chrono::NaiveDate::from_ymd_opt(
                        y.parse().unwrap_or(1970),
                        m.parse().unwrap_or(1),
                        d.parse().unwrap_or(1),
                    );
                    if let Some(build_date) = build_date {
                        let days_ago = (chrono::Utc::now().date_naive() - build_date).num_days();
                        format!(" (built {} days ago)", days_ago)
                    } else {
                        String::from(" (build date unknown)")
                    }
                } else {
                    String::from(" (build date unknown)")
                }
            }),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Default)]
enum OutputMode {
    #[default]
    FollowBottom,

    Reverse,
}

const MAX_OUTPUT_LINES: usize = 1000;

impl eframe::App for StarttApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Drain output lines from the channel (lock free)
        if let Some(rx) = &self.output_lines_rx {
            while let Ok(line) = rx.try_recv() {
                self.output_lines.push(line);
                if self.output_lines.len() > MAX_OUTPUT_LINES {
                    let excess = self.output_lines.len() - MAX_OUTPUT_LINES;
                    self.output_lines.drain(0..excess);
                }
            }
        }

        let bevy_demo_dir = r"C:\w\demos\bevy";
        let bevy_demo_exists = std::path::Path::new(bevy_demo_dir).exists();

        // 2. If a new command is pending, spawn the process and output thread
        if let Some(args) = self.pending_cmd.take() {
            let (tx, rx) = unbounded();
            self.output_lines.clear();
            self.output_lines_rx = Some(rx);
            let child_arc = self.child.clone();
            let is_bevy_demo = args
                == vec![
                    "--follow",
                    "--grid",
                    "5x5m1",
                    "--fit-grid",
                    "--timeout",
                    "5",
                    "--hide-title-bar",
                    "--flash-topmost",
                    "--shake-duration",
                    "50",
                    "--hide-taskbar",
                    "--hide-border",
                    "-rpf",
                    "-rpc",
                    "--assign-parent-cell",
                    "0x2",
                    "--keep-open",
                    "cargo-e",
                    "-f",
                    "--run-all",
                    "--run-at-a-time",
                    "27",
                ];
            let current_dir = if is_bevy_demo && bevy_demo_exists {
                Some(bevy_demo_dir)
            } else {
                None
            };
            std::thread::spawn({
                let ctx = ctx.clone();
                move || {
                    let mut cmd = Command::new("startt");
                    cmd.args(&args);
                    if let Some(dir) = current_dir {
                        cmd.current_dir(dir);
                    }
                    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
                    let mut child = cmd.spawn().expect("Failed to launch startt");
                    let stdout = child.stdout.take().unwrap();
                    {
                        let mut child_lock = child_arc.lock().unwrap();
                        *child_lock = Some(child);
                    }
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            let _ = tx.send(line);
                            ctx.request_repaint();
                        }
                    }
                }
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(format!("startt v{}{}", env!("CARGO_PKG_VERSION"), {
                let (y, m, d) = (
                    option_env!("BUILD_YEAR"),
                    option_env!("BUILD_MONTH"),
                    option_env!("BUILD_DAY"),
                );
                if let (Some(y), Some(m), Some(d)) = (y, m, d) {
                    let build_date = chrono::NaiveDate::from_ymd_opt(
                        y.parse().unwrap_or(1970),
                        m.parse().unwrap_or(1),
                        d.parse().unwrap_or(1),
                    );
                    if let Some(build_date) = build_date {
                        let days_ago = (chrono::Utc::now().date_naive() - build_date).num_days();
                        format!(" (built {} days ago)", days_ago)
                    } else {
                        String::from(" (build date unknown)")
                    }
                } else {
                    String::from(" (build date unknown)")
                }
            }));
            let child_running = self.child.lock().unwrap().is_some();
            ui.label("Command line:");

            ui.horizontal(|ui| {
                ui.add_sized(
                    [ui.available_width() - 100.0, 40.0], // leave space for button
                    egui::TextEdit::multiline(&mut self.cmdline)
                        .hint_text("Enter command line arguments here")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                let button_label = if child_running { "Stop" } else { "Run" };
                let button = egui::Button::new(button_label).min_size(egui::vec2(80.0, 40.0));
                if ui.add(button).clicked() {
                    if child_running {
                        if let Ok(mut child_lock) = self.child.lock() {
                            if let Some(child) = child_lock.take() {
                                let pid = child.id();
                                crate::kill_process_and_children(pid);
                            }
                        }
                    } else {
                        let args: Vec<String> = self
                            .cmdline
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect();
                        if !args.is_empty() {
                            self.pending_cmd = Some(args);
                        }
                    }
                }
            });

            // Combo box for output mode (keep as before)
            let mut output_mode = self.output_mode.lock().unwrap();
            let mut mode_changed = false;
            egui::ComboBox::from_label("Output Mode")
                .selected_text(match *output_mode {
                    OutputMode::FollowBottom => "Follow bottom",
                    OutputMode::Reverse => "Reverse",
                })
                .show_ui(ui, |ui| {
                    mode_changed |= ui
                        .selectable_value(
                            &mut *output_mode,
                            OutputMode::FollowBottom,
                            "Follow bottom",
                        )
                        .clicked();
                    mode_changed |= ui
                        .selectable_value(&mut *output_mode, OutputMode::Reverse, "Reverse")
                        .clicked();
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
                        if let Some(child) = child_lock.take() {
                            let pid = child.id();
                            crate::kill_process_and_children(pid);
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
                        "--grid".into(),
                        "5x5m1".into(),
                        "--fit-grid".into(),
                        "--timeout".into(),
                        "5".into(),
                        "--hide-title-bar".into(),
                        "--flash-topmost".into(),
                        "--shake-duration".into(),
                        "50".into(),
                        "--hide-taskbar".into(),
                        "--hide-border".into(),
                        "-rpf".into(),
                        "-rpc".into(),
                        "--assign-parent-cell".into(),
                        "0x2".into(),
                        "--keep-open".into(),
                        "cargo-e".into(),
                        "-f".into(),
                        "--run-all".into(),
                        "--run-at-a-time".into(),
                        "27".into(),
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
                *self.last_scroll_interaction.lock().unwrap() = Some(Instant::now());
                *stick_to_bottom = false;
            } else if self
                .last_scroll_interaction
                .lock()
                .unwrap()
                .unwrap_or(Instant::now())
                .elapsed()
                > Duration::from_secs(5)
            {
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
                        ui.add(egui::Label::new(""))
                            .scroll_to_me(Some(egui::Align::TOP));
                    }
                    ui.label(&display_text);
                    if let Some(OutputMode::FollowBottom) = force_scroll_jump {
                        // Place a dummy widget after the output and scroll to it (bottom)
                        ui.add(egui::Label::new(""))
                            .scroll_to_me(Some(egui::Align::BOTTOM));
                    }
                });

            // In the egui panel, if the Bevy demo directory does not exist, show a prompt and a button to clone/setup Bevy
            if !bevy_demo_exists {
                ui.colored_label(egui::Color32::YELLOW, "Bevy demo directory not found: C:/w/demos/bevy");
                if ui.button("Clone Bevy and install dependencies").clicked() {
                    // Spawn a thread to run the setup command
                    let (setup_tx, setup_rx) = unbounded();
                    self.output_lines.clear();
                    self.output_lines_rx = Some(setup_rx);
                    std::thread::spawn(move || {
                        let setup_cmd = "git clone https://github.com/bevyengine/bevy.git && cd bevy && cargo install cargo-e startt";
                        let mut child = if cfg!(target_os = "windows") {
                            Command::new("cmd").args(["/C", setup_cmd]).current_dir("C:/w/demos").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
                        } else {
                            Command::new("sh").args(["-c", setup_cmd]).current_dir("C:/w/demos").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
                        }.expect("Failed to run setup command");
                        let stdout = child.stdout.take().unwrap();
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                let _ = setup_tx.send(line);
                            }
                        }
                    });
                }
            }
        });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Ok(mut child_lock) = self.child.lock() {
            if let Some(child) = child_lock.take() {
                let pid = child.id();
                crate::kill_process_and_children(pid);
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

pub fn fun_name() -> Option<std::result::Result<(), std::io::Error>> {
    // Collect command line arguments, remove "--gui" if present, and prepend "startt"
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--gui")
        .collect();
    let cmdline = args.join(" ");

    let options = eframe::NativeOptions {
        persist_window: true,
        persistence_path: Some(std::path::PathBuf::from("startt.json")),
        // app_id: Some("startt.egui.window".to_owned()), // <-- unique ID for your app
        ..Default::default()
    };
    // let _hook = unsafe {
    //     SetWinEventHook(
    //         EVENT_OBJECT_DESTROY,
    //         EVENT_OBJECT_DESTROY,
    //         std::ptr::null_mut(),
    //         Some(win_event_proc),
    //         0,
    //         0,
    //         WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
    //     )
    // };
    let _ = eframe::run_native(
        "startt",
        options,
        Box::new(|_cc| {
            Ok::<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>>(Box::new(
                StarttApp {
                    message: format!("startt v{}{}", env!("CARGO_PKG_VERSION"), {
                        let (y, m, d) = (
                            option_env!("BUILD_YEAR"),
                            option_env!("BUILD_MONTH"),
                            option_env!("BUILD_DAY"),
                        );
                        if let (Some(y), Some(m), Some(d)) = (y, m, d) {
                            let build_date = chrono::NaiveDate::from_ymd_opt(
                                y.parse().unwrap_or(1970),
                                m.parse().unwrap_or(1),
                                d.parse().unwrap_or(1),
                            );
                            if let Some(build_date) = build_date {
                                let days_ago =
                                    (chrono::Utc::now().date_naive() - build_date).num_days();
                                format!(" (built {} days ago)", days_ago)
                            } else {
                                String::from(" (build date unknown)")
                            }
                        } else {
                            String::from(" (build date unknown)")
                        }
                    }),
                    cmdline,
                    output_lines_rx: None,
                    output_lines: Vec::new(),
                    pending_cmd: None,
                    child: Arc::new(Mutex::new(None)),
                    output_mode: Arc::new(Mutex::new(OutputMode::default())),
                    last_scroll_interaction: Arc::new(Mutex::new(Some(Instant::now()))),
                    stick_to_bottom: Arc::new(Mutex::new(true)),
                    force_scroll_jump: None,
                },
            ))
        }),
    );
    return Some(Ok(()));
    // egui/eframe will run its own message pump, so no need to spawn a message loop thread here.
    None
}

// unsafe extern "system" fn win_event_proc(
//     _hWinEventHook: HWINEVENTHOOK,
//     event: DWORD,
//     hwnd: HWND,
//     _idObject: c_long,
//     _idChild: c_long,
//     _dwEventThread: DWORD,
//     _dwmsEventTime: DWORD,
// ) {
//         println!("\n\n\n\nwin_event_proc: event={} hwnd={:?}", event, hwnd);
//     if event == EVENT_OBJECT_DESTROY {
//                 if let Some(ref tx) = crate::HOOK_SENDER {
//                     println!("Sending HWND {:?} to channel", hwnd);
//             let _ = tx.send(hwnd as usize);
//         }
//         // if
//         // GridState::with(|grid| {
//         //     if let Some((_, idx)) = grid.hwnd_to_cell.remove(&hwnd) {
//                 println!("(HOOK) Window destroyed: HWND {:?}", hwnd);

//                 // Print title, class, pid, and process runtime
//                 let mut title = [0u16; 256];
//                 let title_len = unsafe {
//                     winapi::um::winuser::GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32)
//                 };
//                 let title_str = if title_len > 0 {
//                     std::ffi::OsString::from_wide(&title[..title_len as usize])
//                         .to_string_lossy()
//                         .to_string()
//                 } else {
//                     String::from("<no title>")
//                 };

//                 let mut class_name = [0u16; 256];
//                 let class_name_len = unsafe {
//                     winapi::um::winuser::GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32)
//                 };
//                 let class_name_str = if class_name_len > 0 {
//                     std::ffi::OsString::from_wide(&class_name[..class_name_len as usize])
//                         .to_string_lossy()
//                         .to_string()
//                 } else {
//                     String::from("<unknown>")
//                 };

//                 let mut pid: u32 = 0;
//                 unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid) };

//                 // Try to get process runtime (since PROGRAM_START)
//                 let runtime = Instant::now().duration_since(*crate::PROGRAM_START).as_secs();

//                 println!(
//                     "(HOOK) Destroyed window info: Title: '{}', Class: '{}', PID: {}, Runtime: {}s",
//                     title_str, class_name_str, pid, runtime
//                 );
//                 // grid.cells[idx] = GridCell { hwnd: None, filled_at: None };
//         //     }
//         // });
//      }
// }
