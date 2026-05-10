//! Sprint 4-4: Sixel / Kitty パーサのプロパティベーステスト
//!
//! `cargo-fuzz`（Sprint 3-5）と相補的に、proptest で**意味的不変条件**を検証する:
//!
//! - 任意のバイト列でパニックしないこと
//! - デコード成功時、`rgba.len() == width * height * 4` が常に成立すること
//! - 巨大寸法を主張するペイロードはデコードを拒否すること（`MAX_IMAGE_BYTES` 16 MB 上限）
//! - VtParser に渡す経路でも同様の不変条件を維持すること
//!
//! cargo-fuzz は実行時間内に**生のクラッシュ**を探すのに対し、proptest は
//! **特定の事後条件**（dimensions と buffer サイズの一致、APC 終端処理等）を
//! 縮小付きで検証する。両者は相補的に機能する。

use nexterm_vt::VtParser;
use nexterm_vt::image::{decode_kitty, decode_sixel};
use proptest::prelude::*;

// ---- ヘルパー ----

/// proptest のデフォルト 256 ケースは Sixel パーサ全長探索には充分。
/// 1 ケースあたり最大数 KB を扱うが各テストはサンプリングで完結する。
fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 256,
        // パニックは即時 fail にしたいので max_local_rejects は控えめ
        max_local_rejects: 1024,
        ..ProptestConfig::default()
    }
}

// ---- decode_sixel: 任意バイト列 ----

proptest! {
    #![proptest_config(config())]

    /// 任意のバイト列で `decode_sixel` がパニックしないこと
    #[test]
    fn decode_sixel_never_panics(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decode_sixel(&input);
    }

    /// 成功時、`rgba.len()` が `width * height * 4` と一致すること
    #[test]
    fn decode_sixel_buffer_size_matches_dimensions(
        input in proptest::collection::vec(any::<u8>(), 0..4096)
    ) {
        if let Some(img) = decode_sixel(&input) {
            let expected = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), expected);
            // チャンネル単位（4 の倍数）であること
            prop_assert_eq!(img.rgba.len() % 4, 0);
        }
    }
}

// ---- decode_sixel: 構造化生成 ----

/// Sixel パーサで意味のあるバイトのみから生成する（性能のため）。
/// `?`〜`~` のピクセルデータ、`#`カラー命令、`-`改行、`$`復帰、`!`リピートを混在させる。
fn arb_sixel_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![
            // ピクセル文字（0x3F..=0x7E）
            (0x3Fu8..=0x7E).prop_map(|b| vec![b]),
            // 改行 / 復帰 / リピート / カラー
            Just(b"-".to_vec()),
            Just(b"$".to_vec()),
            // !<n><pixel>
            (1u32..=99, 0x3Fu8..=0x7E)
                .prop_map(|(n, ch)| format!("!{}{}", n, ch as char).into_bytes()),
            // #<n>;2;<r>;<g>;<b>
            (0u16..=255, 0u16..=100, 0u16..=100, 0u16..=100).prop_map(|(idx, r, g, b)| format!(
                "#{};2;{};{};{}",
                idx, r, g, b
            )
            .into_bytes()),
        ],
        0..256,
    )
    .prop_map(|chunks| chunks.into_iter().flatten().collect())
}

proptest! {
    #![proptest_config(config())]

    /// 構造化された Sixel データでもパニックせず、dimensions が一貫すること
    #[test]
    fn decode_sixel_structured_invariants(input in arb_sixel_bytes()) {
        if let Some(img) = decode_sixel(&input) {
            let total = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), total);
            // Sixel は 6 行単位なので height は常に 6 の倍数
            prop_assert_eq!(img.height % 6, 0);
            // width / height は 0 にならない（早期 None で弾かれる）
            prop_assert!(img.width > 0);
            prop_assert!(img.height > 0);
        }
    }
}

// ---- decode_kitty: 任意バイト列 ----

