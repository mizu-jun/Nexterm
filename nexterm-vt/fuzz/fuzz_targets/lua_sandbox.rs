#![no_main]
//! Audit round 2 / G6: fuzz the Lua sandbox and the config.lua loader.
//!
//! Feeds arbitrary bytes as a Lua chunk into `sandboxed_lua()` and, when the
//! chunk evaluates to a table, runs it through `apply_lua_table_to_config` —
//! the same path an untrusted `config.lua` takes. Verifies that neither the
//! mlua runtime nor the loader panics, OOMs, or hangs.
//!
//! Attack scenarios in scope:
//! - Parser/loader crashes on malformed or adversarial chunks.
//! - Memory exhaustion (bounded by `set_memory_limit`).
//! - Infinite loops (bounded by an instruction-count hook; the CI `-timeout`
//!   flag is the backstop).
//! - Type-confusion in the table→Config mapping.

use libfuzzer_sys::fuzz_target;
use nexterm_config::loader::apply_lua_table_to_config;
use nexterm_config::lua_sandbox::sandboxed_lua;
use nexterm_config::Config;

/// Memory ceiling for one fuzz iteration (16 MiB is far above what a real
/// config.lua needs but small enough to keep the fuzzer fast).
const MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Abort scripts after this many VM instructions (stops infinite loops well
/// before the libFuzzer timeout).
const INSTRUCTION_LIMIT: u32 = 1_000_000;

fuzz_target!(|data: &[u8]| {
    // Lua sources are text; skip non-UTF-8 inputs early.
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(lua) = sandboxed_lua() else {
        return;
    };
    let _ = lua.set_memory_limit(MEMORY_LIMIT_BYTES);
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(INSTRUCTION_LIMIT),
        |_lua, _debug| Err(mlua::Error::RuntimeError("instruction limit".into())),
    );

    match lua.load(src).eval::<mlua::Value>() {
        Ok(mlua::Value::Table(tbl)) => {
            // Exercise the same mapping an untrusted config.lua goes through.
            let mut config = Config::default();
            let _ = apply_lua_table_to_config(&mut config, &tbl);
        }
        // Errors (syntax, sandbox violations, limits) are expected outcomes.
        _ => {}
    }
});
