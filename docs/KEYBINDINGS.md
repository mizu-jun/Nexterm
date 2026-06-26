# Key Bindings (GPU client)

## General

| Key | Action |
|-----|--------|
| `Ctrl+,` | Open settings panel |
| `Ctrl+Shift+P` | Open / close command palette |
| `Ctrl+F` | Start scrollback search |
| `PageUp` | Scroll up in scrollback |
| `PageDown` | Scroll down in scrollback |
| `Escape` | Close search / palette |
| `Enter` (in search) | Jump to next match |
| `Ctrl+G` | Enter display-panes mode (show pane numbers) |
| `Ctrl+Shift+H` | Open SSH Host Manager |
| `Ctrl+Shift+M` | Open Lua Macro Picker |
| `Ctrl+Shift+U` | Open SFTP Upload dialog |
| `Ctrl+Shift+D` | Open SFTP Download dialog |
| `Ctrl+Shift+Space` | Enter Quick Select mode (URL / path / IP / hash) |
| `Ctrl+B Z` | Toggle zoom on focused pane |
| `Ctrl+B {` | Swap focused pane with previous |
| `Ctrl+B }` | Swap focused pane with next |
| `Ctrl+B !` | Break focused pane to new window |
| Regular key input | Forward to focused pane PTY |

## Font size

| Key | Action |
|-----|--------|
| `Ctrl+=` | Increase font size by 1 pt |
| `Ctrl+-` | Decrease font size by 1 pt |
| `Ctrl+0` | Reset font size to config value |

## Clipboard

| Key | Action |
|-----|--------|
| `Ctrl+Shift+C` | Copy: selected command block when one is active, otherwise the visible grid |
| `Ctrl+Shift+V` | Paste clipboard content into focused pane |

## Command Blocks (OSC 133)

Available when the shell emits OSC 133 prompt markers — see
[`shell-integration.md`](shell-integration.md) for the bash / zsh / fish
snippets. Disable the whole feature with `[blocks] enabled = false` in
`config.toml`.

| Key | Action |
|-----|--------|
| `Ctrl+Shift+ArrowUp` | Jump to previous prompt **and** select the matching block |
| `Ctrl+Shift+ArrowDown` | Jump to next prompt **and** select the matching block |
| `Ctrl+Shift+C` | Copy the selected block's full text (command + output) to the clipboard. Falls back to grid copy when no block is selected |
| `Ctrl+Shift+R` | Replay the selected block's command line through the focused pane (sanitised: rejects ESC / BEL / CSI / embedded newlines) |
| `Ctrl+Shift+L` | Open the block-name input modal for the selected block |
| `Ctrl+Shift+X` | Remove the user-assigned name from the selected block |
| `Ctrl+Shift+/` | Collapse or expand the selected block (running blocks are refused) |

### Block name modal

| Key | Action |
|-----|--------|
| `Enter` | Save the name and close (empty input removes the name) |
| `Escape` | Cancel and close without touching the persisted store |
| `Backspace` | Erase one character |
| printable characters | Append (up to 64 characters; ASCII controls are dropped) |

## Copy mode (Vim-style)

| Key | Action |
|-----|--------|
| `Ctrl+[` | Enter copy mode |
| `h` / `j` / `k` / `l` | Move cursor left / down / up / right |
| `w` | Move forward to start of next word |
| `b` | Move backward to start of previous word |
| `$` | Move to end of line |
| `0` | Move to beginning of line |
| `v` | Toggle selection start |
| `y` | Yank (copy) selection to clipboard and exit |
| `Y` | Yank entire current line to clipboard and exit |
| `/` | Enter incremental search mode |
| `n` | Jump to next search match |
| `q` / `Escape` | Exit copy mode |

## Mouse

| Action | Effect |
|--------|--------|
| Left click | Move focus to clicked pane / send mouse event (when mouse reporting active) |
| Left drag | Select text (blue highlight), auto-copy to clipboard on release |
| `Ctrl` + Left click | Open URL / OSC 8 hyperlink under cursor in browser |
| Right click | Show context menu (Copy/Paste/Split/Close) |
| Wheel up | Scroll up in scrollback (3 lines) |
| Wheel down | Scroll down in scrollback (3 lines) |

## Display Panes mode

| Key | Action |
|-----|--------|
| Digit key (0-9) | Jump to pane with that number |
| Arrow keys | Navigate between panes (preview mode) |
| `Enter` | Confirm pane selection |
| `Escape` | Exit display-panes mode |

## Pane operations (via server protocol)

| Message | Action |
|---------|--------|
| `SplitVertical` | Split focused pane left/right |
| `SplitHorizontal` | Split focused pane top/bottom |
| `FocusNextPane` | Move focus to next pane |
| `FocusPrevPane` | Move focus to previous pane |
| `ClosePane` | Close focused pane (sibling promoted) |
| `ResizeSplit { delta: f32 }` | Adjust focused split ratio |
| `NewWindow` | Create a new window (tab) |
| `CloseWindow { window_id }` | Close specified window |
| `FocusWindow { window_id }` | Switch to specified window |
| `RenameWindow { window_id, name }` | Rename specified window |
| `SetBroadcast { enabled: bool }` | Toggle broadcast input mode |
| `ConnectSsh { host, port, username, auth_type, ... }` | Open SSH connection in new pane |
| `ToggleZoom` | Toggle zoom on focused pane |
| `SwapPaneNext` | Swap focused pane with next sibling |
| `SwapPanePrev` | Swap focused pane with previous sibling |
| `BreakPane` | Move focused pane to a new window |
| `ConnectSerial { path, baud }` | Open serial port in new pane |
