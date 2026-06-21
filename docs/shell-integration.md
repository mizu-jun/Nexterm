# Shell Integration (OSC 133)

Nexterm's command-blocks feature (see [`CONFIGURATION.md`](CONFIGURATION.md#blocks))
relies on **OSC 133**, the FinalTerm prompt-marker protocol that
WezTerm, kitty, Ghostty, and several other terminals share. The terminal
itself only **records** the four marker positions; the shell is
responsible for **emitting** them. This document collects the snippets
required to wire up bash, zsh, and fish.

If your shell does not emit OSC 133, the renderer simply has no blocks
to draw and the keybindings (`Ctrl+Shift+ArrowUp/Down`, `Ctrl+Shift+R`,
`Ctrl+Shift+L`, the block-aware `Ctrl+Shift+C`) become no-ops.

## The four markers

| Sequence | Meaning |
|----------|---------|
| `OSC 133 ; A ST` | **Prompt start** — the shell is about to draw the prompt |
| `OSC 133 ; B ST` | **Command start** — the user input position (between prompt and command) |
| `OSC 133 ; C ST` | **Output start** — the command has been accepted and execution is beginning |
| `OSC 133 ; D ; <exit_code> ST` | **Command end** — execution finished, with the exit code |

`ST` is either `BEL` (`\x07`) or `ESC \` (`\x1b\\`). Both terminations
are accepted by Nexterm.

## bash

Add the following to `~/.bashrc`:

```bash
# OSC 133 markers for Nexterm / WezTerm / kitty / Ghostty.
__nexterm_osc133_prompt_start() {
    printf '\e]133;A\e\\'
}
__nexterm_osc133_command_end() {
    local code=$?
    printf '\e]133;D;%s\e\\' "$code"
    return $code
}
# Wrap PS1 so the A marker fires before the prompt and a B marker fires
# at the start of user input.
PS1='$(__nexterm_osc133_prompt_start)'"$PS1"$'\e]133;B\e\\'
# PROMPT_COMMAND fires after each command; emit D with the exit code.
PROMPT_COMMAND="__nexterm_osc133_command_end${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
# C (OutputStart) is best emitted via DEBUG trap.
trap 'printf "\e]133;C\e\\"' DEBUG
```

Caveats:

- If you use Starship, oh-my-bash, or another prompt framework, paste
  the snippet **after** the framework's own `PS1` setup so its prompt is
  wrapped instead of replaced.
- `DEBUG` traps interact with other tools (e.g. `direnv`); guard with
  `[[ "$BASH_COMMAND" != "trap"* ]]` if you hit recursion.

## zsh

Add the following to `~/.zshrc`:

```zsh
# OSC 133 markers for Nexterm / WezTerm / kitty / Ghostty.
__nexterm_osc133_prompt_start() {
    print -n '\e]133;A\e\\'
}
__nexterm_osc133_command_end() {
    local code=$?
    print -n "\e]133;D;${code}\e\\"
    return $code
}
__nexterm_osc133_output_start() {
    print -n '\e]133;C\e\\'
}
# precmd fires before the prompt is rendered; emit A.
autoload -Uz add-zsh-hook
add-zsh-hook precmd __nexterm_osc133_prompt_start
# preexec fires when a command is accepted but before it runs; emit C.
add-zsh-hook preexec __nexterm_osc133_output_start
# precmd of the next iteration is the natural place for D, but for
# accurate $? we wire D into a TRAPEXIT-style hook via precmd instead.
add-zsh-hook precmd __nexterm_osc133_command_end
# B is harder to place precisely in zsh; appending it to PS1 is the
# pragmatic choice and matches what kitty's own shell integration does.
PS1="%{"$'\e]133;B\e\\'"%}$PS1"
```

The `%{...%}` braces prevent zsh from counting the marker towards line
length.

## fish

Add the following to `~/.config/fish/config.fish`:

```fish
# OSC 133 markers for Nexterm / WezTerm / kitty / Ghostty.
function __nexterm_osc133_prompt_start --on-event fish_prompt
    printf '\e]133;A\e\\'
end
function __nexterm_osc133_output_start --on-event fish_preexec
    printf '\e]133;C\e\\'
end
function __nexterm_osc133_command_end --on-event fish_postexec
    set -l code $status
    printf '\e]133;D;%s\e\\' $code
end
# B is woven into the prompt via fish_prompt.
function fish_prompt
    # ... your existing fish_prompt body ...
    printf '\e]133;B\e\\'
end
```

If you customise `fish_prompt` heavily, append the `B` marker at the
**end** of your existing function rather than replacing it.

## PowerShell

PowerShell is supported but the integration is less established than on
POSIX shells. The Phase 2 cut of the command-blocks feature targets
bash / zsh / fish first; PowerShell support will be revisited once the
overlay renderer is verified on Windows.

## Verifying the integration

After updating your shell config, restart Nexterm and run a few
commands. With the default `[blocks] enabled = true` you should see:

- A coloured left border appear in the scrollback area (use the
  scrollback search or `PageUp` to scroll up) for each finished command.
- Green border for `exit code == 0`, red for non-zero, grey for the
  currently-running block.
- `Ctrl+Shift+ArrowUp` / `ArrowDown` should both scroll between prompts
  *and* highlight the matching block.

If nothing appears:

1. Confirm OSC 133 is reaching Nexterm: `printf '\e]133;A\e\\'` from a
   shell prompt should silently record a mark. Toggle a verbose log
   level (`NEXTERM_LOG=debug nexterm`) to inspect.
2. Confirm `[blocks] enabled = true` in `config.toml` (this is the
   default; explicit `false` disables the overlay regardless of OSC 133).
3. Scroll up — the current implementation only paints the overlay while
   the pane is in scrollback mode. The in-grid path lands in a later
   iteration.

## Why we don't auto-install

Wrapping a user's `PS1` (or fish_prompt) without consent breaks too many
existing setups — prompt frameworks, custom traps, multi-line prompts,
etc. Nexterm therefore ships the integration as a documented opt-in
rather than installing it on first launch.
