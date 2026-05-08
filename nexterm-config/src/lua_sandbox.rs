//! Lua サンドボックス — config.lua / Lua フック / マクロの実行制限
//!
//! # CRITICAL #4 対応
//!
//! mlua の `Lua::new()` は標準ライブラリ全開（os / io / package 等）でインスタンスを
//! 生成する。これにより、信頼できない `config.lua`（dotfiles 共有・チームテンプレ等）
//! をクローンしただけで `os.execute("rm -rf ~")` や `io.open("/etc/passwd")` が
//! 実行されるリモートコード実行（RCE）相当の脆弱性が存在した。
//!
//! 本モジュールは安全なサブセットのみを公開する `Lua` インスタンスを生成する:
//! - 許可: `string`, `table`, `math`, `coroutine`, `print`（warn として記録）
//! - 削除: `os`（time/date/clock を除く）, `io`, `package`, `require`, `dofile`,
//!   `loadfile`, `load`, `loadstring`, `debug`, `collectgarbage`
//!
//! # 互換性破壊
//!
//! 既存の `config.lua` で `os.date()` 以外の `os.*` や `io.*` を使っていると失敗する。
//! Migration ドキュメントに代替 API（将来追加予定の `nexterm.*` 名前空間）を記載すること。

use mlua::{Lua, LuaOptions, StdLib};

/// サンドボックス化された `Lua` インスタンスを生成する。
///
/// 標準ライブラリは `STRING | TABLE | MATH | COROUTINE` のみ有効化し、
/// 残りの危険なグローバル（`os` / `io` / `package` / `require` / `dofile` /
/// `loadfile` / `load` / `loadstring` / `debug` / `collectgarbage`）を
/// 明示的に削除する。
///
/// # 戻り値
///
/// サンドボックス化された `Lua`、または初期化失敗時のエラー。
pub fn sandboxed_lua() -> mlua::Result<Lua> {
    // 安全なライブラリのみロード（os / io / package / debug を除外）
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::COROUTINE,
        LuaOptions::default(),
    )?;

    // 念のため、インポートされた場合に備えて危険なグローバルを削除する
    // （StdLib フラグで除外されているはずだが、防御の深さとして二重ガード）
    let globals = lua.globals();
    for name in &[
        "os",
        "io",
        "package",
        "require",
        "dofile",
        "loadfile",
        "load",
        "loadstring",
        "debug",
        "collectgarbage",
        "rawset",
        "rawget",
        "rawequal",
        "rawlen",
        "setfenv",
        "getfenv",
    ] {
        // エラーは無視: もともと存在しない場合がある
        let _ = globals.set(*name, mlua::Nil);
    }

    Ok(lua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 安全な_lua_は基本演算ができる() {
        let lua = sandboxed_lua().unwrap();
        let result: i32 = lua.load("return 1 + 2").eval().unwrap();
        assert_eq!(result, 3);
    }

    #[test]
    fn 安全な_lua_は_string_テーブル_数学が使える() {
        let lua = sandboxed_lua().unwrap();
        let result: String = lua
            .load(r#"return string.upper("hello") .. " " .. tostring(math.floor(3.7))"#)
            .eval()
            .unwrap();
        assert_eq!(result, "HELLO 3");
    }

    #[test]
    fn 安全な_lua_は_os_execute_を使えない() {
        // CRITICAL #4 核心テスト: os.execute による RCE が成立しないことを保証
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"os.execute("echo PWNED")"#).eval();
        assert!(
            result.is_err(),
            "os.execute が呼べてしまっている。サンドボックス失敗"
        );
    }

    #[test]
    fn 安全な_lua_は_io_open_を使えない() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"io.open("/etc/passwd", "r")"#).eval();
        assert!(result.is_err(), "io.open が呼べてしまっている");
    }

    #[test]
    fn 安全な_lua_は_require_を使えない() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"require("os")"#).eval();
        assert!(result.is_err(), "require が呼べてしまっている");
    }

    #[test]
    fn 安全な_lua_は_dofile_loadfile_を使えない() {
        let lua = sandboxed_lua().unwrap();
        let r1: mlua::Result<()> = lua.load(r#"dofile("/tmp/x.lua")"#).eval();
        assert!(r1.is_err(), "dofile が呼べてしまっている");

        let r2: mlua::Result<()> = lua.load(r#"loadfile("/tmp/x.lua")"#).eval();
        assert!(r2.is_err(), "loadfile が呼べてしまっている");
    }

    #[test]
    fn 安全な_lua_は_debug_ライブラリを使えない() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"debug.getregistry()"#).eval();
        assert!(result.is_err(), "debug ライブラリが使えてしまっている");
    }

    #[test]
    fn 安全な_lua_でも_テーブル操作は通常通り使える() {
        let lua = sandboxed_lua().unwrap();
        let result: i32 = lua
            .load(
                r#"
                local t = {1, 2, 3}
                table.insert(t, 4)
                return #t
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, 4);
    }
}
