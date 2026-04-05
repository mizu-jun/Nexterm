//! vte::Perform の実装 — VT シーケンスを Screen に反映する

use vte::Perform;

use crate::screen::{Screen, SemanticMarkKind};

impl Perform for Screen {
    /// 印字可能文字の書き込み
    fn print(&mut self, c: char) {
        self.write_char(c);
    }

    /// 制御文字（C0/C1）の処理
    fn execute(&mut self, byte: u8) {
        match byte {
            // BEL (ベル) — 通知フラグを立てる
            0x07 => {
                self.set_pending_bell();
            }
            // BS (バックスペース)
            0x08 => {
                if self.cursor().0 > 0 {
                    let (col, row) = self.cursor();
                    self.move_cursor(col - 1, row);
                }
            }
            // HT (水平タブ) — 次の8の倍数列へ
            0x09 => {
                let (col, row) = self.cursor();
                let next_tab = ((col / 8) + 1) * 8;
                self.move_cursor(next_tab.min(self.grid().width.saturating_sub(1)), row);
            }
            // LF / VT / FF (改行相当)
            0x0A..=0x0C => {
                self.advance_line();
            }
            // CR (キャリッジリターン)
            0x0D => {
                let (_, row) = self.cursor();
                self.move_cursor(0, row);
            }
            _ => {} // その他の制御文字は無視
        }
    }

    /// CSI シーケンス（エスケープコード）の処理
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // DEC プライベートモード（`?` プレフィックス付き）を処理する
        if intermediates.first() == Some(&b'?') {
            match action {
                'h' => {
                    self.dec_private_mode(params, true);
                    return;
                }
                'l' => {
                    self.dec_private_mode(params, false);
                    return;
                }
                _ => {}
            }
        }

        // パラメータをフラットな Vec<u16> に変換する
        let p: Vec<u16> = params
            .iter()
            .map(|sub| sub.first().copied().unwrap_or(0))
            .collect();

        // 第1・第2パラメータのデフォルト値を解決するヘルパー
        let p1 = |default: u16| if p.is_empty() || p[0] == 0 { default } else { p[0] };
        let p2 = |default: u16| if p.len() < 2 || p[1] == 0 { default } else { p[1] };

