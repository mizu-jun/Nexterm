//! ratatui rendering.
//!
//! Layout:
//! ```text
//!   ┌──────────────────────────────┐
//!   │  Panes (rows - 1)            │
//!   ├──────────────────────────────┤
//!   │  Status bar (1 row)          │
//!   └──────────────────────────────┘
//! ```

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use nexterm_proto::Color;

use crate::state::{ClientState, PaneState, PrefixMode};

/// Render one frame.
pub fn draw(frame: &mut Frame, state: &ClientState) {
    let area = frame.area();

    // Reserve one row for the status bar.
    let status_height = 1u16;
    let pane_area_rows = area.height.saturating_sub(status_height);
    let pane_area = Rect::new(area.x, area.y, area.width, pane_area_rows);
    let status_area = Rect::new(area.x, area.y + pane_area_rows, area.width, status_height);

    // Render the panes.
    if state.pane_layouts.is_empty() {
        // No layout information yet: show the focused pane fullscreen.
        match state.focused_pane() {
            Some(pane_state) => {
                draw_pane(frame, pane_area, pane_state, true);
            }
            None => {
                draw_connecting(frame, pane_area);
            }
        }
    } else {
        // Layout information available: render each pane at its exact position.
        for layout in &state.pane_layouts {
            if let Some(pane_state) = state.panes.get(&layout.pane_id) {
                // Convert server coordinates into terminal coordinates (the status bar offset
                // is already excluded; clamp into `pane_area`).
                let x = area.x + layout.col_offset;
                let y = area.y + layout.row_offset;
                let max_cols = area.width.saturating_sub(layout.col_offset);
                let max_rows = pane_area_rows.saturating_sub(layout.row_offset);
                let cols = layout.cols.min(max_cols);
                let rows = layout.rows.min(max_rows);

                if cols == 0 || rows == 0 {
                    continue;
                }

                let rect = Rect::new(x, y, cols, rows);
                let is_focused = layout.pane_id == state.focused_pane_id.unwrap_or(0);
                draw_pane(frame, rect, pane_state, is_focused);
            }
        }
    }

    // Render the status bar.
    draw_status_bar(frame, status_area, state);

    // Render the error toast at the top of the screen (overlaid).
    if let Some(toast) = &state.error_toast {
        draw_error_toast(frame, area, &toast.message);
    }

    // Render the help overlay.
    if state.prefix_mode == PrefixMode::Help {
        draw_help_overlay(frame, area);
    }
}

/// Render the "connecting" placeholder.
//
// User-facing strings here are intentionally English literals; routing them through
// `nexterm-i18n` is a follow-up item (see CLAUDE.md "Application-facing strings").
fn draw_connecting(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  nexterm",
            Style::default()
                .fg(Color::Rgb(100, 200, 255).into_ratatui())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Connecting to server...",
            Style::default().fg(ratatui::style::Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Ctrl+Q to quit",
            Style::default().fg(ratatui::style::Color::DarkGray),
        )),
    ];
    let para = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("nexterm"));
    frame.render_widget(para, area);
}

