//! nexterm-client-tui エントリーポイント

mod connection;
mod input;
mod render;
mod state;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

use connection::{Connection, ConnectionExt};
use state::ClientState;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_env("NEXTERM_LOG"))
        .init();

    // IPC ソケットへ接続する
    let mut conn = Connection::connect_tui().await?;

    // セッションにアタッチしてターミナルサイズを通知する
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    conn.send(nexterm_proto::ClientToServer::Attach {
        session_name: "main".to_string(),
    })
    .await?;
    conn.send(nexterm_proto::ClientToServer::Resize { cols, rows })
        .await?;

    // クライアント状態を初期化する
    let mut state = ClientState::new();

    // メインループを実行する
    run(conn, &mut state).await
}

/// イベントループ — 入力処理と描画を交互に実行する
async fn run(mut conn: Connection, state: &mut ClientState) -> Result<()> {
    use crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use ratatui::prelude::*;
    use std::io;

    // 端末を raw モードに切り替える
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // クリーンアップのための defer
    let result = event_loop(&mut terminal, &mut conn, state).await;

    // 端末を元に戻す
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

/// メインイベントループ
async fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
    conn: &mut Connection,
    state: &mut ClientState,
) -> Result<()> {
    use tokio::time::{Duration, interval};

    let mut tick = interval(Duration::from_millis(16)); // ~60fps

    loop {
        // サーバーからのメッセージを受信する（non-blocking）
        while let Ok(msg) = conn.try_recv() {
            state.apply_server_message(msg);
        }

        // エラートーストの期限チェックをする
        state.tick_toasts();

        // 画面を描画する
        terminal.draw(|frame| {
            render::draw(frame, state);
        })?;

        // キー入力を処理する（現在のプレフィックスモードを渡す）
        if let Some(action) = input::poll_input(state.prefix_mode)? {
            use input::Action::*;
            match action {
                Quit => break,
                SendKey(key_msg) => conn.send(key_msg).await?,
                Resize(cols, rows) => {
                    state.resize(cols, rows);
                    conn.send(nexterm_proto::ClientToServer::Resize { cols, rows })
                        .await?;
                }
                EnterPrefix => {
                    state.enter_prefix();
                }
                PrefixCommand(cmd) => {
                    // プレフィックスモードを解除してコマンドを送信する
                    state.exit_prefix();
                    conn.send(cmd).await?;
                }
                ToggleHelp => {
                    state.toggle_help();
                }
                CancelPrefix => {
                    state.exit_prefix();
                }
            }
        }

        tick.tick().await;
    }

    Ok(())
}
