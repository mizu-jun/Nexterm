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
//!
//! 内部構成（Sprint 5-4 / A1）:
//! - `cmd/` — サブコマンド実装（list/new/attach/kill/record/template/service/...）
//! - `ipc.rs` — IPC 接続ラッパー (`IpcConn`)

use anyhow::Result;
use clap::{Arg, Command};
use clap_complete::{Shell, generate};
use clap_mangen::Man;
use nexterm_i18n::fl;
use tracing_subscriber::EnvFilter;

mod cmd;
mod ipc;

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
                )
                .subcommand(
                    Command::new("list").about("List all built-in color themes"),
                )
                .subcommand(
                    Command::new("apply")
                        .about("Apply a built-in theme to config.toml")
                        .arg(
                            Arg::new("name")
                                .help("Theme name (dark / light / tokyonight / catppuccin / dracula / nord / onedark / solarized / gruvbox)")
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
        .subcommand(
            Command::new("wsl")
                .about("WSL distro management (Windows only)")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("import-profiles")
                        .about(
                            "Detect installed WSL distros and add them as profiles to config.toml",
                        )
                        .arg(
                            Arg::new("dry-run")
                                .long("dry-run")
                                .help("Show what would be added without writing to config")
                                .action(clap::ArgAction::SetTrue),
                        ),
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
        Some(("list", _)) => cmd::session::cmd_list().await,
        Some(("new", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .expect("clap required arg")
                .clone();
            cmd::session::cmd_new(name).await
        }
        Some(("attach", sub)) => {
            let name = sub.get_one::<String>("name").expect("clap required arg");
            cmd::session::cmd_attach(name)
        }
        Some(("kill", sub)) => {
            let name = sub
                .get_one::<String>("name")
                .expect("clap required arg")
                .clone();
            cmd::session::cmd_kill(name).await
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
                cmd::record::cmd_record_start(session, file).await
            }
            Some(("stop", s)) => {
                let session = s
                    .get_one::<String>("session")
                    .expect("clap required arg")
                    .clone();
                cmd::record::cmd_record_stop(session).await
            }
            _ => unreachable!(),
        },
        Some(("service", sub)) => match sub.subcommand() {
            Some(("install", _)) => cmd::service::cmd_service_install(),
            Some(("uninstall", _)) => cmd::service::cmd_service_uninstall(),
            Some(("status", _)) => cmd::service::cmd_service_status(),
            _ => unreachable!(),
        },
        Some(("import-ghostty", sub)) => {
            let path = sub.get_one::<String>("path").cloned();
            let output = sub.get_one::<String>("output").cloned();
            cmd::ghostty::cmd_import_ghostty(path, output)
        }
        Some(("theme", sub)) => match sub.subcommand() {
            Some(("import", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd::theme::cmd_theme_import(path)
            }
            Some(("list", _)) => cmd::theme::cmd_theme_list(),
            Some(("apply", s)) => {
                let name = s
                    .get_one::<String>("name")
                    .expect("clap required arg")
                    .clone();
                cmd::theme::cmd_theme_apply(name)
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
                cmd::template::cmd_template_save(name, session).await
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
                cmd::template::cmd_template_load(name, session).await
            }
            Some(("list", _)) => cmd::template::cmd_template_list().await,
            _ => unreachable!(),
        },
        Some(("wsl", sub)) => match sub.subcommand() {
            Some(("import-profiles", s)) => {
                let dry_run = s.get_flag("dry-run");
                cmd::wsl::cmd_wsl_import_profiles(dry_run)
            }
            _ => unreachable!(),
        },
        Some(("plugin", sub)) => match sub.subcommand() {
            Some(("list", _)) => cmd::plugin::cmd_plugin_list().await,
            Some(("load", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd::plugin::cmd_plugin_load(path).await
            }
            Some(("unload", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd::plugin::cmd_plugin_unload(path).await
            }
            Some(("reload", s)) => {
                let path = s
                    .get_one::<String>("path")
                    .expect("clap required arg")
                    .clone();
                cmd::plugin::cmd_plugin_reload(path).await
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use nexterm_proto::ClientToServer;

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
