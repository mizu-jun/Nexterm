//! VT100 / VT220 / xterm compliance tests (QA persona: 仕様懐疑者).
//!
//! Each test names the escape sequence under test and cites the relevant
//! specification clause so that future readers can audit interpretation
//! against primary sources (see `docs/TESTING_STRATEGY.md` §3).
//!
//! References:
//! - ECMA-48 (CSI cursor motion, ED, EL).
//! - DEC VT220 Programmer Reference (EK-VT220-RM): DECSTBM, DECOM.
//! - xterm `ctlseqs.txt`: DEC private modes 47 / 1047 / 1049 (alternate
//!   screen) and 25 (cursor visibility).
//!
//! These tests exercise observable behaviour through the public VtParser API
//! only; they intentionally do not poke `pub(crate)` internals.
//!
//! NOTE: A handful of tests are marked `#[ignore]` when an underlying
//! sequence is recognised but lacks a public observable side-effect today.
//! They document the gap rather than asserting unverified behaviour, per
//! the strategy doc's "推測は #[ignore] でマーク" rule.

use nexterm_vt::VtParser;

// ---------------------------------------------------------------------------
// CSI cursor motion (ECMA-48)
// ---------------------------------------------------------------------------

#[test]
fn cup_at_origin_with_no_params_moves_to_1_1() {
    // ECMA-48 CSI H with no parameters defaults to row 1 column 1 (1-based).
    let mut parser = VtParser::new(80, 24);
    // Move away first, then send bare CSI H.
    parser.advance(b"\x1b[10;10H");
    parser.advance(b"\x1b[HX");
    let grid = parser.screen().grid();
    assert_eq!(
        grid.get(0, 0).unwrap().ch,
        'X',
        "CSI H with no params must home the cursor"
    );
}

#[test]
fn cup_clamps_coordinates_past_screen_bounds() {
    // Out-of-range CUP coordinates must be clamped, not panic.
    let mut parser = VtParser::new(10, 5);
    parser.advance(b"\x1b[100;200HZ");
    let grid = parser.screen().grid();
    // The last cell of the screen is (9, 4); the glyph must land at or
    // before that point (implementations may clamp to either edge).
    let last_row = grid.get(9, 4).unwrap().ch;
    assert_eq!(
        last_row, 'Z',
        "clamped CUP must land at the bottom-right cell"
    );
}

// ---------------------------------------------------------------------------
// ED — Erase in Display (ECMA-48 CSI Ps J)
// ---------------------------------------------------------------------------