/// Render a single pane.
fn draw_pane(frame: &mut Frame, area: Rect, pane: &PaneState, is_focused: bool) {
    let grid = &pane.grid;
    let height = area.height as usize;
    let width = area.width as usize;

    // Convert each grid cell into a ratatui `Span` and render line by line.
    for row_idx in 0..height.min(grid.height as usize) {
        let row = &grid.rows[row_idx];
        let mut spans: Vec<Span> = Vec::with_capacity(width);

        for cell in row.iter().take(width) {
            let style = cell_to_style(cell);
            spans.push(Span::styled(cell.ch.to_string(), style));
        }

        let line = Line::from(spans);
        let y = area.top() + row_idx as u16;
        if y < area.bottom() {
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(area.left(), y, area.width, 1),
            );
        }
    }

    // Position the cursor (only on the focused pane).
    if is_focused {
        let cursor_x = area.left() + pane.cursor_col.min(area.width.saturating_sub(1));
        let cursor_y = area.top() + pane.cursor_row.min(area.height.saturating_sub(1));
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Render the status bar (the bottom row of the screen).
fn draw_status_bar(frame: &mut Frame, area: Rect, state: &ClientState) {
    // Swap out the left-hand text depending on the prefix mode.
    let left = match state.prefix_mode {
        PrefixMode::CtrlB => Span::styled(
            " -- PREFIX -- ",
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PrefixMode::Help => Span::styled(
            " -- HELP -- ",
            Style::default()
                .fg(ratatui::style::Color::Black)
                .bg(ratatui::style::Color::Cyan),
        ),
        PrefixMode::None => {
            let pane_count = state.pane_layouts.len().max(state.panes.len());
            let pane_index = state
                .focused_pane_id
                .and_then(|id| {
                    state
                        .pane_layouts
                        .iter()
                        .position(|l| l.pane_id == id)
                        .map(|i| i + 1)
                })
                .unwrap_or(1);
            let text = if pane_count > 1 {
                format!(
                    " [{}] pane {}/{}",
                    state.session_name, pane_index, pane_count
                )
            } else {
                format!(" [{}]", state.session_name)
            };
            Span::styled(
                text,
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(ratatui::style::Color::Cyan),
            )
        }
    };

    let hints = Span::styled(
        " Ctrl+B: % vsplit  \" hsplit  x close  ? help  q quit ",
        Style::default()
            .fg(ratatui::style::Color::DarkGray)
            .bg(ratatui::style::Color::Reset),
    );

    // Stack the left-justified status and right-aligned hints on the same line.
    let line = Line::from(vec![left, hints]);
    let para = Paragraph::new(line).style(Style::default().bg(ratatui::style::Color::Reset));
    frame.render_widget(para, area);
}

/// Render the error toast at the top of the screen.
fn draw_error_toast(frame: &mut Frame, area: Rect, message: &str) {
    let max_width = (area.width.saturating_sub(4)).min(60);
    let toast_width = (message.len() as u16 + 4).min(max_width);
    let x = area.x + (area.width.saturating_sub(toast_width)) / 2;
    let toast_rect = Rect::new(x, area.y, toast_width, 3);

    frame.render_widget(Clear, toast_rect);
    let para = Paragraph::new(format!(" {} ", message))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Error")
                .style(Style::default().fg(ratatui::style::Color::Red)),
        )
        .style(
            Style::default()
                .fg(ratatui::style::Color::White)
                .bg(ratatui::style::Color::Red),
        );
    frame.render_widget(para, toast_rect);
}

/// Render the help overlay (centered on the screen).
fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let width = 52u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let overlay_rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, overlay_rect);

    let lines = vec![
        Line::from(Span::styled(
            "  nexterm TUI key bindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [ Ctrl+B prefix commands ]",
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Ctrl+B  %   vertical split (left/right)"),
        Line::from("  Ctrl+B  \"   horizontal split (top/bottom)"),
        Line::from("  Ctrl+B  x   close the focused pane"),
        Line::from("  Ctrl+B  n   focus the next pane"),
        Line::from("  Ctrl+B  p   focus the previous pane"),
        Line::from("  Ctrl+B  z   toggle pane zoom"),
        Line::from("  Ctrl+B  ?   show this help"),
        Line::from(""),
        Line::from(Span::styled(
            "  [ Direct key bindings ]",
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Ctrl+Q      quit nexterm"),
        Line::from("  PageUp/Down scroll (not yet implemented)"),
        Line::from(""),
        Line::from(Span::styled(
            "  [ Navigation ]",
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Ctrl+B  ?   close this help"),
        Line::from("  Esc         leave prefix mode"),
    ];

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Help (press Ctrl+B ? or Esc to close)")
            .style(Style::default().fg(ratatui::style::Color::Cyan)),
    );
    frame.render_widget(para, overlay_rect);
}

/// Convert a `Cell` style into a ratatui `Style`.
fn cell_to_style(cell: &nexterm_proto::Cell) -> Style {
    let mut style = Style::default()
        .fg(convert_color(cell.fg))
        .bg(convert_color(cell.bg));

    if cell.attrs.is_bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.attrs.is_italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.attrs.is_underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.attrs.is_reverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }

    style
}

/// Convert a nexterm `Color` into a ratatui color.
fn convert_color(color: Color) -> ratatui::style::Color {
    match color {
        Color::Default => ratatui::style::Color::Reset,
        Color::Indexed(n) => ratatui::style::Color::Indexed(n),
        Color::Rgb(r, g, b) => ratatui::style::Color::Rgb(r, g, b),
    }
}

/// Trait extension that adds a ratatui conversion method to nexterm `Color`.
trait IntoRatatui {
    fn into_ratatui(self) -> ratatui::style::Color;
}

impl IntoRatatui for Color {
    fn into_ratatui(self) -> ratatui::style::Color {
        convert_color(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::Color as NColor;

    #[test]
    fn default_color_maps_to_reset() {
        let c = convert_color(NColor::Default);
        assert_eq!(c, ratatui::style::Color::Reset);
    }

    #[test]
    fn rgb_color_converts() {
        let c = convert_color(NColor::Rgb(255, 128, 0));
        assert_eq!(c, ratatui::style::Color::Rgb(255, 128, 0));
    }

    #[test]
    fn indexed_color_converts() {
        let c = convert_color(NColor::Indexed(42));
        assert_eq!(c, ratatui::style::Color::Indexed(42));
    }
}
