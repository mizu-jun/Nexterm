//! ratatui を使った描画処理

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use nexterm_proto::{Attrs, Color};

use crate::state::ClientState;

/// フレームを描画する
pub fn draw(frame: &mut Frame, state: &ClientState) {
    let area = frame.area();

    match state.focused_pane() {
        Some(pane_state) => {
            draw_pane(frame, area, pane_state);
        }
        None => {
            // 未接続 / 未アタッチ状態のプレースホルダー
            let placeholder = Paragraph::new("nexterm へ接続中...")
                .block(Block::default().borders(Borders::ALL).title("nexterm"));
            frame.render_widget(placeholder, area);
        }
    }
}

/// ペインのグリッドを描画する
fn draw_pane(frame: &mut Frame, area: Rect, pane: &crate::state::PaneState) {
    let grid = &pane.grid;
    let height = area.height.min(grid.height) as usize;
    let width = area.width.min(grid.width) as usize;

    // グリッドの各セルを ratatui の Span に変換して行ごとに描画する
    for row_idx in 0..height {
        let row = &grid.rows[row_idx];
        let mut spans: Vec<Span> = Vec::with_capacity(width);

        for col_idx in 0..width {
            let cell = &row[col_idx];
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

    // カーソルを設定する
    let cursor_x = area.left() + pane.cursor_col.min(area.width.saturating_sub(1));
    let cursor_y = area.top() + pane.cursor_row.min(area.height.saturating_sub(1));
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// Cell のスタイルを ratatui の Style に変換する
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

/// nexterm の Color を ratatui の Color に変換する
fn convert_color(color: Color) -> ratatui::style::Color {
    match color {
        Color::Default => ratatui::style::Color::Reset,
        Color::Indexed(n) => ratatui::style::Color::Indexed(n),
        Color::Rgb(r, g, b) => ratatui::style::Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexterm_proto::Color as NColor;

    #[test]
    fn デフォルト色がResetに変換される() {
        let c = convert_color(NColor::Default);
        assert_eq!(c, ratatui::style::Color::Reset);
    }

    #[test]
    fn rgb色が正しく変換される() {
        let c = convert_color(NColor::Rgb(255, 128, 0));
        assert_eq!(c, ratatui::style::Color::Rgb(255, 128, 0));
    }

    #[test]
    fn indexed色が正しく変換される() {
        let c = convert_color(NColor::Indexed(42));
        assert_eq!(c, ratatui::style::Color::Indexed(42));
    }
}
