# ADR-0005: BSP-tree pane layout

## Status

Accepted (2026-05-12; records the Sprint 1 decision retroactively)

## Context

Different terminal emulators take different approaches to pane splitting:

- **Uniform tiling**: split into an N×M grid via a GridLayout. Simple, but specifying an arbitrary ratio is hard.
- **BSP (Binary Space Partitioning)**: each split is a node in a binary tree. tmux, i3wm, Zellij, and others use this.
- **Floating**: each pane has a free position and size. Windows Terminal supports this partially.

### Requirements for Nexterm

- tmux-compatible keybindings for splitting panes (`Ctrl+B "`, etc.)
- Arbitrary nesting in either split direction (horizontal / vertical).
- Intuitive resize (adjacent panes move together).
- Sessions and templates must be persistable (the structure must be serialisable).

## Decision

**Adopt a BSP (Binary Space Partitioning) tree.**

### Data structure

```rust
enum SplitNode {
    Pane(u32),  // pane_id (leaf)
    Split {
        dir: SplitDir,    // Horizontal / Vertical
        ratio: f32,       // 0.0 to 1.0
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}
```

Each `Window` holds a single `SplitNode` root and expresses splits recursively.

### Core operations

- **Split**: replace an existing leaf with `Split { left: old, right: new }`.
- **Delete**: remove the leaf containing the pane and promote the sibling leaf up to where the parent `Split` was.
- **Resize**: change `Split.ratio` of the relevant node and recompute recursively.
- **Serialize**: round-trip the entire tree through JSON / postcard.

### The order of operations when adding a pane (important)

To avoid the chicken-and-egg problem, add panes in this order:

1. **Reserve the pane_id up front** (sequentially allocated by `SessionManager`).
2. **Insert into the tree** (no PTY yet).
3. **Recompute every pane's size** (compute the `PaneRect` of every leaf).
4. **Spawn the PTY** (now we can launch it with the correct cols/rows).
5. **Resize existing panes** (notify each PTY with the equivalent of `SIGWINCH`).

Getting this order wrong launches PTYs at the wrong size and later resizes fail to take effect.

## Consequences

### Positive

- Same mental model as tmux (existing users already know it).
- Arbitrary nested splits work naturally (a split can contain another split).
- The tree persists directly to JSON (basis for the template feature).
- Resize is local: only the affected region is recomputed.

### Negative

- The recursive data structure is harder to read than a flat array.
- Operations like "move a pane to a different window" require rebuilding the tree.
- Mixing with floating panes is handled separately by `FloatRect` (two representations in parallel).

## Alternatives

- **Alternative A: grid layout (N×M)** — simple, but arbitrary ratios and nesting are hard.
- **Alternative B: tiling array (Zellij-style)** — the flat representation is readable, but resize logic gets complex.
- **Alternative C: floating only** — maximum freedom, but loses tmux compatibility.

## References

- `nexterm-server/src/window/bsp.rs` — the BSP split algorithm (`PaneRect` / `SplitDir`)
- `nexterm-server/src/window/tiling.rs` — tiling layout logic
- `nexterm-server/src/window/floating.rs` — floating `FloatRect`
- `nexterm-server/src/window/tests.rs` — `bsp_split` layout unit tests
- tmux comparison: https://github.com/tmux/tmux/wiki