#[test]
fn ed_with_param_2_clears_entire_display() {
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"AAAAA\r\nBBBBB\r\nCCCCC");
    parser.advance(b"\x1b[2J");
    let grid = parser.screen().grid();
    for row in 0..3 {
        for col in 0..10 {
            assert_eq!(
                grid.get(col, row).unwrap().ch,
                ' ',
                "CSI 2J must clear cell ({col},{row})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// DEC private modes — Alternate screen buffer
// ---------------------------------------------------------------------------

#[test]
fn dec_1049_switch_to_alt_screen_hides_primary_content() {
    // CSI ?1049h saves the cursor, switches to the alternate screen, and
    // clears it (xterm ctlseqs.txt).
    let mut parser = VtParser::new(20, 5);
    parser.advance(b"primary-content");
    parser.advance(b"\x1b[?1049h");
    let grid = parser.screen().grid();
    // The alternate screen must start blank.
    assert_eq!(
        grid.get(0, 0).unwrap().ch,
        ' ',
        "alt screen entry must clear cell (0,0)"
    );
}

#[test]
fn dec_1049_exit_alt_screen_restores_primary_content() {
    // After CSI ?1049l the primary screen content must be visible again.
    let mut parser = VtParser::new(20, 5);
    parser.advance(b"primary");
    parser.advance(b"\x1b[?1049h");
    parser.advance(b"alt-screen-text");
    parser.advance(b"\x1b[?1049l");
    let grid = parser.screen().grid();
    let first_row: String = (0..7).map(|c| grid.get(c, 0).unwrap().ch).collect();
    assert_eq!(
        first_row, "primary",
        "alt screen exit must restore primary content"
    );
}

#[test]
fn dec_1047_switch_to_alt_screen_clears_target() {
    // DECSET 1047 enters the alt screen without saving the cursor.
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"keep");
    parser.advance(b"\x1b[?1047h");
    let grid = parser.screen().grid();
    assert_eq!(grid.get(0, 0).unwrap().ch, ' ');
}

// ---------------------------------------------------------------------------
// DECSTBM — Set Top and Bottom Margins (DEC VT220, CSI Ps;Ps r)
// ---------------------------------------------------------------------------

#[test]
fn decstbm_moves_cursor_to_home_after_setting_region() {
    // DECSTBM unconditionally homes the cursor (VT220 spec). Verifying via
    // the next written character at (0,0).
    let mut parser = VtParser::new(20, 10);
    parser.advance(b"\x1b[5;10H"); // move away
    parser.advance(b"\x1b[3;7r"); // DECSTBM rows 3..7
    parser.advance(b"X"); // write a character at the new home
    let grid = parser.screen().grid();
    assert_eq!(
        grid.get(0, 0).unwrap().ch,
        'X',
        "DECSTBM must move the cursor to the home position"
    );
}

#[test]
fn decstbm_out_of_range_does_not_panic() {
    // Bogus parameters must be clamped, not crash.
    let mut parser = VtParser::new(10, 5);
    parser.advance(b"\x1b[100;200r");
    parser.advance(b"OK");
    // No panic = pass; also confirm the write still lands somewhere.
    let grid = parser.screen().grid();
    let any_o = (0..10).any(|c| grid.get(c, 0).unwrap().ch == 'O');
    assert!(
        any_o,
        "characters must still be writable after invalid DECSTBM"
    );
}

// ---------------------------------------------------------------------------
// CSI ? 25 h/l — show / hide cursor (xterm)
// ---------------------------------------------------------------------------

#[test]
fn dec_25_does_not_corrupt_subsequent_writes() {
    // Even when the cursor is hidden, writes must continue normally.
    // (We do not assert the visibility flag here because it is not part of
    // the public Screen API; this is the regression-safety contract.)
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"\x1b[?25l");
    parser.advance(b"Hello");
    parser.advance(b"\x1b[?25h");
    let grid = parser.screen().grid();
    assert_eq!(grid.get(0, 0).unwrap().ch, 'H');
    assert_eq!(grid.get(4, 0).unwrap().ch, 'o');
}

// ---------------------------------------------------------------------------
// SGR reset (CSI 0 m) — sanity check on attribute restoration
// ---------------------------------------------------------------------------

#[test]
fn sgr_reset_restores_default_attrs() {
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"\x1b[31mR\x1b[0mN");
    let grid = parser.screen().grid();
    let red = grid.get(0, 0).unwrap();
    let normal = grid.get(1, 0).unwrap();
    // After CSI 0m the cell must not retain the foreground colour of 'R'.
    assert_ne!(red.fg, normal.fg, "SGR 0 must reset the foreground colour");
}

// ---------------------------------------------------------------------------
// Malformed sequences must not panic (defence-in-depth alongside the fuzzer)
// ---------------------------------------------------------------------------

#[test]
fn unterminated_csi_does_not_crash() {
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"\x1b[999999999999");
    parser.advance(b"X");
    // No panic = pass.
}

#[test]
fn negative_looking_params_are_ignored_gracefully() {
    // ECMA-48 forbids negative numerics in CSI; non-digit bytes terminate
    // the parameter run. The parser must not interpret them as negative.
    let mut parser = VtParser::new(10, 3);
    parser.advance(b"\x1b[-1;-1H");
    parser.advance(b"Q");
    // No panic = pass; the write also lands somewhere on the screen.
    let grid = parser.screen().grid();
    let any_q = (0..3)
        .flat_map(|r| (0..10).map(move |c| (c, r)))
        .any(|(c, r)| grid.get(c, r).unwrap().ch == 'Q');
    assert!(any_q);
}
