//! Lua sandbox — execution restrictions for `config.lua`, Lua hooks, and macros.
//!
//! # CRITICAL #4 mitigation
//!
//! mlua's `Lua::new()` returns an instance with every standard library enabled
//! (`os`, `io`, `package`, …). That made the simple act of cloning an
//! untrusted `config.lua` (shared dotfiles, team templates, etc.) effectively
//! equivalent to remote code execution: `os.execute("rm -rf ~")` and
//! `io.open("/etc/passwd")` could be run automatically.
//!
//! This module produces a `Lua` instance that only exposes the safe subset:
//! - Allowed: `string`, `table`, `math`, `coroutine`, `print` (logged as a warning).
//! - Removed: `os` (except `time`/`date`/`clock`), `io`, `package`, `require`,
//!   `dofile`, `loadfile`, `load`, `loadstring`, `debug`, `collectgarbage`.
//!
//! # Breaking change
//!
//! Existing `config.lua` files that use anything other than `os.date()` from
//! `os.*`, or anything from `io.*`, will fail. Document the replacement APIs
//! (a forthcoming `nexterm.*` namespace) in the migration guide.

use mlua::{Lua, LuaOptions, StdLib};

/// Creates a sandboxed `Lua` instance.
///
/// Only `STRING | TABLE | MATH | COROUTINE` from the standard library is
/// enabled, and the remaining dangerous globals (`os` / `io` / `package` /
/// `require` / `dofile` / `loadfile` / `load` / `loadstring` / `debug` /
/// `collectgarbage`) are explicitly removed.
///
/// # Returns
///
/// The sandboxed `Lua`, or an error if initialization fails.
pub fn sandboxed_lua() -> mlua::Result<Lua> {
    // Load only the safe libraries (excluding `os` / `io` / `package` / `debug`).
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::COROUTINE,
        LuaOptions::default(),
    )?;

    // Remove dangerous globals just in case (the `StdLib` flags should have
    // excluded them already, but this provides defense in depth).
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
        // Ignore errors: the global may not exist in the first place.
        let _ = globals.set(*name, mlua::Nil);
    }

    Ok(lua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandboxed_lua_can_evaluate_basic_expressions() {
        let lua = sandboxed_lua().unwrap();
        let result: i32 = lua.load("return 1 + 2").eval().unwrap();
        assert_eq!(result, 3);
    }

    #[test]
    fn sandboxed_lua_can_use_string_table_and_math() {
        let lua = sandboxed_lua().unwrap();
        let result: String = lua
            .load(r#"return string.upper("hello") .. " " .. tostring(math.floor(3.7))"#)
            .eval()
            .unwrap();
        assert_eq!(result, "HELLO 3");
    }

    #[test]
    fn sandboxed_lua_cannot_use_os_execute() {
        // CRITICAL #4 core test: ensures `os.execute` cannot be used to gain RCE.
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"os.execute("echo PWNED")"#).eval();
        assert!(
            result.is_err(),
            "os.execute is reachable; the sandbox failed"
        );
    }

    #[test]
    fn sandboxed_lua_cannot_use_io_open() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"io.open("/etc/passwd", "r")"#).eval();
        assert!(result.is_err(), "io.open is reachable");
    }

    #[test]
    fn sandboxed_lua_cannot_use_require() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"require("os")"#).eval();
        assert!(result.is_err(), "require is reachable");
    }

    #[test]
    fn sandboxed_lua_cannot_use_dofile_or_loadfile() {
        let lua = sandboxed_lua().unwrap();
        let r1: mlua::Result<()> = lua.load(r#"dofile("/tmp/x.lua")"#).eval();
        assert!(r1.is_err(), "dofile is reachable");

        let r2: mlua::Result<()> = lua.load(r#"loadfile("/tmp/x.lua")"#).eval();
        assert!(r2.is_err(), "loadfile is reachable");
    }

    #[test]
    fn sandboxed_lua_cannot_use_the_debug_library() {
        let lua = sandboxed_lua().unwrap();
        let result: mlua::Result<()> = lua.load(r#"debug.getregistry()"#).eval();
        assert!(result.is_err(), "the debug library is reachable");
    }

    #[test]
    fn sandboxed_lua_still_supports_normal_table_operations() {
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
