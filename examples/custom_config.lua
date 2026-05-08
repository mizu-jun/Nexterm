-- nexterm custom configuration example (Lua)
-- Place this file at ~/.config/nexterm/config.lua

-- Font settings
font = {
    family = "JetBrains Mono",
    size = 14.0,
}

-- Color scheme (Catppuccin Mocha)
colors = {
    background = "#1e1e2e",
    foreground = "#cdd6f4",
    cursor     = "#f5e0dc",
    ansi = {
        "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
        "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
        "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
        "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
    },
}

-- Status bar: show current time
status_bar = {
    enabled = true,
    left  = " nexterm ",
    right = os.date(" %H:%M "),
}

-- Key bindings (merged with defaults)
keybindings = {
    { key = "ctrl+shift+t", action = "NewWindow" },
}
