//! Service management commands (systemd / launchd / Windows SCM).

// `Context` is only used inside the Linux/macOS `cfg` blocks, so on Windows it would
// otherwise be flagged as unused. Allow the unused-import warning to keep the
// cross-platform build clean.
#[allow(unused_imports)]
use anyhow::{Context, Result, bail};

/// Register `nexterm-server` as an auto-start service.
pub(crate) fn cmd_service_install() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_install_systemd();

    #[cfg(target_os = "macos")]
    return service_install_launchd();

    #[cfg(windows)]
    return service_install_windows();

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("service registration is not supported on this platform")
}

/// Unregister the service.
pub(crate) fn cmd_service_uninstall() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_uninstall_systemd();

    #[cfg(target_os = "macos")]
    return service_uninstall_launchd();

    #[cfg(windows)]
    return service_uninstall_windows();

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("service registration is not supported on this platform")
}

/// Show the registration status of the service.
pub(crate) fn cmd_service_status() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let unit_path = systemd_unit_path()?;
        if unit_path.exists() {
            println!("service: installed ({})", unit_path.display());
            // Check `systemctl is-active`.
            let status = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "nexterm-server.service"])
                .output();
            match status {
                Ok(o) => {
                    let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    println!(
                        "state: {}",
                        if state == "active" { "running" } else { &state }
                    );
                }
                Err(_) => println!("state: unknown (systemctl not found)"),
            }
        } else {
            println!("service: not installed");
            println!("to install: nexterm-ctl service install");
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let plist_path = launchd_plist_path()?;
        if plist_path.exists() {
            println!("service: installed ({})", plist_path.display());
            let status = std::process::Command::new("launchctl")
                .args(["list", "io.github.nexterm.server"])
                .output();
            match status {
                Ok(o) if o.status.success() => println!("state: running"),
                _ => println!("state: stopped"),
            }
        } else {
            println!("service: not installed");
            println!("to install: nexterm-ctl service install");
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        println!("Windows service management is not yet implemented.");
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("service management is not supported on this platform")
}

// ---- Linux (systemd) ----

#[cfg(target_os = "linux")]
fn systemd_unit_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(std::path::PathBuf::from(format!(
        "{}/.config/systemd/user/nexterm-server.service",
        home
    )))
}

#[cfg(target_os = "linux")]
fn nexterm_server_bin() -> Result<String> {
    // Look for `nexterm-server` in the same directory as ourselves (`nexterm-ctl`).
    let exe = std::env::current_exe().context("failed to obtain the executable path")?;
    let dir = exe
        .parent()
        .context("failed to obtain the executable's directory")?;
    let server = dir.join("nexterm-server");
    if server.exists() {
        Ok(server.to_string_lossy().to_string())
    } else {
        // Fall back to looking in $PATH.
        Ok("nexterm-server".to_string())
    }
}

#[cfg(target_os = "linux")]
fn service_install_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;
    let server_bin = nexterm_server_bin()?;

    // Create the unit directory.
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let unit = format!(
        r#"[Unit]
Description=Nexterm Terminal Server
After=network.target

[Service]
Type=simple
ExecStart={server_bin}
Restart=on-failure
RestartSec=5s
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
"#,
        server_bin = server_bin
    );

    std::fs::write(&unit_path, &unit)
        .with_context(|| format!("failed to write unit file: {}", unit_path.display()))?;

    // `systemctl --user daemon-reload && enable && start`.
    let cmds: &[&[&str]] = &[
        &["systemctl", "--user", "daemon-reload"],
        &["systemctl", "--user", "enable", "nexterm-server.service"],
        &["systemctl", "--user", "start", "nexterm-server.service"],
    ];
    for cmd in cmds {
        let status = std::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .status()
            .with_context(|| format!("failed to run command: {}", cmd[0]))?;
        if !status.success() {
            bail!("command failed: {}", cmd.join(" "));
        }
    }

    println!("registered nexterm-server as a systemd user service");
    println!("unit file: {}", unit_path.display());
    println!("service started. follow the log with: journalctl --user -u nexterm-server -f");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_uninstall_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;

    // Stop → disable → remove file.
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "nexterm-server.service"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "nexterm-server.service"])
        .status();

    if unit_path.exists() {
        std::fs::remove_file(&unit_path)
            .with_context(|| format!("failed to remove unit file: {}", unit_path.display()))?;
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    println!("removed the nexterm-server service");
    Ok(())
}

// ---- macOS (launchd) ----

#[cfg(target_os = "macos")]
fn launchd_plist_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(std::path::PathBuf::from(format!(
        "{}/Library/LaunchAgents/io.github.nexterm.server.plist",
        home
    )))
}

#[cfg(target_os = "macos")]
fn nexterm_server_bin() -> Result<String> {
    let exe = std::env::current_exe().context("failed to obtain the executable path")?;
    let dir = exe
        .parent()
        .context("failed to obtain the executable's directory")?;
    let server = dir.join("nexterm-server");
    if server.exists() {
        Ok(server.to_string_lossy().to_string())
    } else {
        Ok("nexterm-server".to_string())
    }
}

#[cfg(target_os = "macos")]
fn service_install_launchd() -> Result<()> {
    let plist_path = launchd_plist_path()?;
    let server_bin = nexterm_server_bin()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>io.github.nexterm.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>{server_bin}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{home}/Library/Logs/nexterm-server.log</string>
    <key>StandardErrorPath</key>
    <string>{home}/Library/Logs/nexterm-server.log</string>
</dict>
</plist>
"#,
        server_bin = server_bin,
        home = home
    );

    std::fs::write(&plist_path, &plist)
        .with_context(|| format!("failed to write plist file: {}", plist_path.display()))?;

    let status = std::process::Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .status()
        .context("`launchctl load` failed")?;

    if !status.success() {
        bail!("`launchctl load` returned a non-zero status");
    }

    println!("registered nexterm-server with launchd");
    println!("plist: {}", plist_path.display());
    println!("log: {}/Library/Logs/nexterm-server.log", home);
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall_launchd() -> Result<()> {
    let plist_path = launchd_plist_path()?;

    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .status();
        std::fs::remove_file(&plist_path)
            .with_context(|| format!("failed to remove plist file: {}", plist_path.display()))?;
        println!("removed the nexterm-server service");
    } else {
        println!("service is not installed");
    }
    Ok(())
}

// ---- Windows (SCM) ----

#[cfg(windows)]
fn service_install_windows() -> Result<()> {
    bail!(
        "Windows service registration is not yet implemented.\nUse Task Scheduler for auto-start as a workaround."
    )
}

#[cfg(windows)]
fn service_uninstall_windows() -> Result<()> {
    bail!("Windows service removal is not yet implemented.")
}
