# Web Terminal

See the [project README](https://github.com/mizu-jun/Nexterm) for a feature overview.

Nexterm includes an optional web terminal that exposes sessions through a browser via WebSocket and an embedded xterm.js front-end. This allows access to sessions from any device without installing the native client.

## Enabling the Web Terminal

Add the following to `nexterm.toml`:

```toml
[web]
enabled = true
bind = "127.0.0.1:7681"
```

Then navigate to `http://localhost:7681` in a browser to access the session list and open panes.

## Security Note

By default the web terminal binds to `127.0.0.1` only. If you expose it on a public interface, enable TLS and authentication. Refer to the [project README](https://github.com/mizu-jun/Nexterm) for TLS configuration details.
