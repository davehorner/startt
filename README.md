feat(startt): hello 'startt' default open tool

`startt` solves a long-standing Windows poor design/quirk: when you do

  • `cmd.exe /c start …`  
  • Explorer “Open with” or protocol handler  
  • PowerShell `Start-Process`  

or anything that calls `ShellExecuteEx` under the covers (even with `SEE_MASK_NOCLOSEPROCESS` + `WaitForInputIdle`), the PID you get back is unusable for apps like Chrome.  You can’t reliably find its window handle (HWND) or true process ID for automation, testing, or demos.

This initial proof of concept implementation of `startt`:

- **Launches** your file/URL/command via `ShellExecuteExW(SEE_MASK_NOCLOSEPROCESS)`
- **Blocks** until the new process is idle (`WaitForInputIdle`)  
- **Snapshots** all child processes (`Toolhelp32Snapshot`) to catch helpers  
- **Enumerates** top-level windows (`EnumWindows`) and filters by PID or child-PID  
- **Matches** on executable name and creation time for robustness  
- **Centers**, **flashes**, and optionally **shakes** the found window for visual confirmation  
- **Restores** minimized windows if needed, and reports both parent and child PIDs
- **Reports** both parent and child PIDs
- **Ranks** remaining candidates by process **creation time**, selecting the most recent  
- **Filters** candidates by a few things look at the code:
  - **PID match** (parent or any child PID)  
  - **Executable name** contains the target command/document name  
- **Optionally follows and shakes child windows** with the `-f` or `--follow` flag:
  - When `-f` is specified, `startt` will continue to monitor for new child processes/windows spawned by the launched process, and shake each new window once as it appears.  
  - This is useful for apps that spawn additional windows after startup (e.g., browsers, editors, etc.).


**Usage:**
```
startt [-f|--follow] [-F|--follow-forever] [-g ROWSxCOLS|--grid ROWSxCOLS] [-t SECONDS|--timeout SECONDS] [-hT|--hide-title-bar] [-hB|--hide-border] [-T|--flash-topmost] [-sd MILLISECONDS|--shake-duration MILLISECONDS] <executable|document|URL> [args...]
```
- Use `-f` or `--follow` to keep watching for and shaking new child windows.
- Use `-F` or `--follow-forever` to keep watching for and shaking new child windows even after the parent has closed.
- Use `-t SECONDS` or `--timeout SECONDS` to specify the number of seconds each window should remain open before a quit message is sent to it. Each window is tracked individually; after the timeout, `startt` will send a WM_CLOSE message to that window.
- Use `-g ROWSxCOLS` or `--grid ROWSxCOLS` to tile each window into a grid on the primary monitor (e.g., `-g 2x2` for a 2x2 grid).
- Specify a monitor with `-g ROWSxCOLSmN` (e.g., `-g 2x2m1` for monitor 1, zero-based). `-gROWSxCOLSm#` is also valid.
- Use `-hT` or `--hide-title-bar` to hide the title bar of the target window.
- Use `-hB` or `--hide-border` to hide the border of the target window.
- Use `-T` or `--flash-topmost` to briefly set the window as topmost, then restore it.
- Use `-sd MILLISECONDS` or `--shake-duration MILLISECONDS` to set the shake animation duration in milliseconds (default: 2000ms).

**Examples:**

startt was developed for use with [cargo-e](https://crates.io/crates/cargo-e) but can be used with any application that pops a window on windows.

```
startt -f -g1x4 -t 10 -hT -T -sd 1500 cargo-e --run-all --run-at-a-time 4
```
This will shake and grid each window, hide the title bar, briefly flash the window as topmost, and use a 1.5 second shake duration. After 10 seconds, a quit message is sent to each window.
[![startt + cargo-e + bevy](https://github.com/davehorner/cargo-e_walkthrus/raw/main/startt_cargo-e_bevy_runall_4x1.gif)](https://github.com/davehorner/cargo-e_walkthrus/tree/main)


works with commands that are detached as demonstrated by cmd.exe start.
```
startt -f -g1x5 cmd /c "start \"parent\" cmd /k echo parent & start \"1\" cmd /k echo 1 & start \"2\" cmd /k echo 2 & start \"3\" cmd /k echo 3 & start \"4\" cmd /k echo 4"
```
maybe you prefer some other program.
```
# powershell
startt -f -g1x5 powershell -NoProfile -WindowStyle Normal -Command "1..5 | ForEach-Object { Start-Process powershell -ArgumentList '-NoProfile','-Command','$host.ui.RawUI.WindowTitle = \"Prompt $_ PID=\" + $PID; Write-Host Prompt $_ PID=$PID; Start-Sleep -Seconds 99999' }; Start-Sleep -Seconds 99999"
```

When grid mode is enabled, each window (parent or child) is moved to the next cell in the grid, wrapping around as needed. This works for both the initial window and any new windows found in follow mode.

See also:  
- A protocol‐handler for launching & controlling Chrome via CDP  
  https://crates.io/crates/debugchrome-cdp-rs
- "How to get HWND of window opened by ShellExecuteEx?," StackOverflow  
  <https://stackoverflow.com/questions/3269390/how-to-get-hwnd-of-window-opened-by-shellexecuteex-hprocess>  
- PowerBasic forum thread on finding a shelled window handle  
  <https://forum.powerBasic.com/forum/user-to-user-discussions/powerbasic-for-windows/13933-finding-the-handle-of-a-shelled-window>


**startt solves the problem of finding the hwnd and process id of a command or url that is launched by cmd.exe /c start, explorer <url>, start-process, or anything that calls shellexecuteex under the covers.**

still rough around the edges and not intended for any purpose but demonstration.  the lib interface is subject to change - SEMVER rules will be applied.

it shakes the 1st found window; moves windows in grids; kills all child processes on ctrl+c;

tested with chrome, vscode, mpv, msedge, cmd.  Your application, mileage, and use case may vary. If you find a problem; take a look at the code, PRs and polite discussion are welcome.

--dave horner  
5/25

MIT License

Copyright (c) 2025 David Horner

Permission is hereby granted, free of charge, to any person obtaining a copy  
of this software and associated documentation files (the “Software”), to deal  
in the Software without restriction, including without limitation the rights  
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell  
copies of the Software, and to permit persons to whom the Software is  
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in  
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR  
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,  
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE  
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER  
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,  
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN  
THE SOFTWARE.
