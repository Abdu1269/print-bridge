# Deployment

<div align="right">

[中文](./deployment.md)

</div>

## Desktop

Windows, macOS, and Linux GUI installers provide the `print-bridge-desktop` package and the `print-bridge` executable. Running without arguments starts the GUI. The desktop product does not support the `serve` command.

## Linux Headless

The headless deb/rpm package is named `print-bridge-server`. Upon installation, it automatically:

1. Creates a `printbridge` system user and group (without a home directory);
2. Sets up `/etc/print-bridge`, `/var/lib/print-bridge`, and `/run/print-bridge`;
3. Installs and enables `print-bridge.service`;
4. Runs `/usr/bin/print-bridge serve` as the `printbridge` user.

```bash
sudo apt install ./print-bridge-server_VERSION_ARCH.deb
systemctl status print-bridge
journalctl -u print-bridge -f
```

For manual diagnostics:

```bash
sudo -u printbridge /usr/bin/print-bridge serve
print-bridge status
print-bridge doctor
```

`doctor` runs read-only environment checks: configuration validity, data directory permissions, Agent IPC reachability, port availability, printer visibility, browser and Office converter availability, and remote connection reachability (the headless product additionally checks systemd service status).

There are no `print-bridge serve install` or `uninstall` commands. Service lifecycle is managed by the system package manager; upgrades preserve configuration and state, and data is only removed on a full purge.

## Mutual Exclusivity

Both the GUI and headless products occupy `/usr/bin/print-bridge`, so packages bidirectionally declare `Conflicts` and `Provides: print-bridge`. Attempting to install the other product fails explicitly rather than automatically uninstalling or replacing the installed variant.

## Health Checks and Diagnostics

The product does not expose `/health` or other REST APIs. Use the following methods for health monitoring:

- `systemctl status print-bridge`
- `print-bridge status` (via local IPC)
- `print-bridge doctor` (environment checks described above)
- WebSocket `/ws` ping/pong
- systemd `Type=notify` READY/STOPPING states

## Dependencies

Headless operation requires a CUPS client, LibreOffice, and a Chrome/Chromium installation executable by a system user. Printers must be visible to the `printbridge` user; browser and LibreOffice runtime files must write to `/var/lib/print-bridge` or temp directories, and cannot depend on a personal home directory.
