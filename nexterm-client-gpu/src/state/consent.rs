//! 機密操作の同意ダイアログ（Sprint 4-1）
//!
//! `state/mod.rs` から抽出した:
//! - `ConsentKind` — 同意要求の種別（OpenUrl / ClipboardWrite / Notification）
//! - `ConsentDialog` — ダイアログの状態（種別と選択中ボタンインデックス）
//! - `SessionConsentOverrides` — セッション中の「常に許可 / 拒否」オーバーライド

/// 同意ダイアログの種別ごとに「セッション内で常に許可/拒否」を保持する
///
/// `None` のままならポリシーに従う（つまり再びダイアログが出る）。
/// ユーザーが [Always Allow] / [Always Deny] を選んだ場合のみ書き込まれる。
#[derive(Default)]
pub struct SessionConsentOverrides {
    pub external_url: Option<bool>,
    pub osc52_clipboard: Option<bool>,
    pub osc_notification: Option<bool>,
}

/// 同意ダイアログの種別
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsentKind {
    /// 外部 URL を開こうとしている
    OpenUrl(String),
    /// OSC 52 クリップボード書き込み要求
    ClipboardWrite {
        /// 要求元ペイン ID（None = ローカル発行）
        source_pane: Option<u32>,
        /// 書き込みテキスト
        text: String,
    },
    /// OSC 9 / 777 デスクトップ通知要求
    Notification {
        /// 要求元ペイン ID
        source_pane: u32,
        /// 通知タイトル
        title: String,
        /// 通知本文
        body: String,
    },
}

/// 同意ダイアログの状態
#[derive(Clone, Debug)]
pub struct ConsentDialog {
    /// 同意要求の種別
    pub kind: ConsentKind,
    /// 選択中のボタンインデックス (0=Allow, 1=Deny, 2=Always Allow, 3=Always Deny)
    pub selected: usize,
}

impl ConsentDialog {
    pub fn new(kind: ConsentKind) -> Self {
        Self { kind, selected: 0 }
    }
}