proptest! {
    #![proptest_config(config())]

    /// 任意のバイト列で `decode_kitty` がパニックしないこと
    #[test]
    fn decode_kitty_never_panics(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decode_kitty(&input);
    }

    /// 成功時、`rgba.len()` が `width * height * 4` と一致すること
    #[test]
    fn decode_kitty_buffer_size_matches_dimensions(
        input in proptest::collection::vec(any::<u8>(), 0..4096)
    ) {
        if let Some(img) = decode_kitty(&input) {
            let expected = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), expected);
            prop_assert_eq!(img.rgba.len() % 4, 0);
            prop_assert!(img.width > 0);
            prop_assert!(img.height > 0);
        }
    }
}

// ---- decode_kitty: 構造化生成 ----

/// 妥当な Kitty APC を構造的に生成する（パニック耐性 + 寸法検証両方をカバー）。
fn arb_kitty_apc() -> impl Strategy<Value = Vec<u8>> {
    (
        prop_oneof![Just(32u32), Just(24u32), 0..255u32], // f= フォーマット
        0..2048u32,                                       // s= 幅
        0..2048u32,                                       // v= 高さ
        proptest::collection::vec(any::<u8>(), 0..4096),  // ペイロード
    )
        .prop_map(|(f, s, v, payload)| {
            // base64 エンコードは省略してナマバイトでテスト
            // → decode_kitty は base64_decode に失敗して None を返すケースを多数生成する
            let mut out = format!("Ga=T,f={},s={},v={};", f, s, v).into_bytes();
            out.extend_from_slice(&payload);
            out
        })
}

proptest! {
    #![proptest_config(config())]

    /// 構造化 APC でも事後条件（dimensions と buffer 一致）が成立すること
    #[test]
    fn decode_kitty_structured_invariants(input in arb_kitty_apc()) {
        if let Some(img) = decode_kitty(&input) {
            let total = (img.width as u64)
                .checked_mul(img.height as u64)
                .and_then(|v| v.checked_mul(4))
                .map(|v| v as usize);
            prop_assert_eq!(Some(img.rgba.len()), total);
            prop_assert_eq!(img.rgba.len() % 4, 0);
        }
    }

    /// 巨大寸法（16 MB ピクセル超え）の APC は必ず None を返すこと
    /// `MAX_IMAGE_BYTES = 256 MiB`（実装側参照）。height=8193, width=8192, RGBA=4 → 256 MiB 超
    #[test]
    fn decode_kitty_oversized_dimensions_rejected(
        s in 8193u32..=u16::MAX as u32,
        v in 8192u32..=u16::MAX as u32,
    ) {
        // ペイロードは空でよい（寸法チェックの方が先行）
        let apc = format!("Ga=T,f=32,s={},v={};", s, v).into_bytes();
        let result = decode_kitty(&apc);
        prop_assert!(
            result.is_none(),
            "巨大寸法 ({}x{}) は拒否されるべき",
            s, v
        );
    }
}

// ---- VtParser 経由のパニック耐性 ----

proptest! {
    #![proptest_config(ProptestConfig {
        // VtParser はバイトごとに状態遷移するため少し重い。ケース数を絞る。
        cases: 64,
        ..config()
    })]

    /// VtParser が任意のバイト列でパニックしないこと
    /// （cargo-fuzz と相補的: proptest は CI で常時走らせて回帰検出）
    #[test]
    fn vt_parser_never_panics(input in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut parser = VtParser::new(80, 24);
        parser.advance(&input);
        // 内部状態を覗いてもパニックしないこと
        let _ = parser.bracketed_paste_mode();
        let _ = parser.synchronized_output_mode();
    }

    /// APC（Kitty グラフィックス）シーケンスを含む入力でもパニックしないこと
    #[test]
    fn vt_parser_apc_robust(
        prefix in proptest::collection::vec(any::<u8>(), 0..256),
        apc_body in proptest::collection::vec(any::<u8>(), 0..2048),
        suffix in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let mut input = prefix;
        input.extend_from_slice(b"\x1b_");
        input.extend_from_slice(&apc_body);
        input.extend_from_slice(b"\x1b\\");
        input.extend_from_slice(&suffix);
        let mut parser = VtParser::new(80, 24);
        parser.advance(&input);
    }
}
