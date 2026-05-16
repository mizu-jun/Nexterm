//! Sprint 4-1: 機密操作の同意フロー
//!
//! `event_handler.rs` から抽出した:
//! - `handle_notification_request` / `handle_clipboard_write_request` / `request_open_url`
//! - `resolve_pending_consent`（input_handler から呼び出される）
//! - `truncate_utf8_at` ヘルパー

use super::EventHandler;

impl EventHandler {
    /// OSC 9 / 777 デスクトップ通知要求を処理する
    ///
    /// `SecurityConfig.osc_notification` ポリシー + セッション内 override に従って
    /// 即時送信 / 拒否 / 同意ダイアログ表示のいずれかを行う。
    pub(super) fn handle_notification_request(
        &mut self,
        pane_id: u32,
        title: String,
        body: String,
    ) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc_notification;
        let session_override = self.app.state.session_consent_overrides.osc_notification;
        // 通知本文は config の上限で切り詰める（DoS 対策）
        let max = self.app.config.security.notification_max_bytes;
        let body = truncate_utf8_at(&body, max);

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                // ダイアログを表示。ユーザーの選択は input_handler 側で処理する
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::Notification {
                        source_pane: pane_id,
                        title,
                        body,
                    },
                ));
                return;
            }
        };

        if allow {
            crate::notification::send_notification(&title, &body);
        } else {
            tracing::info!(
                "デスクトップ通知をポリシーで拒否しました: pane={} title={:?}",
                pane_id,
                title
            );
        }
    }

    /// OSC 52 クリップボード書き込み要求を処理する
    ///
    /// `SecurityConfig.osc52_clipboard` ポリシー + セッション内 override に従って
    /// 即時書き込み / 拒否 / 同意ダイアログ表示のいずれかを行う。
    pub(super) fn handle_clipboard_write_request(&mut self, pane_id: u32, text: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.osc52_clipboard;
        let session_override = self.app.state.session_consent_overrides.osc52_clipboard;
        // 設定で許可された最大バイト数を超える要求は無条件で拒否
        let max = self.app.config.security.osc52_max_bytes;
        if text.len() > max {
            tracing::warn!(
                "OSC 52 要求をサイズ上限超過で拒否: pane={} bytes={} max={}",
                pane_id,
                text.len(),
                max
            );
            return;
        }

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::ClipboardWrite {
                        source_pane: Some(pane_id),
                        text,
                    },
                ));
                return;
            }
        };

        if allow {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    if let Err(e) = clipboard.set_text(text) {
                        tracing::warn!("OSC 52 クリップボード書き込み失敗: {}", e);
                    }
                }
                Err(_) => {
                    tracing::warn!("OSC 52: クリップボード API を初期化できません");
                }
            }
        } else {
            tracing::info!(
                "OSC 52 クリップボード要求をポリシーで拒否しました: pane={}",
                pane_id
            );
        }
    }

    /// 外部 URL を開く要求を処理する（Ctrl+クリック / OSC 8 経由）
    ///
    /// `SecurityConfig.external_url` ポリシー + セッション内 override に従う。
    pub(super) fn request_open_url(&mut self, url: String) {
        use nexterm_config::ConsentPolicy;
        let policy = self.app.config.security.external_url;
        let session_override = self.app.state.session_consent_overrides.external_url;

        let allow = match (policy, session_override) {
            (_, Some(decision)) => decision,
            (ConsentPolicy::Allow, _) => true,
            (ConsentPolicy::Deny, _) => false,
            (ConsentPolicy::Prompt, _) => {
                self.app.state.pending_consent = Some(crate::state::ConsentDialog::new(
                    crate::state::ConsentKind::OpenUrl(url),
                ));
                return;
            }
        };

        if allow {
            crate::vertex_util::open_url(&url);
        } else {
            tracing::info!("URL オープン要求をポリシーで拒否しました: {}", url);
        }
    }

    /// 同意ダイアログのユーザー決定を実行する
    ///
    /// 呼び出し側 (input_handler) はキー入力を解釈してこのメソッドを呼ぶ。
    /// 引数 `decision`:
    /// - `Some(true)`: 1 度だけ許可
    /// - `Some(false)`: 1 度だけ拒否
    /// - `None`: ダイアログ閉じるのみ（拒否扱い）
    ///
    /// 引数 `always`: true なら同種の要求をセッション中常に同じ決定で扱う
    pub(in crate::renderer) fn resolve_pending_consent(
        &mut self,
        decision: Option<bool>,
        always: bool,
    ) {
        let Some(dialog) = self.app.state.pending_consent.take() else {
            return;
        };
        let allow = decision.unwrap_or(false);

        if always {
            match &dialog.kind {
                crate::state::ConsentKind::OpenUrl(_) => {
                    self.app.state.session_consent_overrides.external_url = Some(allow);
                }
                crate::state::ConsentKind::ClipboardWrite { .. } => {
                    self.app.state.session_consent_overrides.osc52_clipboard = Some(allow);
                }
                crate::state::ConsentKind::Notification { .. } => {
                    self.app.state.session_consent_overrides.osc_notification = Some(allow);
                }
            }
        }

        if !allow {
            return;
        }

        match dialog.kind {
            crate::state::ConsentKind::OpenUrl(url) => {
                crate::vertex_util::open_url(&url);
            }
            crate::state::ConsentKind::ClipboardWrite { text, .. } => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Err(e) = clipboard.set_text(text)
                {
                    tracing::warn!("クリップボード書き込み失敗: {}", e);
                }
            }
            crate::state::ConsentKind::Notification { title, body, .. } => {
                crate::notification::send_notification(&title, &body);
            }
        }
    }
}

/// バイト長で UTF-8 文字境界を尊重しつつ文字列を切り詰める
fn truncate_utf8_at(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}
