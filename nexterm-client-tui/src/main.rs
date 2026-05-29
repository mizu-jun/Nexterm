//! `nexterm-client-tui` entry point.

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

    // Connect to the IPC socket.
    let mut conn = Connection::connect_tui().await?;

    // Attach to the session and report the terminal size to the server.
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    conn.send(nexterm_proto::ClientToServer::Attach {
        session_name: "main".to_string(),
    })
    .await?;
    conn.send(nexterm_proto::ClientToServer::Resize { cols, rows })
        .await?;

    // Initialize client-side state.
    let mut state = ClientState::new();

    // Run the main loop.
    run(conn, &mut state).await
}

/// Event loop — interleaves input handling and rendering.
async fn run(mut conn: Connection, state: &mut ClientState) -> Result<()> {
    use crossterm::{
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use ratatui::prelude::*;
    use std::io;

    // Switch the terminal into raw mode.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop and always restore the terminal on the way out.
    let result = event_loop(&mut terminal, &mut conn, state).await;

    // Restore the terminal state.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

/// Main event loop.
async fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
    conn: &mut Connection,
    state: &mut ClientState,
) -> Result<()> {
    use tokio::time::{Duration, interval};

    let mut tick = interval(Duration::from_millis(16)); // ~60 fps

    loop {
        // Drain any pending server messages (non-blocking).
        while let Ok(msg) = conn.try_recv() {
            state.apply_server_message(msg);
        }

        // Expire any toast notifications whose TTL has elapsed.
        state.tick_toasts();

        // Render the frame.
        terminal.draw(|frame| {
            render::draw(frame, state);
        })?;

        // Process key input (the current prefix mode is forwarded).
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
                    // Leave prefix mode and forward the command.
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
