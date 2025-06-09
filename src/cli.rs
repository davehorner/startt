use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::env;

#[derive(Default, Clone, Debug)]
pub struct CommandLineOptions {
    pub follow_children: bool,
    pub follow_forever: bool,
    pub timeout_secs: Option<u64>,
    pub flash_topmost_ms: u64,
    pub should_hide_title_bar: bool,
    pub should_hide_border: bool,
    pub shake_duration: u64,
    pub fit_grid: bool,
    pub reserve_parent_cell: bool,
    pub assign_parent_cell: Option<(u32, u32, Option<i32>)>,
    pub hide_taskbar: bool,
    pub show_taskbar: bool,
    pub grid_placement_mode: GridPlacementMode,
    pub retain_parent_focus: bool,
    pub retain_launcher_focus: bool,
    pub keep_open: bool,
}

impl std::fmt::Display for CommandLineOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CommandLineOptions {{
    follow_children: {},
    follow_forever: {},
    timeout_secs: {:?},
    flash_topmost_ms: {},
    should_hide_title_bar: {},
    should_hide_border: {},
    shake_duration: {},
    fit_grid: {},
    reserve_parent_cell: {},
    assign_parent_cell: {:?},
    hide_taskbar: {},
    show_taskbar: {},
    grid_placement_mode: {:?},
    retain_parent_focus: {},
    retain_launcher_focus: {},
    keep_open: {}
}}",
            self.follow_children,
            self.follow_forever,
            self.timeout_secs,
            self.flash_topmost_ms,
            self.should_hide_title_bar,
            self.should_hide_border,
            self.shake_duration,
            self.fit_grid,
            self.reserve_parent_cell,
            self.assign_parent_cell,
            self.hide_taskbar,
            self.show_taskbar,
            self.grid_placement_mode,
            self.retain_parent_focus,
            self.retain_launcher_focus,
            self.keep_open
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridPlacementMode {
    FirstFree,
    Sequential,
}

impl Default for GridPlacementMode {
    fn default() -> Self {
        GridPlacementMode::FirstFree
    }
}

pub static CMD_OPTIONS: Lazy<DashMap<&'static str, CommandLineOptions>> = Lazy::new(DashMap::new);

/// Access the parsed command-line options.
pub fn get_command_line_options() -> CommandLineOptions {
    CMD_OPTIONS
        .get("options")
        .map(|entry| entry.value().clone())
        .unwrap_or_else(CommandLineOptions::default)
}

/// Update the command-line options.
pub fn update_command_line_options(new_options: CommandLineOptions) {
    CMD_OPTIONS.insert("options", new_options);
}

/// Prints the program name and version, then exits.
fn print_version_and_exit() -> ! {
    let exe = env::args().next().unwrap_or_else(|| "startt".to_string());
    let exe = exe
        .rsplit_once(std::path::MAIN_SEPARATOR)
        .map(|(_, file)| file)
        .unwrap_or(&exe)
        .split('.')
        .next()
        .unwrap_or(&exe);
    println!("{} {}", exe, env!("CARGO_PKG_VERSION"));
    std::process::exit(0);
}
pub fn parse_command_line() {
    let mut args = env::args_os().skip(1).peekable();
    // Get or insert default options for mutation
    let mut options = CMD_OPTIONS
        .entry("options")
        .or_insert_with(CommandLineOptions::default);

    while let Some(arg) = args.next() {
        let arg_str = arg.to_string_lossy();

        match arg_str.as_ref() {
            "--version" => print_version_and_exit(),
            "-f" | "--follow" => options.follow_children = true,
            "-F" | "--follow-forever" => {
                options.follow_children = true;
                options.follow_forever = true;
            }
            "-t" | "--timeout" => {
                let t_arg = args
                    .next()
                    .expect("Expected number of seconds after -t/--timeout");
                options.timeout_secs = Some(
                    t_arg
                        .to_string_lossy()
                        .parse()
                        .expect("Invalid timeout value"),
                );
            }
            "-T" | "--flash-topmost" => {
                // Accept an optional value (milliseconds)
                if let Some(val) = args.peek() {
                    if let Ok(ms) = val.to_string_lossy().parse::<u64>() {
                        args.next(); // consume
                        options.flash_topmost_ms = ms;
                    } else {
                        options.flash_topmost_ms = 10; // default
                    }
                } else {
                    options.flash_topmost_ms = 10; // default
                }
            }
            "-hT" | "--hide-title-bar" => options.should_hide_title_bar = true,
            "-hB" | "--hide-border" => options.should_hide_border = true,
            "--shake-duration" | "-sd" => {
                if let Some(duration_arg) = args.peek() {
                    if let Ok(duration) = duration_arg.to_string_lossy().parse::<u64>() {
                        options.shake_duration = duration;
                        args.next(); // Consume the value
                        println!("Shake duration set to {} ms", options.shake_duration);
                    } else {
                        panic!("Invalid shake duration value");
                    }
                } else {
                    panic!("Expected milliseconds after --shake-duration/-sd");
                }
            }
            "--fit-grid" | "-fg" => options.fit_grid = true,
            "--reserve-parent-cell" | "-rpc" => options.reserve_parent_cell = true,
            "--assign-parent-cell" | "-apc" => {
                options.assign_parent_cell = Some((0, 0, None)); // Example default value
            }
            "--retain-parent-focus" | "-rpf" => options.retain_parent_focus = true,
            "--retain-launcher-focus" | "-rlf" => options.retain_launcher_focus = true,
            "-ko" | "--keep-open" => options.keep_open = true,
            _ => {}
        }
    }
}
