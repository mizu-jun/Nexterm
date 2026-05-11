//! サービス管理コマンド (systemd / launchd / Windows SCM)。

// `Context` は Linux/macOS の cfg ブロック内でのみ使用するため、Windows
// ビルドでは未使用となる。プラットフォーム横断のビルドエラーを避けるため
// `unused_imports` 警告を許容する。
#[allow(unused_imports)]
use anyhow::{Context, Result, bail};

/// nexterm-server を自動起動サービスとして登録する
pub(crate) fn cmd_service_install() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_install_systemd();

    #[cfg(target_os = "macos")]
    return service_install_launchd();

    #[cfg(windows)]
    return service_install_windows();

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("このプラットフォームはサービス登録に非対応です")
}

/// サービス登録を解除する
pub(crate) fn cmd_service_uninstall() -> Result<()> {
    #[cfg(target_os = "linux")]
    return service_uninstall_systemd();

    #[cfg(target_os = "macos")]
    return service_uninstall_launchd();

    #[cfg(windows)]
    return service_uninstall_windows();

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("このプラットフォームはサービス登録に非対応です")
}

/// サービス登録状態を表示する
pub(crate) fn cmd_service_status() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let unit_path = systemd_unit_path()?;
        if unit_path.exists() {
            println!("サービス: インストール済み ({})", unit_path.display());
            // systemctl is-active の結果を確認する
            let status = std::process::Command::new("systemctl")
                .args(["--user", "is-active", "nexterm-server.service"])
                .output();
            match status {
                Ok(o) => {
                    let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    println!(
                        "状態: {}",
                        if state == "active" {
                            "実行中"
                        } else {
                            &state
                        }
                    );
                }
                Err(_) => println!("状態: 不明（systemctl が見つかりません）"),
            }
        } else {
            println!("サービス: 未インストール");
            println!("インストールするには: nexterm-ctl service install");
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let plist_path = launchd_plist_path()?;
        if plist_path.exists() {
            println!("サービス: インストール済み ({})", plist_path.display());
            let status = std::process::Command::new("launchctl")
                .args(["list", "io.github.nexterm.server"])
                .output();
            match status {
                Ok(o) if o.status.success() => println!("状態: 実行中"),
                _ => println!("状態: 停止中"),
            }
        } else {
            println!("サービス: 未インストール");
            println!("インストールするには: nexterm-ctl service install");
        }
        Ok(())
    }

    #[cfg(windows)]
    {
        println!("Windows サービス管理は準備中です。");
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    bail!("このプラットフォームはサービス管理に非対応です")
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
    // 自分自身（nexterm-ctl）と同じディレクトリにある nexterm-server を探す
    let exe = std::env::current_exe().context("実行ファイルパスの取得に失敗しました")?;
    let dir = exe
        .parent()
        .context("実行ファイルのディレクトリ取得に失敗しました")?;
    let server = dir.join("nexterm-server");
    if server.exists() {
        Ok(server.to_string_lossy().to_string())
    } else {
        // PATH から探す
        Ok("nexterm-server".to_string())
    }
}

#[cfg(target_os = "linux")]
fn service_install_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;
    let server_bin = nexterm_server_bin()?;

    // ユニットディレクトリを作成する
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("ディレクトリの作成に失敗しました: {}", parent.display()))?;
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

    std::fs::write(&unit_path, &unit).with_context(|| {
        format!(
            "ユニットファイルの書き込みに失敗しました: {}",
            unit_path.display()
        )
    })?;

    // systemctl --user daemon-reload && enable && start
    let cmds: &[&[&str]] = &[
        &["systemctl", "--user", "daemon-reload"],
        &["systemctl", "--user", "enable", "nexterm-server.service"],
        &["systemctl", "--user", "start", "nexterm-server.service"],
    ];
    for cmd in cmds {
        let status = std::process::Command::new(cmd[0])
            .args(&cmd[1..])
            .status()
            .with_context(|| format!("コマンド実行に失敗しました: {}", cmd[0]))?;
        if !status.success() {
            bail!("コマンドが失敗しました: {}", cmd.join(" "));
        }
    }

    println!("nexterm-server を systemd ユーザーサービスとして登録しました");
    println!("ユニットファイル: {}", unit_path.display());
    println!("サービスを開始しました。ログ: journalctl --user -u nexterm-server -f");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_uninstall_systemd() -> Result<()> {
    let unit_path = systemd_unit_path()?;

    // 停止 → 無効化 → ファイル削除
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "nexterm-server.service"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "nexterm-server.service"])
        .status();

    if unit_path.exists() {
        std::fs::remove_file(&unit_path).with_context(|| {
            format!(
                "ユニットファイルの削除に失敗しました: {}",
                unit_path.display()
            )
        })?;
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    println!("nexterm-server サービスを削除しました");
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
    let exe = std::env::current_exe().context("実行ファイルパスの取得に失敗しました")?;
    let dir = exe
        .parent()
        .context("実行ファイルのディレクトリ取得に失敗しました")?;
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
            .with_context(|| format!("ディレクトリの作成に失敗しました: {}", parent.display()))?;
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

    std::fs::write(&plist_path, &plist).with_context(|| {
        format!(
            "plist ファイルの書き込みに失敗しました: {}",
            plist_path.display()
        )
    })?;

    let status = std::process::Command::new("launchctl")
        .args(["load", "-w", &plist_path.to_string_lossy()])
        .status()
        .context("launchctl load に失敗しました")?;

    if !status.success() {
        bail!("launchctl load が失敗しました");
    }

    println!("nexterm-server を launchd に登録しました");
    println!("plist: {}", plist_path.display());
    println!("ログ: {}/Library/Logs/nexterm-server.log", home);
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall_launchd() -> Result<()> {
    let plist_path = launchd_plist_path()?;

    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w", &plist_path.to_string_lossy()])
            .status();
        std::fs::remove_file(&plist_path).with_context(|| {
            format!(
                "plist ファイルの削除に失敗しました: {}",
                plist_path.display()
            )
        })?;
        println!("nexterm-server サービスを削除しました");
    } else {
        println!("サービスはインストールされていません");
    }
    Ok(())
}

// ---- Windows (SCM) ----

#[cfg(windows)]
fn service_install_windows() -> Result<()> {
    bail!(
        "Windows サービス登録は現在準備中です。\nタスクスケジューラでの自動起動を代替として使用してください。"
    )
}

#[cfg(windows)]
fn service_uninstall_windows() -> Result<()> {
    bail!("Windows サービス削除は現在準備中です。")
}
