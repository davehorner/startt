pub fn get_cmdline_for_pid(parent_pid: u32) -> String {
    use std::process::Command;

    // Use PowerShell with NtObjectManager to get the command line
    // Prompt for -Force if not installed, otherwise skip Install-Module
    let ps_script = format!(
        r#"
            if (-not (Get-Module -ListAvailable -Name NtObjectManager)) {{
            Write-Host "NtObjectManager not found. Installing..." -ForegroundColor Yellow
            if (-not (Get-PackageProvider -Name NuGet -ErrorAction SilentlyContinue)) {{
            Write-Host "NuGet provider not found. Installing..." -ForegroundColor Yellow
            Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser -ErrorAction Stop
            }}
            Install-Module -Name NtObjectManager -Scope CurrentUser -Force -ErrorAction Stop -Confirm:$false
            }}
            Import-Module NtObjectManager -ErrorAction Stop
            try {{ (Get-NtProcess -ProcessId {}).CommandLine }} catch {{ '' }}
            "#,
        parent_pid
    );
    // println!("[PowerShell] Executing script: {}", ps_script);
    let output = Command::new("powershell")
        .args(&["-NoProfile", "-Command", &ps_script])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let cmdline = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !cmdline.is_empty() {
                cmdline
            } else {
                "<no cmdline found>".to_string()
            }
        }
        _ => "<failed to get cmdline>".to_string(),
    }
}

pub fn process_has_env_var(
    parent_pid: u32,
    env_name: &str,
    env_value: Option<&str>,
) -> Option<bool> {
    use std::process::Command;

    // PowerShell script to get the environment variables for the process
    let ps_script = format!(
        r#"
                if (-not (Get-Module -ListAvailable -Name NtObjectManager)) {{
                    Write-Host "NtObjectManager not found. Installing..." -ForegroundColor Yellow
                    if (-not (Get-PackageProvider -Name NuGet -ErrorAction SilentlyContinue)) {{
                        Write-Host "NuGet provider not found. Installing..." -ForegroundColor Yellow
                        Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser -ErrorAction Stop
                    }}
                    Install-Module -Name NtObjectManager -Scope CurrentUser -Force -ErrorAction Stop -Confirm:$false
                }}
                Import-Module NtObjectManager -ErrorAction Stop
                try {{
                    $envs = Get-NtProcessEnvironment -ProcessId {pid}
                    if ($envs -eq $null) {{ exit 2 }}
                    $found = $false
                    foreach ($env in $envs.GetEnumerator()) {{
                        if ($env.Name -eq '{name}') {{
                            {value_check}
                        }}
                    }}
                    if ($found) {{ exit 0 }} else {{ exit 1 }}
                }} catch {{ exit 3 }}
                "#,
        pid = parent_pid,
        name = env_name,
        value_check = match env_value {
            Some(val) => format!("if ($env.Value -eq '{}') {{ $found = $true }}", val),
            None => "$found = $true".to_string(),
        }
    );
    //println!("[PowerShell] Executing script: {}", ps_script);
    let output = Command::new("powershell")
        .args(&["-NoProfile", "-Command", &ps_script])
        .output();

    match output {
        Ok(out) => match out.status.code() {
            Some(0) => Some(true),            // Found
            Some(1) => Some(false),           // Not found
            Some(2) | Some(3) | None => None, // Error or no env
            _ => None,
        },
        Err(_) => None,
    }
}

pub fn process_print_env(parent_pid: u32) -> Option<String> {
    use std::process::Command;

    // PowerShell script to get all environment variables for the process
    let ps_script = format!(
        r#"
                    if (-not (Get-Module -ListAvailable -Name NtObjectManager)) {{
                         Write-Host "NtObjectManager not found. Installing..." -ForegroundColor Yellow
                         if (-not (Get-PackageProvider -Name NuGet -ErrorAction SilentlyContinue)) {{
                              Write-Host "NuGet provider not found. Installing..." -ForegroundColor Yellow
                              Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser -ErrorAction Stop
                         }}
                         Install-Module -Name NtObjectManager -Scope CurrentUser -Force -ErrorAction Stop -Confirm:$false
                    }}
                    Import-Module NtObjectManager -ErrorAction Stop
                    try {{
                         $envs = Get-NtProcessEnvironment -ProcessId {pid}
                         if ($envs -eq $null) {{ exit 2 }}
                         $envs.GetEnumerator() | ForEach-Object {{ "$($_.Name)=$($_.Value)" }}
                    }} catch {{ exit 3 }}
                    "#,
        pid = parent_pid
    );
    //println!("[PowerShell] Executing script: {}", ps_script);
    let output = Command::new("powershell")
        .args(&["-NoProfile", "-Command", &ps_script])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let envs = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !envs.is_empty() { Some(envs) } else { None }
        }
        _ => None,
    }
}

pub fn get_env_child_pids(
    env_name: &str,
    env_value: Option<&str>,
    pid_map: &mut std::collections::HashSet<u32>,
) -> Option<bool> {
    use std::process::Command;

    // PowerShell script to enumerate all processes and check their environment variables
    let ps_script = format!(
        r#"
        if (-not (Get-Module -ListAvailable -Name NtObjectManager)) {{
            Write-Host "NtObjectManager not found. Installing..." -ForegroundColor Yellow
            if (-not (Get-PackageProvider -Name NuGet -ErrorAction SilentlyContinue)) {{
                Write-Host "NuGet provider not found. Installing..." -ForegroundColor Yellow
                Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Force -Scope CurrentUser -ErrorAction Stop
            }}
            Install-Module -Name NtObjectManager -Scope CurrentUser -Force -ErrorAction Stop -Confirm:$false
        }}
        Import-Module NtObjectManager -ErrorAction Stop
        $found = $false
        try {{
            $procs = Get-NtProcess
            foreach ($proc in $procs) {{
                try {{
                    $envs = Get-NtProcessEnvironment -ProcessId $proc.ProcessId
                    if ($envs -ne $null) {{
                        foreach ($env in $envs.GetEnumerator()) {{
                            if ($env.Name -eq '{name}') {{
                                {value_check}
                            }}
                        }}
                    }}
                }} catch {{}}
            }}
            if ($found) {{ exit 0 }} else {{ exit 1 }}
        }} catch {{ exit 2 }}
        "#,
        name = env_name,
        value_check = match env_value {
            Some(val) => format!(
                "if ($env.Value -eq '{}') {{ Write-Output $proc.ProcessId; $found = $true }}",
                val
            ),
            None => "Write-Output $proc.ProcessId; $found = $true".to_string(),
        }
    );

    let output = Command::new("powershell")
        .args(&["-NoProfile", "-Command", &ps_script])
        .output();

    match output {
        Ok(out) => match out.status.code() {
            Some(0) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if let Ok(pid) = line.trim().parse::<u32>() {
                        pid_map.insert(pid);
                    }
                }
                Some(true)
            }
            Some(1) => Some(false), // Not found
            _ => None,              // Error
        },
        Err(_) => None,
    }
}
