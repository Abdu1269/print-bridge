# Configuration

<div align="right">

[中文](./configuration.md)

</div>

PrintBridge stores all settings in a single JSON configuration file shared across the GUI, CLI, headless `serve`, remote worker, and config import/export. This single file is the sole source of truth—CLI commands modify it directly, while the GUI reads and writes it via Tauri commands.

## Configuration Structure

The root `AgentConfig` (`config.rs`) contains six sections:

### `service`

| Field | Type | Default | Description |
|---|---|---|---|
| `host` | String | `"127.0.0.1"` | Compatibility placeholder—does not restrict binding. Always normalized to `127.0.0.1`. |
| `port` | u16 | `17890` | Actual listening port. Range: 10000–65535. |

### `security`

| Field | Type | Default | Description |
|---|---|---|---|
| `allowed_origins` | Vec\<String\> | `[]` | Website Origin allowlist for WebSocket connections |
| `allowed_ips` | Vec\<String\> | `["127.0.0.1"]` | IP/CIDR allowlist. `127.0.0.1` is always present and cannot be removed. |

### `printing`

| Field | Type | Default | Description |
|---|---|---|---|
| `default_printer` | Option\<String\> | `null` | Default printer name |
| `default_paper` | Option\<EffectivePaper\> | `null` | Default paper (width_mm + height_mm) |
| `default_copies` | u16 | `1` | Default print copy count |

### `limits`

| Field | Type | Default | Description |
|---|---|---|---|
| `max_file_size_mb` | u32 | `20` | Maximum file download size per job |
| `max_batch_jobs` | u32 | `20` | Maximum jobs permitted per batch |
| `max_copies` | u16 | `100` | Maximum print copies per job |
| `download_timeout_seconds` | u64 | `30` | Download timeout in seconds |

### `app`

| Field | Type | Default | Description |
|---|---|---|---|
| `autostart` | bool | `false` | Launch automatically on system startup |
| `language` | UiLanguage | `zh-CN` | UI language (`zh-CN` or `en`) |

### `remote`

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable remote task polling |
| `endpoint_url` | Option\<String\> | `null` | Polling and reporting endpoint URL (http/https only) |
| `bearer_token` | Option\<String\> | `null` | Authorization Bearer token |
| `device_id` | Option\<String\> | `null` | Transmitted in `X-PrintBridge-Device-Id` header |
| `device_name` | Option\<String\> | `null` | Transmitted in `X-PrintBridge-Device-Name` header |
| `poll_interval_seconds` | u64 | `10` | Polling interval in seconds (minimum 3) |
| `max_report_retries` | u32 | `10` | Max status reporting retry attempts (minimum 1) |
| `history_retention_days` | u32 | `3` | Remote task history retention period in days |

## Configuration Example

```json
{
  "service": { "host": "127.0.0.1", "port": 17890 },
  "security": {
    "allowed_origins": ["https://example.com"],
    "allowed_ips": ["127.0.0.1", "192.168.1.0/24"]
  },
  "printing": {
    "default_printer": "TSC TE244",
    "default_paper": { "width_mm": 60, "height_mm": 40 },
    "default_copies": 1
  },
  "limits": {
    "max_file_size_mb": 20,
    "max_batch_jobs": 20,
    "max_copies": 100,
    "download_timeout_seconds": 30
  },
  "app": { "autostart": false, "language": "zh-CN" },
  "remote": {
    "enabled": false,
    "endpoint_url": null,
    "bearer_token": null,
    "device_id": null,
    "device_name": null,
    "poll_interval_seconds": 10,
    "max_report_retries": 10,
    "history_retention_days": 3
  }
}
```

## Persistence and Loading

- `AgentConfig::load(path)`—Reads JSON; returns `Default` if the file does not exist; always calls `normalized()` (enforces `host=127.0.0.1`, normalizes IP allowlist).
- `AgentConfig::save(path)`—Pretty-prints JSON and creates parent directories if needed.
- GUI and CLI use the exact same `config.json` file.

## Data Directories

| Platform | Default Path |
|---|---|
| Windows | `%APPDATA%\com.vergil.printbridge` |
| macOS | `~/Library/Application Support/com.vergil.printbridge` |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/com.vergil.printbridge` |

Files residing in the data directory:

| File | Purpose |
|---|---|
| `config.json` | Agent configuration |
| `task_history.sqlite3` | Task history and event log |
| `remote.sqlite3` | Remote task deduplication and status outbox |

## Environment Variables

| Variable | Effect |
|---|---|
| `PRINT_BRIDGE_DATA_DIR` | Overrides the entire data directory (config + SQLite databases) |
| `PRINT_BRIDGE_CONFIG_PATH` | Overrides only the configuration file path; SQLite databases remain in the data directory |

If `PRINT_BRIDGE_CONFIG_PATH` is not set, configuration defaults to `{data_dir}/config.json`.

## Validation Rules

The configuration system enforces the following constraints:

- Port range: 10,000–65,535
- Remote polling interval: ≥ 3 seconds
- Remote report retries: ≥ 1
- Remote endpoint URL: http or https only
- IP entries: Valid IPv4/IPv6 or CIDR; rejects `0.0.0.0`, `::`, `/0` prefixes
- Origin: Must be a valid URL with http/https scheme
- `127.0.0.1` is always injected into `allowed_ips`

## Configuration Import / Export

For encrypted configuration transfer format and cross-language generator examples, see the [Security Model](security_en.md). The Settings UI supports:
- **Export:** Select config sections to include, set an optional password, and generate an encrypted `printbridge-config.json`.
- **Import:** Select file and provide password, preview diff, and confirm merge.
- Bearer tokens are overwritten only when the imported file contains a non-empty string.

## Source Code Reference

| Domain | File |
|---|---|
| Config Structs + Defaults + Load/Save | `crates/core/src/config.rs` |
| Config Encryption / Decryption | `crates/cli/src/config_transfer.rs` |
| Data Directory Resolution + Env Vars | `crates/core/src/config.rs` |
| System Paths (Headless) | `apps/server/src/paths.rs` |
| Frontend Config Types | `apps/desktop/src/types.ts` |
| Frontend API Calls | `apps/desktop/src/api.ts` |
