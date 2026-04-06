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

    // ---- ANSI 256色・True Color テスト ----

    #[test]
    fn sgr_256色前景色が設定される() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;5;196 = 256色インデックス 196（明るい赤）
        parser.advance(b"\x1b[38;5;196mX");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'X');
        assert_eq!(cell.fg, nexterm_proto::Color::Indexed(196));
    }

    #[test]
    fn sgr_256色背景色が設定される() {
        let mut parser = VtParser::new(80, 24);
        // SGR 48;5;21 = 256色インデックス 21（青）
        parser.advance(b"\x1b[48;5;21mY");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'Y');
        assert_eq!(cell.bg, nexterm_proto::Color::Indexed(21));
    }

    #[test]
    fn sgr_truecolor前景色が設定される() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;2;255;128;0 = RGB(255, 128, 0) オレンジ
        parser.advance(b"\x1b[38;2;255;128;0mZ");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'Z');
        assert_eq!(cell.fg, nexterm_proto::Color::Rgb(255, 128, 0));
    }

    #[test]
    fn sgr_truecolor背景色が設定される() {
        let mut parser = VtParser::new(80, 24);
        // SGR 48;2;0;255;128 = RGB(0, 255, 128) 緑
        parser.advance(b"\x1b[48;2;0;255;128mW");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'W');
        assert_eq!(cell.bg, nexterm_proto::Color::Rgb(0, 255, 128));
    }

    #[test]
    fn sgr_グレースケール256色が設定される() {
        let mut parser = VtParser::new(80, 24);
        // SGR 38;5;240 = グレースケール（232-255 の範囲）
        parser.advance(b"\x1b[38;5;240mG");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.fg, nexterm_proto::Color::Indexed(240));
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

    // ---- 追加 VT シーケンステスト ----

    #[test]
    fn sgr_bold属性が設定される() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"\x1b[1mB");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'B');
        assert!(cell.attrs.is_bold());
    }

    #[test]
    fn sgr_reset後に属性がクリアされる() {
        let mut parser = VtParser::new(80, 24);
        // BOLD を設定してからリセット
        parser.advance(b"\x1b[1m\x1b[0mX");
        let cell = parser.screen().grid().get(0, 0).unwrap();
        assert_eq!(cell.ch, 'X');
        assert!(!cell.attrs.is_bold());
    }

    #[test]
    fn ed_画面消去でセルがクリアされる() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        // CSI 2J = 画面全体消去
        parser.advance(b"\x1b[2J");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn el_行消去でセルがクリアされる() {
        let mut parser = VtParser::new(80, 24);
        parser.advance(b"Hello");
        // CSI 1G でカーソルを行頭へ
        parser.advance(b"\x1b[1G");
        // CSI 2K = 行全体消去
        parser.advance(b"\x1b[2K");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
    }

    #[test]
    fn 長いテキストが行末で折り返す() {
        let mut parser = VtParser::new(10, 5); // 幅 10 の狭い端末
        // 11文字書くと2行目に折り返す
        parser.advance(b"0123456789A");
        let grid = parser.screen().grid();
        // 1行目に 0〜9
        assert_eq!(grid.get(9, 0).unwrap().ch, '9');
        // 11文字目 'A' は2行目へ
        assert_eq!(grid.get(0, 1).unwrap().ch, 'A');
    }

    #[test]
    fn vtparser_new後の初期カーソル位置() {
        let parser = VtParser::new(80, 24);
        let grid = parser.screen().grid();
        assert_eq!(grid.cursor_col, 0);
        assert_eq!(grid.cursor_row, 0);
    }

    #[test]
    fn tab文字でカーソルが8の倍数に移動する() {
        let mut parser = VtParser::new(80, 24);
        // TAB の前に文字を書いてからカーソル位置を確認する
        // TAB 後に文字を書いて位置が 8 以降であることを確認する
        parser.advance(b"\tX");
        let grid = parser.screen().grid();
        // TAB 後に 'X' が書かれる位置は col=8 以降であること
        // （TAB が col=8 に移動し、X が col=8 に書かれる）
        assert_eq!(grid.get(8, 0).unwrap().ch, 'X');
    }

    // ─── CJK 全角文字テスト ──────────────────────────────────────────────────

    #[test]
    fn cjk全角文字が2カラム幅で配置される() {
        let mut parser = VtParser::new(80, 24);
        // 日本語全角文字（あ）は幅 2
        parser.advance("あ".as_bytes());
        let grid = parser.screen().grid();
        // col=0 に文字本体
        assert_eq!(grid.get(0, 0).unwrap().ch, 'あ');
        // col=1 はプレースホルダー（空白）
        assert_eq!(grid.get(1, 0).unwrap().ch, ' ');
        // カーソルは col=2 に進んでいること（Screen.cursor_col を参照する）
        assert_eq!(parser.screen().cursor().0, 2);
    }

    #[test]
    fn cjk複数全角文字が連続して配置される() {
        let mut parser = VtParser::new(80, 24);
        // 「日本語」= 3 文字 × 幅 2 = 6 カラム消費
        parser.advance("日本語".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '日');
        assert_eq!(grid.get(2, 0).unwrap().ch, '本');
        assert_eq!(grid.get(4, 0).unwrap().ch, '語');
        // カーソルは col=6 にあること
        assert_eq!(parser.screen().cursor().0, 6);
    }

    #[test]
    fn cjk全角と半角の混在() {
        let mut parser = VtParser::new(80, 24);
        // "A日B" → A(col=0), 日(col=1,2), B(col=3)
        parser.advance("A日B".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'A');
        assert_eq!(grid.get(1, 0).unwrap().ch, '日');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'B');
        assert_eq!(parser.screen().cursor().0, 4);
    }

    #[test]
    fn cjk行末で折り返す() {
        // 幅 5 の端末で全角文字が行末端（col=4）にくる場合は次行に折り返す
        // "ABCD" + 全角"あ": 幅2 の「あ」が col=4 から始まると
        // col+1=5 >= width=5 となり折り返しが発生する
        let mut parser = VtParser::new(5, 5);
        parser.advance("ABCDあ".as_bytes());
        let grid = parser.screen().grid();
        // ABCD は 1 行目の col=0〜3 に配置される
        assert_eq!(grid.get(0, 0).unwrap().ch, 'A');
        assert_eq!(grid.get(3, 0).unwrap().ch, 'D');
        // 「あ」は幅 2 で col=4 に入らないため 2 行目の col=0 に折り返す
        assert_eq!(grid.get(0, 1).unwrap().ch, 'あ');
    }

    #[test]
    fn 中国語簡体字が2カラム幅で配置される() {
        let mut parser = VtParser::new(80, 24);
        // 中国語文字（汉字）は幅 2
        parser.advance("汉字".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '汉');
        assert_eq!(grid.get(2, 0).unwrap().ch, '字');
        assert_eq!(parser.screen().cursor().0, 4);
    }

    #[test]
    fn 韓国語ハングルが2カラム幅で配置される() {
        let mut parser = VtParser::new(80, 24);
        // ハングル音節（가）は幅 2
        parser.advance("가나다".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '가');
        assert_eq!(grid.get(2, 0).unwrap().ch, '나');
        assert_eq!(grid.get(4, 0).unwrap().ch, '다');
        assert_eq!(parser.screen().cursor().0, 6);
    }

    #[test]
    fn 半角カタカナは1カラム幅() {
        let mut parser = VtParser::new(80, 24);
        // 半角カタカナ（ｱｲｳ）は幅 1
        parser.advance("ｱｲｳ".as_bytes());
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, 'ｱ');
        assert_eq!(grid.get(1, 0).unwrap().ch, 'ｲ');
        assert_eq!(grid.get(2, 0).unwrap().ch, 'ｳ');
        assert_eq!(parser.screen().cursor().0, 3);
    }

    #[test]
    fn cjk全角文字のカラーが引き継がれる() {
        let mut parser = VtParser::new(80, 24);
        // SGR で赤（ANSI 31）に設定してから全角文字を書く
        parser.advance(b"\x1b[31m");
        parser.advance("あ".as_bytes());
        let grid = parser.screen().grid();
        // 本体セルは赤色
        use nexterm_proto::Color;
        assert_eq!(grid.get(0, 0).unwrap().fg, Color::Indexed(1)); // ANSI red = index 1
        // プレースホルダーセルも同じ前景色
        assert_eq!(grid.get(1, 0).unwrap().fg, Color::Indexed(1));
    }

    #[test]
    fn cjk全角文字とsgr_リセットが正しく機能する() {
        let mut parser = VtParser::new(80, 24);
        // 太字 + 全角文字
        parser.advance(b"\x1b[1m");
        parser.advance("漢".as_bytes());
        // リセット後の通常文字
        parser.advance(b"\x1b[0m");
        parser.advance(b"X");
        let grid = parser.screen().grid();
        assert_eq!(grid.get(0, 0).unwrap().ch, '漢');
        assert!(grid.get(0, 0).unwrap().attrs.is_bold());
        assert_eq!(grid.get(2, 0).unwrap().ch, 'X');
        assert!(!grid.get(2, 0).unwrap().attrs.is_bold());
    }

    #[test]
    fn cjk文字のリサイズ後も正しく動作する() {
        let mut parser = VtParser::new(80, 24);
        parser.advance("あいう".as_bytes());
        // リサイズ後も全角文字書き込みが正常に動作することを確認する
        parser.screen.resize(40, 12);
        parser.advance("えお".as_bytes());
        let grid = parser.screen().grid();
        // リサイズ後に書いた「え」「お」がグリッド内に存在すること
        let row0: String = grid.rows[0].iter().map(|c| c.ch).collect();
        assert!(row0.contains('え') || row0.contains('お'));
    }
}
