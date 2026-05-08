# SFTP File Transfer

See the [project README](https://github.com/mizu-jun/Nexterm) for a feature overview.

Nexterm provides an integrated SFTP client that reuses the active SSH connection to an existing pane, so no second connection or separate tool is required.

## Uploading Files

Open the SFTP upload dialog from the command palette (`SftpUploadDialog`) or the key binding. Select local files; they are uploaded to the remote working directory of the focused SSH pane.

## Downloading Files

Open the SFTP download dialog (`SftpDownloadDialog`), browse the remote filesystem, and select files to download to a local path.

## Key Bindings (default)

| Key | Action |
|-----|--------|
| `Ctrl+Shift+U` | Open SFTP upload dialog |
| `Ctrl+Shift+D` | Open SFTP download dialog |

Both actions can also be triggered from the right-click context menu inside any SSH pane.
