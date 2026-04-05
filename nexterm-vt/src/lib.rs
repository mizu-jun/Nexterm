//! nexterm-vt — VT シーケンスパーサ + 仮想グリッド実装
//!
//! vte クレートを使って端末エスケープシーケンスをパースし、
//! Cell の二次元配列（仮想グリッド）に反映する。

pub mod image;
mod performer;
mod screen;

pub use screen::{PendingImage, Screen, SemanticMark, SemanticMarkKind};

/// VT シーケンスを処理してグリッドを更新するパーサ
pub struct VtParser {
    parser: vte::Parser,
    screen: Screen,
    /// APC シーケンス（Kitty グラフィックス）受信中フラグ
    apc_active: bool,
    /// APC データ累積バッファ
    apc_buf: Vec<u8>,
    /// 直前のバイトが ESC (0x1B) だったかどうか
    apc_pending_esc: bool,
}

impl VtParser {
    /// 指定サイズの仮想スクリーンを持つパーサを生成する
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: vte::Parser::new(),
            screen: Screen::new(cols, rows),
            apc_active: false,
            apc_buf: Vec::new(),
            apc_pending_esc: false,
        }
    }

    /// バイト列を処理してグリッドを更新する
    ///
    /// vte 0.13 は APC コールバックを持たないため、APC シーケンス（Kitty グラフィックス）を
    /// ここでインターセプトして Screen へ渡す。APC 以外のバイト列は vte に委譲する。
    pub fn advance(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            // ESC の次のバイトで APC 開始 / 終了を判定する
            if self.apc_pending_esc {
                self.apc_pending_esc = false;
                match byte {
                    b'_' => {
                        // ESC _ = APC 開始
                        self.apc_active = true;
                        self.apc_buf.clear();
                        continue;
                    }
                    b'\\' if self.apc_active => {
                        // ESC \ = ST（String Terminator）= APC 終了
                        let data = std::mem::take(&mut self.apc_buf);
                        self.screen.handle_kitty_apc(&data);
                        self.apc_active = false;
                        continue;
                    }
                    _ => {
                        // APC 以外の ESC シーケンス — vte に ESC + 現在バイトを渡す
                        if self.apc_active {
                            // APC 中の孤立 ESC はバッファに追加する
                            self.apc_buf.push(0x1b);
                            self.apc_buf.push(byte);
                        } else {
                            self.parser.advance(&mut self.screen, 0x1b);
                            self.parser.advance(&mut self.screen, byte);
                        }
                        continue;
                    }
                }
            }

            if byte == 0x1b {
                // ESC: 次のバイトで判定するため保留する
                self.apc_pending_esc = true;
                continue;
            }

            if self.apc_active {
                self.apc_buf.push(byte);
            } else {
                self.parser.advance(&mut self.screen, byte);
            }
        }
    }

    /// 現在のスクリーン状態への参照を返す
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// 現在のスクリーン状態への可変参照を返す
    pub fn screen_mut(&mut self) -> &mut Screen {
        &mut self.screen
    }

    /// ブラケットペーストモード（DEC ?2004）が有効かどうかを返す
    pub fn bracketed_paste_mode(&self) -> bool {
        self.screen.bracketed_paste_mode()
    }

    /// 同期出力モード（DEC ?2026）が有効かどうかを返す
    pub fn synchronized_output_mode(&self) -> bool {
        self.screen.synchronized_output_mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 通常文字を書き込める() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'e');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'o');
    }

    #[test]
    fn キャリッジリターン改行でカーソルが移動する() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Line1\r\nLine2");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'L');
        assert_eq!(grid.get(0, 1).unwrap().ch, 'L');
    }

    #[test]
    fn カーソル位置指定エスケープが動作する() {
        let mut parser = VtParser::new(80, 24);
        // CSI 5;10H → 行5列10へ移動（1始まり）
        parser.advance(b"\x1b[5;10HA");
        let grid = parser.screen().grid();
        // 行4列9（0始まり）に 'A' が書かれる
        assert_eq!(grid.get(9, 4).unwrap().ch, 'A');
    }

    #[test]
    fn ダーティフラグが書き込みで立つ() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        let screen = parser.screen();
        assert!(screen.is_dirty(0), "行0はダーティであるべき");
        assert!(!screen.is_dirty(1), "行1はクリーンであるべき");
    }

    #[test]
    fn ダーティフラグをクリアできる() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"X");
        parser.screen_mut().clear_dirty();
        assert!(!parser.screen().is_dirty(0));
    }

    #[test]
    fn リサイズで新しいサイズに変わる() {
        let mut parser = VtParser::new(80, 24);
        parser.screen_mut().resize(120, 40);
        let grid = parser.screen().grid();
        assert_eq!(grid.width, 120);
        assert_eq!(grid.height, 40);
    }

    #[test]
    fn ブラケットペーストモードが初期状態で無効() {
        let parser = VtParser::new(80, 24);
        assert!(!parser.bracketed_paste_mode());
    }

    #[test]
    fn ブラケットペーストモードを有効化できる() {
        let mut parser = VtParser::new(80, 24);
        // CSI ?2004h — ブラケットペーストモード有効化
        parser.advance(b"\x1b[?2004h");
        assert!(parser.bracketed_paste_mode(), "?2004h で有効になるべき");
    }

    #[test]
    fn 同期出力モードが初期状態で無効() {
        let parser = VtParser::new(80, 24);
        assert!(!parser.synchronized_output_mode());
    }

    #[test]
    fn 同期出力モード有効中はダーティ行を返さない() {
        let mut parser = VtParser::new(80, 24);
        // モード有効化
        parser.advance(b"\x1b[?2026h");
        assert!(parser.synchronized_output_mode());
        // テキスト書き込み
        parser.advance(b"Hello");
        // ダーティ行は空（保留中）
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(dirty.is_empty(), "同期出力モード中はダーティ行を返さないべき");
        // モード無効化でフラッシュ
        parser.advance(b"\x1b[?2026l");
        assert!(!parser.synchronized_output_mode());
        let dirty = parser.screen_mut().take_dirty_rows();
        assert!(!dirty.is_empty(), "モード無効化後にダーティ行を返すべき");
    }

    #[test]
    fn ブラケットペーストモードを無効化できる() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[?2004h");
        assert!(parser.bracketed_paste_mode());
        // CSI ?2004l — 無効化
        parser.advance(b"\x1b[?2004l");
        assert!(!parser.bracketed_paste_mode(), "?2004l で無効になるべき");
    }

    #[test]
    fn osc133セマンティックゾーンが記録される() {
        let mut parser = VtParser::new(80, 24);
        // A: プロンプト開始 → B: コマンド開始 → C: 出力開始 → D;0: 終了
        parser.advance(b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07");
        let marks = parser.screen_mut().take_semantic_marks();
        assert_eq!(marks.len(), 4, "4つのマークが記録されること");
        assert!(matches!(marks[0].kind, SemanticMarkKind::PromptStart));
        assert!(matches!(marks[1].kind, SemanticMarkKind::CommandStart));
        assert!(matches!(marks[2].kind, SemanticMarkKind::OutputStart));
        assert!(matches!(marks[3].kind, SemanticMarkKind::CommandEnd));
        assert_eq!(marks[3].exit_code, Some(0));
    }

    #[test]
    fn osc133コマンド失敗時にexit_codeが記録される() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b]133;D;1\x07");
        let marks = parser.screen_mut().take_semantic_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].exit_code, Some(1));
    }

    #[test]
    fn osc8ハイパーリンクがグリッドに記録される() {
        let mut parser = VtParser::new(80, 24);
        // ESC ] 8 ; ; https://example.com BEL + テキスト + リンク終了
        parser.advance(b"\x1b]8;;https://example.com\x07Click\x1b]8;;\x07");
        let grid = parser.screen().grid();
        // 文字が書き込まれている
        assert_eq!(grid.get(0, 0).unwrap().ch, 'C');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'k');
        // hyperlinks にスパンが記録されている
        assert!(!grid.hyperlinks.is_empty(), "ハイパーリンクスパンが存在すること");
        let span = &grid.hyperlinks[0];
        assert_eq!(span.url, "https://example.com");
        assert_eq!(span.row, 0);
        assert_eq!(span.col_start, 0);
        assert_eq!(span.col_end, 5); // "Click" は 5 文字
    }

    // ---- Kitty グラフィックスプロトコル テスト ----

    /// 1x1 RGBA 画像の base64: [R=255, G=0, B=0, A=255]
    fn kitty_rgba_1x1_base64() -> &'static str {
        // RGBA [255, 0, 0, 255] を base64 エンコード = "/wAA/w=="
        "/wAA/w=="
    }

    #[test]
    fn kitty単一チャンクRGBA画像がデコードされる() {
        let mut parser = VtParser::new(80, 24);
        // ESC _ G a=T,f=32,s=1,v=1;<base64> ESC \
        let payload = kitty_rgba_1x1_base64();
        let seq = format!("\x1b_Ga=T,f=32,s=1,v=1;{}\x1b\\", payload);
        parser.advance(seq.as_bytes());
        let images = parser.screen_mut().take_pending_images();
        assert_eq!(images.len(), 1, "1枚の画像が登録されること");
        assert_eq!(images[0].width, 1);
        assert_eq!(images[0].height, 1);
        assert_eq!(images[0].rgba[0], 255); // R
        assert_eq!(images[0].rgba[1], 0);   // G
        assert_eq!(images[0].rgba[2], 0);   // B
        assert_eq!(images[0].rgba[3], 255); // A
    }

    #[test]
    fn kitty分割チャンク転送がデコードされる() {
        let mut parser = VtParser::new(80, 24);
        // 1x1 RGBA を 2 チャンクに分割して送る
        // "/wAA/w==" を "/wAA" + "/w==" に分割
        // チャンク1: m=1（続きあり）— サイズパラメータを含む
        parser.advance(b"\x1b_Ga=T,f=32,s=1,v=1,m=1;/wAA\x1b\\");
        // チャンク2: m=0（最終チャンク）
        parser.advance(b"\x1b_Gm=0;/w==\x1b\\");
        let images = parser.screen_mut().take_pending_images();
        assert_eq!(images.len(), 1, "分割チャンクから1枚の画像が組み立てられること");
        assert_eq!(images[0].width, 1);
        assert_eq!(images[0].height, 1);
    }

    #[test]
    fn kittyシーケンス後も通常テキストが処理される() {
        let mut parser = VtParser::new(80, 24);
        // Kitty APC の前後にテキストを配置する
        let payload = kitty_rgba_1x1_base64();
        let seq = format!("Hi\x1b_Ga=T,f=32,s=1,v=1;{}\x1b\\Bye", payload);
        parser.advance(seq.as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'i');
        assert_eq!(grid.get(2, 0).unwrap().ch, 'B');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'y');
        assert_eq!(grid.get(4, 0).unwrap().ch, 'e');
    }
}
