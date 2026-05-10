//! nexterm-ctl — nexterm session management CLI
//!
//! # Usage
//!
//! ```text
//! nexterm-ctl list                          # List all sessions
//! nexterm-ctl new <name>                    # Create a new session
//! nexterm-ctl attach <name>                 # Show how to attach to a session
//! nexterm-ctl kill <name>                   # Kill a session
//! nexterm-ctl record start <session> <file> # Start recording PTY output
//! nexterm-ctl record stop <session>         # Stop recording
//! nexterm-ctl theme import <path>           # Import a color theme from file
//! nexterm-ctl template save <name>          # Save current session layout as template
//! nexterm-ctl template load <name>          # Load and apply a saved template
//! nexterm-ctl template list                 # List all saved templates
//! ```

use anyhow::{Context, Result, bail};
use clap::{Arg, Command};
use clap_complete::{Shell, generate};
use clap_mangen::Man;
use nexterm_i18n::fl;
use nexterm_proto::{ClientToServer, ServerToClient};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing_subscriber::EnvFilter;

// ---- CLI 定義（ビルダー形式でロケール対応） ----

fn build_cli() -> Command {
    Command::new("nexterm-ctl")
        .about(fl!("ctl-about"))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about(fl!("ctl-list-about")))
        .subcommand(
            Command::new("new")
                .about(fl!("ctl-new-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("attach")
                .about(fl!("ctl-attach-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("kill")
                .about(fl!("ctl-kill-about"))
                .arg(Arg::new("name").help(fl!("ctl-arg-name")).required(true)),
        )
        .subcommand(
            Command::new("record")
                .about(fl!("ctl-record-about"))
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("start")
                        .about(fl!("ctl-record-start-about"))
                        .arg(Arg::new("session").help(fl!("ctl-arg-name")).required(true))
                        .arg(
                            Arg::new("file")
                                .help(fl!("ctl-record-arg-file"))
                                .required(true),
                        ),
                )
                .subcommand(
                    Command::new("stop")
                        .about(fl!("ctl-record-stop-about"))
                        .arg(Arg::new("session").help(fl!("ctl-arg-name")).required(true)),
                ),
        )
        .subcommand(
            Command::new("theme")
                .about("Theme management")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("import")
                        .about("Import a color theme from a file (.itermcolors, .yaml/.yml, .toml)")
                        .arg(
                            Arg::new("path")
                                .help("Path to the theme file")
                                .required(true),
                        ),
                ),
        )
        .subcommand(
            Command::new("man")
                .about("Generate man page (outputs to stdout, redirect to nexterm-ctl.1)"),
        )
        .subcommand(
            Command::new("completions")
                .about("Generate shell completion scripts")
                .arg(
                    Arg::new("shell")
                        .help("Shell type: bash, zsh, fish, powershell, elvish")
                        .required(true)
                        .value_parser(["bash", "zsh", "fish", "powershell", "elvish"]),
                ),
        )
        .subcommand(
            Command::new("service")
                .about("Manage nexterm server as a system service (systemd/launchd/SCM)")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("install")
                        .about("Install nexterm server as an auto-start service"),
                )
                .subcommand(Command::new("uninstall").about("Uninstall the nexterm server service"))
                .subcommand(Command::new("status").about("Show service installation status")),
        )
        .subcommand(
            Command::new("import-ghostty")
                .about("Import Ghostty terminal configuration and convert to nexterm format")
                .arg(
                    Arg::new("path")
                        .help("Path to Ghostty config file (default: ~/.config/ghostty/config)")
                        .required(false),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .short('o')
                        .help("Output path (default: ~/.config/nexterm/config.toml)")
                        .required(false),
                ),
        )
        .subcommand(
            Command::new("template")
                .about("Layout template management")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("save")
                        .about("Save current session layout as a template")
                        .arg(Arg::new("name").help("Template name").required(true))
                        .arg(Arg::new("session").help("Session name").required(true)),
                )
                .subcommand(
                    Command::new("load")
                        .about("Load and apply a saved template")
                        .arg(Arg::new("name").help("Template name").required(true))
                        .arg(Arg::new("session").help("Session name").required(true)),
                )
                .subcommand(Command::new("list").about("List all saved templates")),
        )
        .subcommand(
            Command::new("plugin")
                .about("WASM plugin management")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(Command::new("list").about("List loaded plugins"))
                .subcommand(
                    Command::new("load")
                        .about("Load a WASM plugin")
                        .arg(Arg::new("path").help("Path to .wasm file").required(true)),
                )
                .subcommand(
                    Command::new("unload")
                        .about("Unload a loaded plugin")
                        .arg(Arg::new("path").help("Path to .wasm file").required(true)),
                )
                .subcommand(
                    Command::new("reload")
                        .about("Reload a plugin (unload + load)")
                        .arg(Arg::new("path").help("Path to .wasm file").required(true)),
                ),
        )
}

