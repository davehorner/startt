feat(startt): hello 'startt' default open tool

`startt` solves a long-standing Windows poor design/quirk: when you do

  • `cmd.exe /c start …`  
  • Explorer “Open with” or protocol handler  
  • PowerShell `Start-Process`  

or anything that calls `ShellExecuteEx` under the covers (even with `SEE_MASK_NOCLOSEPROCESS` + `WaitForInputIdle`), the PID you get back is unusable for apps like Chrome.  You can’t reliably find its window handle (HWND) or true process ID for automation, testing, or demos.

**Usage:**
```
startt [options] <executable|document|URL> [args...]
```

**Grid and cell assignment options:**
- `-g ROWSxCOLS[ mMONITOR]` or `--grid ROWSxCOLS[ mMONITOR]`  
  Tile each window into a grid on the specified monitor (e.g., `-g 2x2m1` for a 2x2 grid on monitor 1, zero-based).
- `-fg` or `--fit-grid`  
  Resize each window to exactly fit its grid cell before positioning, instead of centering at original size.
- `-apc ROWxCOL[ mMONITOR]` or `--assign-parent-cell ROWxCOL[ mMONITOR]`  
  Assign the parent window to a specific grid cell and monitor (e.g., `-apc 1x1m1`). If monitor is omitted, uses the grid's monitor.
- `-rpc` or `--reserve-parent-cell`  
  Prevents any child window from being assigned to the same grid cell as the parent window (whether default or set by `--assign-parent-cell`).

**Taskbar options (experimental):**
- `-htb` or `--hide-taskbar`  
  Attempt to hide the Windows taskbar on the grid's monitor.
- `-stb` or `--show-taskbar`  
  Attempt to show the Windows taskbar on the grid's monitor.

**Other options:**
- `-f` or `--follow`  
  Keep watching for and shaking new child windows.
- `-F` or `--follow-forever`  
  Keep watching for and shaking new child windows even after the parent has closed.
- `-t SECONDS` or `--timeout SECONDS`  
  Specify the number of seconds each window should remain open before a quit message is sent to it.
- `-hT` or `--hide-title-bar`  
  Hide the title bar of the target window.
- `-hB` or `--hide-border`  
  Hide the border of the target window.
- `-T` or `--flash-topmost`  
  Briefly set the window as topmost, then restore it.
- `-sd MILLISECONDS` or `--shake-duration MILLISECONDS`  
  Set the shake animation duration in milliseconds (default: 2000ms).

**Examples:**

```
startt -f -g2x2m1 -fg -apc 1x1m1 -rpc myapp.exe
```
This will assign the parent window to cell (1,1) on monitor 1, ensure no child window is placed in that cell, and resize all windows to fit their grid cells.

```
startt -f -g1x4 -fg -t 10 -hT -T -sd 1500 cargo-e --run-all --run-at-a-time 4
```
This will shake and grid each window, resize them to fit their grid cells, hide the title bar, briefly flash the window as topmost, and use a 1.5 second shake duration. After 10 seconds, a quit message is sent to each window.
[![startt + cargo-e + bevy](https://github.com/davehorner/cargo-e_walkthrus/raw/main/startt_cargo-e_bevy_runall_4x1.gif)](https://github.com/davehorner/cargo-e_walkthrus/tree/main)

Works with commands that are detached as demonstrated by cmd.exe start:
```
startt -f -g1x5 cmd /c "start \"parent\" cmd /k echo parent & start \"1\" cmd /k echo 1 & start \"2\" cmd /k echo 2 & start \"3\" cmd /k echo 3 & start \"4\" cmd /k echo 4"
```
Or use with PowerShell:
```
startt -f -g1x5 powershell -NoProfile -WindowStyle Normal -Command "1..5 | ForEach-Object { Start-Process powershell -ArgumentList '-NoProfile','-Command','$host.ui.RawUI.WindowTitle = \"Prompt $_ PID=\" + $PID; Write-Host Prompt $_ PID=$PID; Start-Sleep -Seconds 99999' }; Start-Sleep -Seconds 99999"
```

When grid mode is enabled, each window (parent or child) is moved to the next cell in the grid, wrapping around as needed. With `--fit-grid`, each window is also resized to fill its cell. This works for both the initial window and any new windows found in follow mode.

See also:  
- A protocol‐handler for launching & controlling Chrome via CDP  
  https://crates.io/crates/debugchrome-cdp-rs
- "How to get HWND of window opened by ShellExecuteEx?," StackOverflow  
  <https://stackoverflow.com/questions/3269390/how-to-get-hwnd-of-window-opened-by-shellexecuteex-hprocess>  
- PowerBasic forum thread on finding a shelled window handle  
  <https://forum.powerBasic.com/forum/user-to-user-discussions/powerbasic-for-windows/13933-finding-the-handle-of-a-shelled-window>

**startt solves the problem of finding the hwnd and process id of a command or url that is launched by cmd.exe /c start, explorer <url>, start-process, or anything that calls shellexecuteex under the covers.**

Still rough around the edges and not intended for any purpose but demonstration. The lib interface is subject to change - SEMVER rules will be applied.

It shakes the 1st found window; moves windows in grids; kills all child processes on ctrl+c;

Tested with chrome, vscode, mpv, msedge, cmd. Your application, mileage, and use case may vary. If you find a problem; take a look at the code, PRs and polite discussion are welcome.

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