        match action {
            // CUP / HVP — カーソル位置移動（1始まり → 0始まり）
            'H' | 'f' => {
                let row = p1(1).saturating_sub(1);
                let col = p2(1).saturating_sub(1);
                self.move_cursor(col, row);
            }
            // CUU — カーソル上移動
            'A' => {
                let (col, row) = self.cursor();
                self.move_cursor(col, row.saturating_sub(p1(1)));
            }
            // CUD — カーソル下移動
            'B' => {
                let (col, row) = self.cursor();
                let new_row = (row + p1(1)).min(self.grid().height.saturating_sub(1));
                self.move_cursor(col, new_row);
            }
            // CUF — カーソル右移動
            'C' => {
                let (col, row) = self.cursor();
                let new_col = (col + p1(1)).min(self.grid().width.saturating_sub(1));
                self.move_cursor(new_col, row);
            }
            // CUB — カーソル左移動
            'D' => {
                let (col, row) = self.cursor();
                self.move_cursor(col.saturating_sub(p1(1)), row);
            }
            // CHA — カーソル列移動（1始まり）
            'G' => {
                let (_, row) = self.cursor();
                self.move_cursor(p1(1).saturating_sub(1), row);
            }
            // VPA — カーソル行移動（1始まり）
            'd' => {
                let (col, _) = self.cursor();
                self.move_cursor(col, p1(1).saturating_sub(1));
            }
            // ED — 画面消去
            'J' => {
                self.erase_in_display(p1(0));
            }
            // EL — 行消去
            'K' => {
                self.erase_in_line(p1(0));
            }
            // SGR — 属性設定
            'm' => {
                let sgr: Vec<u16> = params
                    .iter()
                    .map(|sub| sub.first().copied().unwrap_or(0))
                    .collect();
                self.apply_sgr(&sgr);
            }
            // DECSTBM — スクロール領域設定
            'r' => {
                let top = p1(1).saturating_sub(1);
                let bottom = p2(self.grid().height).saturating_sub(1);
                // Screen への直接アクセスが必要なため screen.rs のメソッドを呼ぶ
                self.set_scroll_region(top, bottom);
            }
            _ => {} // 未対応の CSI シーケンスは無視
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // params[0] がコード、params[1] 以降がデータ
        if params.is_empty() {
            return;
        }
        let code = match std::str::from_utf8(params[0]) {
            Ok(s) => s.trim(),
            Err(_) => return,
        };
        match code {
            // OSC 0: アイコン名とウィンドウタイトルを設定
            // OSC 1: アイコン名を設定（タイトルとして扱う）
            // OSC 2: ウィンドウタイトルを設定
            "0" | "1" | "2" => {
                if let Some(title_bytes) = params.get(1)
                    && let Ok(title) = std::str::from_utf8(title_bytes) {
                        self.set_pending_title(title.to_string());
                    }
            }
            // OSC 8: ハイパーリンク
            // フォーマット: ESC ] 8 ; <params> ; <URI> BEL
            // URI が空文字列の場合はリンク終了
            "8" => {
                // params[1] = オプション（無視可）、params[2] = URI
                let uri = params
                    .get(2)
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .unwrap_or("")
                    .trim();
                if uri.is_empty() {
                    self.set_hyperlink(None);
                } else {
                    self.set_hyperlink(Some(uri.to_string()));
                }
            }
            // OSC 9: iTerm2 互換デスクトップ通知
            // フォーマット: ESC ] 9 ; <メッセージ> BEL
            "9" => {
                if let Some(msg_bytes) = params.get(1)
                    && let Ok(msg) = std::str::from_utf8(msg_bytes) {
                        self.set_pending_notification("Nexterm".to_string(), msg.to_string());
                    }
            }
            // OSC 133: セマンティックゾーン（プロンプト / コマンド / 出力のマーキング）
            // フォーマット: ESC ] 133 ; <A|B|C|D[;exit_code]> ST
            "133" => {
                if let Some(mark_bytes) = params.get(1)
                    && let Ok(mark) = std::str::from_utf8(mark_bytes)
                {
                    match mark.trim() {
                        "A" => {
                            self.add_semantic_mark(SemanticMarkKind::PromptStart, None);
                        }
                        "B" => {
                            self.add_semantic_mark(SemanticMarkKind::CommandStart, None);
                        }
                        "C" => {
                            self.add_semantic_mark(SemanticMarkKind::OutputStart, None);
                        }
                        "D" => {
                            // ESC ] 133 ; D ; <exit_code> BEL
                            // params[2] が exit_code（省略可能）
                            let exit_code = params
                                .get(2)
                                .and_then(|b| std::str::from_utf8(b).ok())
                                .and_then(|s| s.trim().parse::<i32>().ok());
                            self.add_semantic_mark(SemanticMarkKind::CommandEnd, exit_code);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}

    /// DCS 開始 — action == 'q' のとき Sixel
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'q' {
            self.start_sixel();
        }
    }

    /// DCS データバイト
    fn put(&mut self, byte: u8) {
        self.push_dcs_byte(byte);
    }

    /// DCS 終了 — Sixel デコードを確定する
    fn unhook(&mut self) {
        self.finish_sixel();
    }
}

impl Screen {
    /// DEC プライベートモードの設定（`?` プレフィックス付き CSI h / l）
    fn dec_private_mode(&mut self, params: &vte::Params, enable: bool) {
        for param in params.iter() {
            let mode = param.first().copied().unwrap_or(0);
            match mode {
                // DEC Private Mode 47 / 1047: 代替画面バッファ（カーソル保存なし）
                47 | 1047 => {
                    if enable {
                        self.switch_to_alt();
                    } else {
                        self.switch_to_primary();
                    }
                }
                // DEC Private Mode 1049: 代替画面バッファ（カーソル保存付き）
                1049 => {
                    if enable {
                        self.switch_to_alt();
                    } else {
                        self.switch_to_primary();
                    }
                }
                // DEC Private Mode 1000: X11 マウスレポーティング（基本クリック）
                1000 => {
                    self.mouse_mode = if enable { 1 } else { 0 };
                }
                // DEC Private Mode 1006: SGR 拡張マウスレポーティング
                1006 => {
                    self.mouse_mode = if enable { 2 } else { 0 };
                }
                // DEC Private Mode 2004: ブラケットペーストモード
                2004 => {
                    self.set_bracketed_paste(enable);
                }
                // DEC Private Mode 2026: 同期出力モード
                2026 => {
                    self.set_synchronized_output(enable);
                }
                _ => {}
            }
        }
    }
}
