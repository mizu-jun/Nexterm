# Serial Port

See the [project README](https://github.com/mizu-jun/Nexterm) for a feature overview.

Nexterm can connect to serial ports (e.g., `/dev/ttyUSB0`, `COM3`) directly as a pane, making it suitable for embedded development and hardware debugging without a separate terminal application.

## Connecting

Open the command palette and select **Connect Serial**, or trigger the `ConnectSerialPrompt` action. You will be prompted for:

1. **Port path** — e.g., `/dev/ttyUSB0` on Linux, `COM3` on Windows
2. **Baud rate** — e.g., `115200`

The connection opens in the focused pane and supports all standard terminal features (scrollback, copy/paste, quick select).

## Programmatic Connection

You can also connect via `nexterm-ctl`:

```sh
nexterm-ctl serial connect /dev/ttyUSB0 115200
```