// ---- エントリーポイント ----

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

    // ロケールを検出してから CLI を構築する
    nexterm_i18n::init();

    let matches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("list", _)) => cmd_list().await,
        Some(("new", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .expect("clap required arg")
                .clone();
            cmd_new(name).await
        }
        Some(("attach", sub)) => {
            let name = sub.get_one::<String>("name").expect("clap required arg");
            cmd_attach(name)
        }
        Some(("kill", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .expect("clap required arg")
                .clone();
            cmd_kill(name).await
        }
        Some(("record", sub)) => match sub.subcommand() {
            Some(("start", s)) => {
                let session = s
                    .get_one::<String>("session")
                    .expect("clap required arg")
                    .clone();
                let file = s
                    .get_one::<String>("file")
                    .expect("clap required arg")
                    .clone();
                cmd_record_start(session, file).await
            }
            Some(("stop", s)) => {
                let session = s
                    .get_one::<String>("session")
                    .expect("clap required arg")
                    .clone();
                cmd_record_stop(session).await
            }
            _ => unreachable!(),
        },
        Some(("service", sub)) => match sub.subcommand() {
            Some(("install", _)) => cmd_service_install(),
            Some(("uninstall", _)) => cmd_service_uninstall(),
            Some(("status", _)) => cmd_service_status(),
            _ => unreachable!(),
        },
        Some(("import-ghostty", sub)) => {
            let path = sub.get_one::<String>("path").cloned();
            let output = sub.get_one::<String>("output").cloned();
            cmd_import_ghostty(path, output)
        }
        Some(("theme", sub)) => match sub.subcommand() {
            Some(("import", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd_theme_import(path)
            }
            _ => unreachable!(),
        },
        Some(("man", _)) => {
            let man = Man::new(build_cli());
            man.render(&mut std::io::stdout())?;
            Ok(())
        }
        Some(("completions", sub)) => {
            let shell_str = sub.get_one::<String>("shell").expect("clap required arg");
            let shell: Shell = shell_str.parse().expect("valid shell");
            generate(
                shell,
                &mut build_cli(),
                "nexterm-ctl",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Some(("template", sub)) => match sub.subcommand() {
            Some(("save", s)) => {
                let name = s
                    .get_one::<String>("name")
                    .expect("clap required arg")
                    .clone();
                let session = s
                    .get_one::<String>("session")
                    .expect("clap required arg")
                    .clone();
                cmd_template_save(name, session).await
            }
            Some(("load", s)) => {
                let name = s
                    .get_one::<String>("name")
                    .expect("clap required arg")
                    .clone();
                let session = s
                    .get_one::<String>("session")
                    .expect("clap required arg")
                    .clone();
                cmd_template_load(name, session).await
            }
            Some(("list", _)) => cmd_template_list().await,
            _ => unreachable!(),
        },
        Some(("plugin", sub)) => match sub.subcommand() {
            Some(("list", _)) => cmd_plugin_list().await,
            Some(("load", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd_plugin_load(path).await
            }
            Some(("unload", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd_plugin_unload(path).await
            }
            Some(("reload", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd_plugin_reload(path).await
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

// ---- サブコマンド実装 ----

/// セッション一覧を取得して表示する
async fn cmd_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListSessions).await?;

    match conn.recv().await? {
        ServerToClient::SessionList { sessions } => {
            if sessions.is_empty() {
                println!("{}", fl!("ctl-no-sessions"));
            } else {
                println!(
                    "{:<20} {:<12} {}",
                    fl!("ctl-list-col-name"),
                    fl!("ctl-list-col-windows"),
                    fl!("ctl-list-col-status")
                );
                println!("{}", "-".repeat(48));
                for s in &sessions {
                    let status = if s.attached {
                        fl!("ctl-status-attached")
                    } else {
                        fl!("ctl-status-detached")
                    };
                    println!("{:<20} {:<12} {}", s.name, s.window_count, status);
                }
            }
        }
        ServerToClient::Error { message } => {
            bail!("{}", fl!("ctl-server-error", message = message))
        }
        _ => {}
    }

    Ok(())
}

/// 新規セッションを作成してすぐデタッチする
async fn cmd_new(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::Attach {
        session_name: name.clone(),
    })
    .await?;

    // SessionList を受け取るまで最大 8 メッセージ読み飛ばす
    let mut created = false;
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { sessions } => {
                created = sessions.iter().any(|s| s.name == name);
                break;
            }
            ServerToClient::Error { message } => {
                bail!("{}", fl!("ctl-error", message = message))
            }
            _ => {}
        }
    }
    conn.send(ClientToServer::Detach).await?;

    if created {
        println!("{}", fl!("ctl-session-created", name = name));
        println!("{}", fl!("ctl-session-created-hint"));
    } else {
        bail!("{}", fl!("ctl-session-create-failed", name = name));
    }

    Ok(())
}

/// セッションへのアタッチ方法を案内する（ctl 自体はインタラクティブ端末ではない）
fn cmd_attach(name: &str) -> Result<()> {
    println!("{}", fl!("ctl-attach-guide", name = name));
    println!("{}", fl!("ctl-attach-tui", name = name));
    println!("{}", fl!("ctl-attach-gpu", name = name));
    Ok(())
}

/// セッションを強制終了する
async fn cmd_kill(name: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::KillSession { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::SessionList { .. } => {
            println!("{}", fl!("ctl-session-killed", name = name));
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

/// セッションのフォーカスペインで録音を開始する
async fn cmd_record_start(session: String, file: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::StartRecording {
        session_name: session.clone(),
        output_path: file.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::RecordingStarted { pane_id, path } => {
            println!(
                "{}",
                fl!(
                    "ctl-record-started",
                    session = session,
                    pane_id = pane_id,
                    path = path
                )
            );
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

/// セッションのフォーカスペインの録音を停止する
async fn cmd_record_stop(session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::StopRecording {
        session_name: session.clone(),
    })
    .await?;
    match conn.recv().await? {
        ServerToClient::RecordingStopped { pane_id } => {
            println!(
                "{}",
                fl!("ctl-record-stopped", session = session, pane_id = pane_id)
            );
        }
        ServerToClient::Error { message } => bail!("{}", fl!("ctl-error", message = message)),
        _ => {}
    }
    Ok(())
}

// ---- テンプレート管理 ----

/// 現在のセッションレイアウトをテンプレートとして保存する
async fn cmd_template_save(name: String, session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    // セッションにアタッチしてから SaveTemplate を送信する
    conn.send(ClientToServer::Attach {
        session_name: session.clone(),
    })
    .await?;
    // Attach の応答（FullRefresh, LayoutChanged, SessionList）を読み飛ばす
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { .. } => break,
            ServerToClient::Error { message } => bail!("{}", message),
            _ => {}
        }
    }
    conn.send(ClientToServer::SaveTemplate { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::TemplateSaved {
            name: saved_name,
            path,
        } => {
            println!("テンプレート '{}' を保存しました: {}", saved_name, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// 保存済みテンプレートを読み込む
async fn cmd_template_load(name: String, session: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::Attach {
        session_name: session.clone(),
    })
    .await?;
    for _ in 0..8 {
        match conn.recv().await? {
            ServerToClient::SessionList { .. } => break,
            ServerToClient::Error { message } => bail!("{}", message),
            _ => {}
        }
    }
    conn.send(ClientToServer::LoadTemplate { name: name.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::TemplateLoaded { name: loaded_name } => {
            println!("テンプレート '{}' を読み込みました", loaded_name);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    conn.send(ClientToServer::Detach).await?;
    Ok(())
}

/// 保存済みテンプレート一覧を表示する
async fn cmd_template_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListTemplates).await?;
    match conn.recv().await? {
        ServerToClient::TemplateList { names } => {
            if names.is_empty() {
                println!("保存済みテンプレートはありません");
            } else {
                println!("保存済みテンプレート:");
                for name in &names {
                    println!("  {}", name);
                }
            }
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

// ---- サービス管理（systemd / launchd / SCM） ----

/// nexterm-server を自動起動サービスとして登録する
fn cmd_service_install() -> Result<()> {
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
fn cmd_service_uninstall() -> Result<()> {
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
fn cmd_service_status() -> Result<()> {
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
        return Ok(());
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

// ---- Ghostty 設定インポート ----

/// Ghostty 設定ファイルを読み込んで nexterm の config.toml に変換する
fn cmd_import_ghostty(path: Option<String>, output: Option<String>) -> Result<()> {
    // 入力パスのデフォルト: ~/.config/ghostty/config
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    let input_path = path.unwrap_or_else(|| format!("{}/.config/ghostty/config", home));

    if !Path::new(&input_path).exists() {
        bail!(
            "Ghostty 設定ファイルが見つかりません: {}\n\
             パスを明示的に指定してください: nexterm-ctl import-ghostty <path>",
            input_path
        );
    }

    let content = std::fs::read_to_string(&input_path).with_context(|| {
        format!(
            "Ghostty 設定ファイルの読み込みに失敗しました: {}",
            input_path
        )
    })?;

    let converted = parse_ghostty_config(&content)?;

    // 出力パスのデフォルト: ~/.config/nexterm/config.toml
    let output_path = output.unwrap_or_else(|| format!("{}/.config/nexterm/config.toml", home));

    // 出力ディレクトリを作成する
    if let Some(parent) = Path::new(&output_path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("ディレクトリの作成に失敗しました: {}", parent.display()))?;
    }

    // 既存の config.toml に Ghostty から変換した設定をマージする
    let existing = if Path::new(&output_path).exists() {
        std::fs::read_to_string(&output_path)
            .with_context(|| format!("既存設定ファイルの読み込みに失敗しました: {}", output_path))?
    } else {
        String::new()
    };

    let merged = merge_ghostty_config(&existing, &converted);

    std::fs::write(&output_path, &merged)
        .with_context(|| format!("設定ファイルの書き込みに失敗しました: {}", output_path))?;

    println!("Ghostty 設定をインポートしました");
    println!("  入力: {}", input_path);
    println!("  出力: {}", output_path);
    if !converted.notes.is_empty() {
        println!("\n変換メモ（手動確認が必要な項目）:");
        for note in &converted.notes {
            println!("  ⚠ {}", note);
        }
    }

    Ok(())
}

/// Ghostty 設定の変換結果
struct GhosttyConverted {
    /// [font] セクションの TOML フラグメント
    font_toml: Option<String>,
    /// [color-scheme.custom] セクションの TOML フラグメント（パレット設定時）
    palette_toml: Option<String>,
    /// [window] セクションの TOML フラグメント
    window_toml: Option<String>,
    /// 手動確認が必要な項目
    notes: Vec<String>,
}

/// Ghostty の設定ファイルをパースして nexterm 互換の設定に変換する
fn parse_ghostty_config(content: &str) -> Result<GhosttyConverted> {
    // Ghostty の設定フォーマット: `key = value` （TOML に近いが独自形式）
    let mut font_family: Option<String> = None;
    let mut font_size: Option<f32> = None;
    let mut background: Option<String> = None;
    let mut foreground: Option<String> = None;
    let mut cursor_color: Option<String> = None;
    let mut background_opacity: Option<f32> = None;
    let mut ansi: Vec<Option<String>> = vec![None; 16];
    let mut notes = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // コメント行とブランク行をスキップ
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // `key = value` を分割する
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };
        let key = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..]
            .trim()
            .trim_matches('"')
            .trim_matches('\'');

        match key {
            "font-family" => font_family = Some(value.to_string()),
            "font-size" => font_size = value.parse::<f32>().ok(),
            "background" => background = Some(normalize_color(value)),
            "foreground" => foreground = Some(normalize_color(value)),
            "cursor-color" => cursor_color = Some(normalize_color(value)),
            "background-opacity" => background_opacity = value.parse::<f32>().ok(),
            // ANSI パレット: palette = N=#RRGGBB 形式
            "palette" => {
                if let Some((idx_str, color)) = value.split_once('=')
                    && let Ok(idx) = idx_str.trim().parse::<usize>()
                    && idx < 16
                {
                    ansi[idx] = Some(normalize_color(color.trim()));
                }
            }
            // 未対応キーはメモに追記する
            "theme" => notes.push(format!(
                "theme = \"{}\" は手動で nexterm の color-scheme に変換してください",
                value
            )),
            "keybind" => notes.push(format!(
                "keybind = {} は nexterm の [keybindings] に手動でマッピングしてください",
                value
            )),
            "shell-integration" | "shell-integration-features" => {
                notes.push(format!("{} は nexterm では自動的に統合されます", key))
            }
            "window-decoration" => {
                // Ghostty の window-decoration → nexterm の window.decorations
                // "false" = none, "true"/"client"/"server" = full
            }
            _ => {
                // 重要そうなキーのみメモ（細かいものは無視）
                if !matches!(
                    key,
                    "cursor-style"
                        | "cursor-style-blink"
                        | "scrollback-limit"
                        | "clipboard-read"
                        | "clipboard-write"
                        | "mouse-hide-while-typing"
                ) && !key.starts_with("gtk-")
                    && !key.starts_with("macos-")
                    && !key.starts_with("linux-")
                    && !key.starts_with("windows-")
                {
                    // 未対応のキーは無視（警告しすぎるとユーザーが混乱する）
                }
            }
        }
    }

    // [font] セクションの生成
    let font_toml = if font_family.is_some() || font_size.is_some() {
        let mut s = String::from("[font]\n");
        if let Some(family) = &font_family {
            s.push_str(&format!("family = \"{}\"\n", family));
        }
        if let Some(size) = font_size {
            s.push_str(&format!("size = {}\n", size));
        }
        Some(s)
    } else {
        None
    };

    // [color-scheme.custom] セクションの生成
    let palette_toml = if background.is_some()
        || foreground.is_some()
        || ansi.iter().any(|a| a.is_some())
    {
        let bg = background.clone().unwrap_or_else(|| "#1d1f21".to_string());
        let fg = foreground.clone().unwrap_or_else(|| "#c5c8c6".to_string());
        let cur = cursor_color.clone().unwrap_or_else(|| fg.clone());
        let ansi_arr: Vec<String> = ansi
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.clone().unwrap_or_else(|| {
                    // デフォルト ANSI カラー
                    DEFAULT_ANSI_COLORS[i % 16].to_string()
                })
            })
            .collect();
        let ansi_str = ansi_arr
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "[color-scheme.custom]\nforeground = \"{}\"\nbackground = \"{}\"\ncursor = \"{}\"\nansi = [{}]\n",
            fg, bg, cur, ansi_str
        ))
    } else {
        None
    };

    // [window] セクションの生成
    let window_toml = background_opacity
        .map(|opacity| format!("[window]\nbackground_opacity = {:.2}\n", opacity));

    Ok(GhosttyConverted {
        font_toml,
        palette_toml,
        window_toml,
        notes,
    })
}

/// カラー文字列を正規化する（"RRGGBB" → "#RRGGBB"、既に "#" がある場合はそのまま）
fn normalize_color(s: &str) -> String {
    let s = s.trim_matches('"').trim_matches('\'');
    if s.starts_with('#') {
        s.to_uppercase()
    } else {
        format!("#{}", s.to_uppercase())
    }
}

/// デフォルト ANSI 16色（フォールバック用）
const DEFAULT_ANSI_COLORS: &[&str] = &[
    "#2E3440", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#88C0D0", "#E5E9F0",
    "#4C566A", "#BF616A", "#A3BE8C", "#EBCB8B", "#81A1C1", "#B48EAD", "#8FBCBB", "#ECEFF4",
];

/// 既存の config.toml に Ghostty から変換した設定をマージする
///
/// 各セクションが既に存在する場合は上書き、存在しない場合は末尾に追加する。
fn merge_ghostty_config(existing: &str, converted: &GhosttyConverted) -> String {
    let mut result = existing.to_string();

    if let Some(font) = &converted.font_toml {
        result = remove_toml_section(&result, "font");
        result = format!("{}\n{}", result.trim_end(), font);
    }

    if let Some(palette) = &converted.palette_toml {
        result = remove_toml_section(&result, "color-scheme.custom");
        result = format!("{}\n{}", result.trim_end(), palette);
    }

    if let Some(window) = &converted.window_toml {
        result = remove_toml_section(&result, "window");
        result = format!("{}\n{}", result.trim_end(), window);
    }

    result
}

// ---- テーマインポート ----

/// カラーパレット（インポート時の内部表現）
struct ImportedPalette {
    foreground: String,
    background: String,
    cursor: String,
    /// 16 ANSI 色 (black, red, green, yellow, blue, magenta, cyan, white, bright×8)
    ansi: Vec<String>,
}

/// テーマファイルをインポートしてカスタムパレットとして設定に書き込む
fn cmd_theme_import(path: String) -> Result<()> {
    let file_path = Path::new(&path);
    if !file_path.exists() {
        bail!("ファイルが見つかりません: {}", path);
    }

    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("ファイルの読み込みに失敗しました: {}", path))?;

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let palette = match ext.as_str() {
        "itermcolors" => parse_iterm_colors(&content)?,
        "yaml" | "yml" => parse_alacritty_yaml(&content)?,
        "toml" => parse_base16_toml(&content)?,
        other => bail!(
            "未対応のファイル形式です: .{} (対応形式: .itermcolors, .yaml, .yml, .toml)",
            other
        ),
    };

    // 設定ファイルのパス
    let config_path = {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.config/nexterm/config.toml", home)
    };

    write_custom_palette(&config_path, &palette)?;

    // インポートした色を表示
    println!("テーマをインポートしました: {}", path);
    println!("  foreground: {}", palette.foreground);
    println!("  background: {}", palette.background);
    println!("  cursor:     {}", palette.cursor);
    println!("  ANSI 16色:");
    let names = [
        "black  ",
        "red    ",
        "green  ",
        "yellow ",
        "blue   ",
        "magenta",
        "cyan   ",
        "white  ",
        "br-black  ",
        "br-red    ",
        "br-green  ",
        "br-yellow ",
        "br-blue   ",
        "br-magenta",
        "br-cyan   ",
        "br-white  ",
    ];
    for (i, color) in palette.ansi.iter().enumerate() {
        let label = names.get(i).copied().unwrap_or("?");
        println!("    [{}] {}: {}", i, label, color);
    }
    println!("設定ファイルに書き込みました: {}", config_path);

    Ok(())
}

// ---------------------------------------------------------------------------
// iTerm2 .itermcolors パーサ
// ---------------------------------------------------------------------------

/// RGB float (0.0–1.0) を #RRGGBB に変換する
fn rgb_float_to_hex(r: f64, g: f64, b: f64) -> String {
    let ri = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let gi = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let bi = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", ri, gi, bi)
}

/// XML テキストから `<key>K</key>` の直後のブロックから値を取り出す
fn iterm_extract_color(xml: &str, color_key: &str) -> Option<String> {
    // "Ansi 0 Color" などのキーを探す
    let search = format!("<key>{}</key>", color_key);
    let start = xml.find(&search)?;
    let after_key = &xml[start + search.len()..];
    // その後の <dict> を探す
    let dict_start = after_key.find("<dict>")?;
    let dict_content = &after_key[dict_start..];
    let dict_end = dict_content.find("</dict>")?;
    let dict = &dict_content[..dict_end + 7];

    let r = iterm_extract_component(dict, "Red Component")?;
    let g = iterm_extract_component(dict, "Green Component")?;
    let b = iterm_extract_component(dict, "Blue Component")?;
    Some(rgb_float_to_hex(r, g, b))
}

fn iterm_extract_component(dict: &str, component_key: &str) -> Option<f64> {
    let key_tag = format!("<key>{}</key>", component_key);
    let pos = dict.find(&key_tag)?;
    let after = &dict[pos + key_tag.len()..];
    // <real>...</real> または <integer>...</integer>
    let val_str = if let Some(real_start) = after.find("<real>") {
        let inner = &after[real_start + 6..];
        let end = inner.find("</real>")?;
        &inner[..end]
    } else if let Some(int_start) = after.find("<integer>") {
        let inner = &after[int_start + 9..];
        let end = inner.find("</integer>")?;
        &inner[..end]
    } else {
        return None;
    };
    val_str.trim().parse::<f64>().ok()
}

fn parse_iterm_colors(content: &str) -> Result<ImportedPalette> {
    // ANSI 0–15 の対応
    let ansi_key_names = [
        "Ansi 0 Color",
        "Ansi 1 Color",
        "Ansi 2 Color",
        "Ansi 3 Color",
        "Ansi 4 Color",
        "Ansi 5 Color",
        "Ansi 6 Color",
        "Ansi 7 Color",
        "Ansi 8 Color",
        "Ansi 9 Color",
        "Ansi 10 Color",
        "Ansi 11 Color",
        "Ansi 12 Color",
        "Ansi 13 Color",
        "Ansi 14 Color",
        "Ansi 15 Color",
    ];

    let mut ansi = Vec::with_capacity(16);
    for key in &ansi_key_names {
        ansi.push(iterm_extract_color(content, key).unwrap_or_else(|| "#000000".to_string()));
    }

    let foreground =
        iterm_extract_color(content, "Foreground Color").unwrap_or_else(|| "#c5c8c6".to_string());
    let background =
        iterm_extract_color(content, "Background Color").unwrap_or_else(|| "#1d1f21".to_string());
    let cursor = iterm_extract_color(content, "Cursor Color").unwrap_or_else(|| foreground.clone());

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// Alacritty YAML パーサ
// ---------------------------------------------------------------------------

/// ラインから `key: '#RRGGBB'` または `key: '#RGB'` を抽出する
fn yaml_extract_hex(line: &str) -> Option<String> {
    // '#xxxxxx' または '#xxx' を含む行を探す
    let hash_pos = line.find('#')?;
    let after_hash = &line[hash_pos + 1..];
    // 引用符や空白で区切られた16進数列を取り出す
    let hex: String = after_hash
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.len() == 6 {
        Some(format!("#{}", hex.to_uppercase()))
    } else if hex.len() == 3 {
        // 短縮形を展開
        let r = &hex[0..1];
        let g = &hex[1..2];
        let b = &hex[2..3];
        Some(format!("#{}{}{}{}{}{}", r, r, g, g, b, b).to_uppercase())
    } else {
        None
    }
}

fn parse_alacritty_yaml(content: &str) -> Result<ImportedPalette> {
    let mut foreground = "#c5c8c6".to_string();
    let mut background = "#1d1f21".to_string();
    let cursor = "#c5c8c6".to_string();
    let mut ansi = vec!["#000000".to_string(); 16];

    // ANSI color name → index mapping
    let normal_map: &[(&str, usize)] = &[
        ("black", 0),
        ("red", 1),
        ("green", 2),
        ("yellow", 3),
        ("blue", 4),
        ("magenta", 5),
        ("cyan", 6),
        ("white", 7),
    ];
    let bright_map: &[(&str, usize)] = &[
        ("black", 8),
        ("red", 9),
        ("green", 10),
        ("yellow", 11),
        ("blue", 12),
        ("magenta", 13),
        ("cyan", 14),
        ("white", 15),
    ];

    let mut in_primary = false;
    let mut in_normal = false;
    let mut in_bright = false;
    let mut in_cursor_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        // Section detection (no leading spaces beyond indent)
        if trimmed.starts_with("primary:") {
            in_primary = true;
            in_normal = false;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("normal:") {
            in_primary = false;
            in_normal = true;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("bright:") {
            in_primary = false;
            in_normal = false;
            in_bright = true;
            in_cursor_section = false;
            continue;
        }
        if trimmed.starts_with("cursor:") && !trimmed.contains('#') {
            in_primary = false;
            in_normal = false;
            in_bright = false;
            in_cursor_section = true;
            continue;
        }
        // Top-level "colors:" resets sections
        if trimmed == "colors:" {
            in_primary = false;
            in_normal = false;
            in_bright = false;
            in_cursor_section = false;
            continue;
        }

        if in_primary {
            if trimmed.starts_with("background:") {
                if let Some(hex) = yaml_extract_hex(trimmed) {
                    background = hex;
                }
            } else if trimmed.starts_with("foreground:")
                && let Some(hex) = yaml_extract_hex(trimmed)
            {
                foreground = hex;
            }
        }

        if in_normal {
            for (name, idx) in normal_map {
                if trimmed.starts_with(name)
                    && let Some(hex) = yaml_extract_hex(trimmed)
                {
                    ansi[*idx] = hex;
                }
            }
        }

        if in_bright {
            for (name, idx) in bright_map {
                if trimmed.starts_with(name)
                    && let Some(hex) = yaml_extract_hex(trimmed)
                {
                    ansi[*idx] = hex;
                }
            }
        }

        let _ = in_cursor_section;
    }

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// base16 TOML パーサ
// ---------------------------------------------------------------------------

fn parse_base16_toml(content: &str) -> Result<ImportedPalette> {
    // base00–base0F を抽出する
    // base00 = background, base05 = foreground
    // ANSI マッピング: base16 → 16色
    let base_keys = [
        "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
        "base09", "base0A", "base0B", "base0C", "base0D", "base0E", "base0F",
    ];

    let mut bases: Vec<String> = vec!["#000000".to_string(); 16];

    for line in content.lines() {
        let trimmed = line.trim();
        // 大文字小文字どちらも対応
        for (i, key) in base_keys.iter().enumerate() {
            let key_lower = key.to_lowercase();
            let trimmed_lower = trimmed.to_lowercase();
            if trimmed_lower.starts_with(&key_lower) && trimmed_lower.contains('=') {
                // 値部分を取り出す: `base00 = "282828"` または `base00 = "#282828"`
                if let Some(eq_pos) = trimmed.find('=') {
                    let val = trimmed[eq_pos + 1..]
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    let hex = val.trim_start_matches('#');
                    if hex.len() == 6 {
                        bases[i] = format!("#{}", hex.to_uppercase());
                    }
                }
            }
        }
    }

    // base16 → ANSI 16色マッピング (standard base16 terminal mapping)
    // 0:black=base00, 1:red=base08, 2:green=base0B, 3:yellow=base0A,
    // 4:blue=base0D, 5:magenta=base0E, 6:cyan=base0C, 7:white=base05,
    // 8:br-black=base03, 9:br-red=base08, 10:br-green=base0B, 11:br-yellow=base0A,
    // 12:br-blue=base0D, 13:br-magenta=base0E, 14:br-cyan=base0C, 15:br-white=base07
    let ansi = vec![
        bases[0x00].clone(),
        bases[0x08].clone(),
        bases[0x0B].clone(),
        bases[0x0A].clone(),
        bases[0x0D].clone(),
        bases[0x0E].clone(),
        bases[0x0C].clone(),
        bases[0x05].clone(),
        bases[0x03].clone(),
        bases[0x08].clone(),
        bases[0x0B].clone(),
        bases[0x0A].clone(),
        bases[0x0D].clone(),
        bases[0x0E].clone(),
        bases[0x0C].clone(),
        bases[0x07].clone(),
    ];

    let background = bases[0x00].clone();
    let foreground = bases[0x05].clone();
    let cursor = bases[0x05].clone();

    Ok(ImportedPalette {
        foreground,
        background,
        cursor,
        ansi,
    })
}

// ---------------------------------------------------------------------------
// 設定ファイルへの書き込み
// ---------------------------------------------------------------------------

/// `~/.config/nexterm/config.toml` の `[color-scheme.custom]` セクションを
/// 更新（またはファイルを新規作成）する
fn write_custom_palette(config_path: &str, palette: &ImportedPalette) -> Result<()> {
    // ディレクトリを作成する
    if let Some(parent) = Path::new(config_path).parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("設定ディレクトリの作成に失敗しました: {}", parent.display())
        })?;
    }

    // 既存ファイルを読み込む（存在しない場合は空文字）
    let existing = if Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)
            .with_context(|| format!("設定ファイルの読み込みに失敗しました: {}", config_path))?
    } else {
        String::new()
    };

    // TOML フラグメントを生成する
    let ansi_array = palette
        .ansi
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");

    let new_section = format!(
        "\n[color-scheme.custom]\nforeground = \"{}\"\nbackground = \"{}\"\ncursor = \"{}\"\nansi = [{}]\n",
        palette.foreground, palette.background, palette.cursor, ansi_array
    );

    // 既存ファイルから [color-scheme.custom] セクションを除去してから追記する
    let cleaned = remove_toml_section(&existing, "color-scheme.custom");
    // また [colors] や [color-scheme] の単独セクションも置き換え対象外
    let final_content = format!("{}{}", cleaned.trim_end(), new_section);

    std::fs::write(config_path, final_content)
        .with_context(|| format!("設定ファイルへの書き込みに失敗しました: {}", config_path))?;

    Ok(())
}

/// TOML テキストから指定されたセクション `[section_name]` を削除する
fn remove_toml_section(content: &str, section_name: &str) -> String {
    let search = format!("[{}]", section_name);
    let mut result = Vec::new();
    let mut skip = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == search {
            skip = true;
            continue;
        }
        // 次のセクション見出しが来たらスキップ終了
        if skip && trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            skip = false;
        }
        if !skip {
            result.push(line);
        }
    }

    result.join("\n")
}

// ---- IPC 接続 ----

/// IPC 接続ラッパー（プラットフォーム非依存の read/write 半）
struct IpcConn {
    reader: Box<dyn AsyncRead + Unpin + Send>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
}

impl IpcConn {
    /// プラットフォームに応じて IPC に接続する
    async fn connect() -> Result<Self> {
        let mut conn: Self = {
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ClientOptions;
                let username = std::env::var("USERNAME").unwrap_or_else(|_| "nexterm".to_string());
                let pipe = format!("\\\\.\\pipe\\nexterm-{}", username);
                let stream = ClientOptions::new()
                    .open(&pipe)
                    .map_err(|e| anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e)))?;
                let (r, w) = tokio::io::split(stream);
                Self {
                    reader: Box::new(r),
                    writer: Box::new(w),
                }
            }

            #[cfg(unix)]
            {
                let uid = unsafe { libc::getuid() };
                let dir = std::env::var("XDG_RUNTIME_DIR")
                    .unwrap_or_else(|_| format!("/run/user/{}", uid));
                let path = format!("{}/nexterm.sock", dir);
                let stream = tokio::net::UnixStream::connect(&path)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", fl!("ctl-connect-failed", error = e)))?;
                let (r, w) = tokio::io::split(stream);
                Self {
                    reader: Box::new(r),
                    writer: Box::new(w),
                }
            }
        };

        // ハンドシェイク: 接続直後にプロトコルバージョンを送信し、HelloAck を受信する
        conn.send(ClientToServer::Hello {
            proto_version: nexterm_proto::PROTOCOL_VERSION,
            client_kind: nexterm_proto::ClientKind::Ctl,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .await?;
        match conn.recv().await? {
            ServerToClient::HelloAck { .. } => {}
            ServerToClient::Error { message } => {
                anyhow::bail!("サーバーからハンドシェイクエラー: {}", message);
            }
            other => {
                anyhow::bail!(
                    "予期しないハンドシェイク応答: {:?} （HelloAck を期待）",
                    other
                );
            }
        }

        Ok(conn)
    }

    /// メッセージを送信する（4B LE 長さプレフィックス + postcard）
    async fn send(&mut self, msg: ClientToServer) -> Result<()> {
        let payload = postcard::to_stdvec(&msg)?;
        let len = payload.len() as u32;
        self.writer.write_all(&len.to_le_bytes()).await?;
        self.writer.write_all(&payload).await?;
        Ok(())
    }

    /// メッセージを受信する
    async fn recv(&mut self) -> Result<ServerToClient> {
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        // 巨大な長さプレフィックスによる OOM 攻撃を防ぐ
        nexterm_proto::validate_msg_len(msg_len).map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut payload = vec![0u8; msg_len];
        self.reader.read_exact(&mut payload).await?;
        Ok(postcard::from_bytes(&payload)?)
    }
}

// ---- プラグイン管理コマンド ----

/// ロード済みプラグイン一覧を表示する
async fn cmd_plugin_list() -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ListPlugins).await?;
    match conn.recv().await? {
        ServerToClient::PluginList { paths } => {
            if paths.is_empty() {
                println!("ロード済みプラグインはありません");
            } else {
                println!("{:<6} Path", "No.");
                println!("{}", "-".repeat(60));
                for (i, path) in paths.iter().enumerate() {
                    println!("{:<6} {}", i + 1, path);
                }
            }
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// WASM プラグインをロードする
async fn cmd_plugin_load(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::LoadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// プラグインをアンロードする
async fn cmd_plugin_unload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::UnloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

/// プラグインを再ロードする
async fn cmd_plugin_reload(path: String) -> Result<()> {
    let mut conn = IpcConn::connect().await?;
    conn.send(ClientToServer::ReloadPlugin { path: path.clone() })
        .await?;
    match conn.recv().await? {
        ServerToClient::PluginOk { path, action } => {
            println!("プラグインを{}しました: {}", action, path);
        }
        ServerToClient::Error { message } => bail!("{}", message),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postcard_roundtrip_list_sessions() {
        let msg = ClientToServer::ListSessions;
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::ListSessions));
    }

    #[test]
    fn postcard_roundtrip_kill_session() {
        let msg = ClientToServer::KillSession {
            name: "main".to_string(),
        };
        let encoded = postcard::to_stdvec(&msg).unwrap();
        let decoded: ClientToServer = postcard::from_bytes(&encoded).unwrap();
        assert!(matches!(decoded, ClientToServer::KillSession { .. }));
    }
}
