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
- **Filters** candidates by:
  - **PID match** (parent or any child PID)  
  - **Executable name** contains the target command/document name  
- **Optionally follows and shakes child windows** with the `-f` or `--follow` flag:
  - When `-f` is specified, `startt` will continue to monitor for new child processes/windows spawned by the launched process, and shake each new window once as it appears.  
  - This is useful for apps that spawn additional windows after startup (e.g., browsers, editors, etc.).

Because it uses only Win32 APIs (`OpenProcess`, `GetProcessImageFileNameW`, `GetProcessTimes`, `EnumWindows`, etc.), it works for _any_ “start”-style invocation—making it perfect for scripts, CI jobs, or demo tooling where you need to programmatically find and manipulate the window/app you just opened.

**Usage:**
```
startt [-f|--follow] <executable|document|URL> [args...]
```
- Use `-f` or `--follow` to keep watching for and shaking new child windows.

See also:  
- A protocol‐handler for launching & controlling Chrome via CDP  
  https://crates.io/crates/debugchrome-cdp-rs
- "How to get HWND of window opened by ShellExecuteEx?," StackOverflow  
  <https://stackoverflow.com/questions/3269390/how-to-get-hwnd-of-window-opened-by-shellexecuteex-hprocess>  
- PowerBasic forum thread on finding a shelled window handle  
  <https://forum.powerBasic.com/forum/user-to-user-discussions/powerbasic-for-windows/13933-finding-the-handle-of-a-shelled-window>


**startt solves the problem of finding the hwnd and process id of a command or url that is launched by cmd.exe /c start, explorer <url>, start-process, or anything that calls shellexecuteex under the covers.**

its rough around the edges and not intended for any purpose but demonstration.
it shakes the 1st found window;  tested with chrome, vscode, mpv, msedge, cmd.

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
