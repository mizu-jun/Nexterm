# Session Management

See the [project README](https://github.com/mizu-jun/Nexterm) and [Architecture Overview](../architecture.md) for design details.

Nexterm uses a client-server architecture where the server process owns all sessions and PTYs. Clients connect over a local Unix socket (Linux/macOS) or named pipe (Windows) and can detach and re-attach without losing session state.

## Key Concepts

- **Sessions** — named workspaces that persist independently of any connected client
- **Panes** — individual PTY instances within a session, arranged in a tree layout
- **Attach / Detach** — connect to or disconnect from a session without terminating it

## Managing Sessions with nexterm-ctl

```sh
nexterm-ctl session list           # list all sessions
nexterm-ctl session new my-work    # create a named session
nexterm-ctl session attach my-work # attach to an existing session
nexterm-ctl session kill my-work   # terminate a session
```

## Pane Layout

Panes can be split vertically or horizontally using key bindings or the command palette. The layout is stored server-side and restored when a client re-attaches.
