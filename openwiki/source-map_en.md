# Source Map

<div align="right">

[中文](./source-map.md)

</div>

A developer guide to the codebase: file responsibilities, caveats when modifying each area, and key design decisions.

## Project Structure

```text
PrintBridge/                       # Cargo workspace (5 members)
├── crates/
│   ├── core/                      # Domain models, protocol, config—no framework dependencies
│   │   └── src/
│   │       ├── config.rs          # AgentConfig + section structs, load/save, defaults
│   │       ├── protocol.rs        # ClientMessage/ServerMessage, validation, ErrorCode
│   │       ├── ip_whitelist.rs    # IP/CIDR validation + runtime checks
│   │       ├── printing.rs        # PrintBackend trait, PrinterInfo/PaperInfo types
│   │       ├── queue.rs           # QueueState types, QueueError
│   │       ├── remote_protocol.rs # RemoteTask types + parser
│   │       └── activity.rs        # TaskHistoryJob, TaskHistoryEvent, TaskLogEntry
│   ├── runtime/                   # AgentRuntime, platform adapters, background tasks
│   │   └── src/
│   │       ├── agent.rs           # AgentRuntime + AgentHandle lifecycle
│   │       ├── builder.rs         # RuntimeBuilder (paths + adapters → AgentRuntime)
│   │       ├── state.rs           # AgentState (shared config, queue, stores, adapters)
│   │       ├── server.rs          # Axum router (exposes /ws only)
│   │       ├── queue.rs           # Serial print queue + job processing pipeline
│   │       ├── command_executor.rs # RuntimeCommandExecutor (offline Command executor)
│   │       ├── doctor.rs          # Doctor diagnostic checks
│   │       ├── html/              # HTML → PDF rendering via Chrome/Chromium (CDP)
│   │       ├── printing/          # Platform print backends (CUPS, Windows)
│   │       ├── office/            # Office → PDF conversion (LibreOffice / Windows chain)
│   │       ├── document.rs        # Magic byte detection, image → PDF
│   │       ├── download.rs        # HTTP/HTTPS/data URL download to temp file
│   │       ├── ipc/               # Local IPC (Unix socket / Windows named pipe)
│   │       ├── remote_*.rs        # Remote task polling subsystem
│   │       ├── task_history.rs    # SQLite task history store
│   │       ├── logs.rs            # In-memory log ring buffer
│   │       ├── test_print.rs      # Calibration test page
│   │       └── agent_guard.rs     # Instance exclusivity (port probing)
│   └── cli/                       # Command enum, CommandService, CLI parser, IPC client
│       └── src/
│           ├── command.rs         # Command enum + policies (online/offline preference)
│           ├── service.rs         # CommandService (online/offline dispatch)
│           ├── parser.rs          # Clap command structs + CLI handler
│           ├── client.rs          # LocalClientExecutor (IPC client)
│           ├── output.rs          # CommandResult, CommandError, DoctorReport types
│           ├── policy.rs          # CommandPolicy enum
│           ├── product.rs         # ProductCommandAdapter (autostart, language)
│           ├── interaction.rs     # Terminal interaction prompts
│           └── config_transfer.rs # Encrypted config import/export
├── apps/
│   ├── desktop/                   # Vue 3 frontend + Tauri backend
│   │   ├── src/                   # Frontend (Vue 3 + TypeScript)
│   │   │   ├── App.vue            # Monolithic settings UI
│   │   │   ├── api.ts             # Tauri invoke + HTTP wrapper
│   │   │   ├── types.ts           # Shared TypeScript types
│   │   │   ├── i18n.ts            # vue-i18n (zh-CN, en)
│   │   │   └── updater.ts         # Tauri updater wrapper
│   │   └── src-tauri/             # Desktop Rust backend
│   │       ├── src/
│   │       │   ├── main.rs        # Binary entry point
│   │       │   ├── lib.rs         # Tauri app setup + runtime assembly
│   │       │   ├── cli.rs         # Desktop CLI dispatch
│   │       │   ├── product_cli.rs # Desktop product adapter (autostart, language)
│   │       │   ├── commands.rs    # Tauri commands (config, logs, test print)
│   │       │   └── tray.rs        # System tray
│   │       ├── resources/         # SumatraPDF.exe (Windows)
│   │       └── tests/             # Integration tests
│   └── server/                    # Linux headless binary
│       ├── src/
│       │   ├── main.rs            # Binary entry point: serve / shared CLI dispatch
│       │   ├── dependencies.rs    # Preflight (CUPS, LibreOffice, Chrome)
│       │   ├── parser.rs          # Server CLI args
│       │   ├── paths.rs           # System paths (/etc, /var/lib, /run)
│       │   ├── readiness.rs       # systemd READY/STOPPING notifications
│       │   └── signals.rs         # SIGTERM/SIGINT handling
│       ├── packaging/             # deb/rpm/systemd unit files
│       └── tests/
├── examples/                      # Server-side reference implementations
├── scripts/                       # Build/release utilities
├── docs/                          # Technical documentation
└── .github/workflows/             # CI (release + workflows)
```

## Core Crate (`crates/core`)

Domain models and protocol types with zero framework dependencies. Does not depend on Tauri, Axum, Clap, or SQLite.

| File | Responsibilities | Caveats |
|---|---|---|
| `protocol.rs` | `ClientMessage`/`ServerMessage` types, `JobStatus`, `ErrorCode`, `PrintJobInput` validation, `SupportedFormat` enum. | `validate_for_acceptance` is the gatekeeper for job admission. Error codes are protocol-stable. Includes `Html` and `RawHtml` formats + `html`/`wait_ms` fields. |
| `config.rs` | `AgentConfig` + section structs, defaults, `load`/`save`, `normalized`, data directory resolution. | `normalized()` enforces `host=127.0.0.1`. Config types must match frontend `types.ts`. |
| `ip_whitelist.rs` | IP/CIDR validation, `is_client_ip_allowed`, rejection of allow-all entries. | `127.0.0.1` is mandatory and immutable. Never trust proxy headers. |
| `printing.rs` | `PrintBackend` trait, `PrinterInfo`, `PaperInfo`, `PrintOptions`, `PrintSubmission`, `PrintTrackingOutcome`. | Framework-free types—platform implementations live in `crates/runtime`. |
| `queue.rs` | Queue state types and `QueueError`. | |
| `remote_protocol.rs` | `RemoteTask` enum + flexible parser. | Parser accepts single objects, arrays, null, or empty strings. |
| `activity.rs` | `TaskHistoryJob`, `TaskHistoryEvent`, `TaskLogEntry`—shared activity types. | |

## Runtime Crate (`crates/runtime`)

`AgentRuntime` lifecycle, platform adapters, and all background execution tasks.

### Agent Lifecycle

| File | Responsibilities | Caveats |
|---|---|---|
| `agent.rs` | `AgentRuntime` (pre-start), `AgentHandle` (running). `start()` binds listener and starts server/queue/remote/IPC tasks. | All four tasks share one `CancellationToken`. `shutdown()` awaits completion and deletes the IPC socket. |
| `builder.rs` | `RuntimeBuilder`—paths + optional print backend and HTML renderer → `AgentRuntime`. `RuntimePaths` (config, data, runtime dirs). | Defaults: `printing::default_backend()` and `BrowserHtmlRenderer`. |
| `state.rs` | `AgentState`—shared container: config, queue, stores, print backend, HTML renderer, IPC executor. | Low-cost clone via `Arc`. `status_events` broadcast capacity is 128. |
| `command_executor.rs` | `RuntimeCommandExecutor`—offline executor for `Command` (used when Agent is not running). | |
| `doctor.rs` | `run_doctor`—read-only diagnostics (config validity, data dir, ports, printers, browser, Office, systemd, remote). | Passively checks browser and Office converters per format; does not spawn Office. |

### Server and IPC

| File | Responsibilities | Caveats |
|---|---|---|
| `server.rs` | Axum router exposing only `/ws`. IP allowlist middleware, WebSocket handler, Origin validation. | No HTTP REST endpoints—config/logs/printers/test prints are handled via Tauri commands or CLI/IPC. |
| `ipc/mod.rs` | Platform dispatch for local IPC (Unix socket on Unix, named pipes on Windows). | 4-byte big-endian length prefix, JSON envelope, protocol v1, 8 MiB max frame. Socket at `agent.sock` with `0660` permissions. |
| `ipc/unix.rs` | Unix domain socket implementation. | |
| `ipc/windows.rs` | Named pipes implementation. | |

### Print Queue and Pipeline

| File | Responsibilities | Caveats |
|---|---|---|
| `queue.rs` | `QueueState` (FIFO + deduplication), `run_worker` (serial loop), `process_job_inner` (download → convert → print). Distinct branches for `html`, `raw-html`, and `raw`. | Single worker = strict serial execution. `print_html_job` renders to temp PDF then reuses PDF submission path. Temp file cleanup is critical. |
| `printing/mod.rs` | Platform dispatch (`default_backend`), `sumatra_print_settings`. | Backend selected at compile time via `#[cfg(target_os = ...)]`. |
| `printing/cups.rs` | macOS/Linux backend: `lp`, `lpstat`, `lpoptions`. Tracks jobs via `lpstat -W completed`. | Job ID parsing from `lp` stdout is fragile. |
| `printing/windows.rs` | Windows backend: SumatraPDF CLI for PDF, Win32 Spooler API for raw. | No job tracking (`tracking_supported: false`). |
| `document.rs` | Magic byte detection (PDF/PNG/JPEG), image → PDF conversion via `printpdf`. | Label printers assume 203 DPI. |
| `office.rs` + `office/` | Office format detection + conversion. macOS/Linux: LibreOffice (`libreoffice.rs`). Windows: Microsoft Office → WPS Office → LibreOffice (`windows.rs`). | Only falls back if converter is unavailable; fidelity depends on host software. 120s timeout. |
| `download.rs` | `download_to_temp`: HTTP/HTTPS streaming + data URLs. Dual-layer size limits. Timeout. | Content-Length check + streaming byte counter. Cleans up partial files on error. |

### HTML Rendering (`html/`)

| File | Responsibilities | Caveats |
|---|---|---|
| `mod.rs` | `HtmlRenderer` trait, `HtmlRenderRequest`, `HtmlSource` (Url or Inline), `HtmlRenderError`. | Trait is async (`Pin<Box<dyn Future>>`), enabling test injection. |
| `browser.rs` | `BrowserHtmlRenderer`—discovers Chrome/Chromium/Edge, launches with proxy, renders to PDF via CDP. | Renders through a filtering proxy to prevent SSRF. 60s render timeout, 10s per CDP operation. Discovers browser at platform-specific paths. |
| `resource_policy.rs` | `ResourcePolicy`—blocks non-public IPs (loopback, private, link-local, multicast). Resolves DNS prior to connecting. | Prevents SSRF attacks against internal infrastructure. Blocks `file:`, `data:`, `localhost`, `127.0.0.1`, `10.x`, `169.254.x`, etc. |
| `proxy.rs` | `FilteringProxy`—local HTTP proxy that intercepts all browser resource requests and applies `ResourcePolicy`. | All Chrome traffic is forced through proxy via `--proxy-server`. Blocked resources abort rendering with `BlockedResource`. |

### Remote Polling

| File | Responsibilities | Caveats |
|---|---|---|
| `remote_client.rs` | HTTP client: `fetch_tasks`, `report_status`, `test_connection`. | `is_configuration_status()` classifies 401/403/404 as configuration errors → backoff. |
| `remote_store.rs` | SQLite: `remote_jobs` (deduplication) + `remote_status_events` (outbox). | `INSERT OR IGNORE` ensures idempotency. |
| `remote_worker.rs` | Infinite loop: poll → enqueue → report. Pauses on configuration errors. | Implements status report backoff via `retry_at_string`. |

## CLI Crate (`crates/cli`)

Framework-free command types and CLI parser shared by desktop and headless products.

| File | Responsibilities | Caveats |
|---|---|---|
| `command.rs` | `Command` enum (GetConfig, SaveConfig, ListPrinters, Doctor, Status, ExportConfig, ImportConfig, etc.). Each command has a `CommandPolicy`. | Adding commands requires updating `policy()` and executor implementations. |
| `service.rs` | `CommandService`—dispatches `Command` to online (Agent via IPC) or offline executor according to policy. | Only `NotRunning` errors trigger offline fallback for `OnlinePreferred` commands. |
| `parser.rs` | Clap command definitions (status, config, printer, paper, origin, remote, task, logs, test-remote, test-print, autostart, app, service, ip, doctor). | CLI subcommands must mirror `Command` variants. |
| `client.rs` | `LocalClientExecutor`—sends commands to the running Agent over IPC. | |
| `output.rs` | `CommandResult`, `CommandError`, `AgentStatus`, `DoctorReport`/`DoctorCheck`/`DoctorSummary`, `ProductKind`. | Error types map to stable exit codes. |
| `policy.rs` | `CommandPolicy` enum (`OnlineOnly`, `OnlinePreferred`, `OfflineAllowed`). | |
| `product.rs` | `ProductCommandAdapter` trait (autostart, language). Desktop adapter enabled; headless uses `UnsupportedProductCommandAdapter`. | |
| `config_transfer.rs` | Encrypted export/import: Argon2id + AES-256-GCM, field selection, merge, preview diff. | Bearer tokens have special preservation rules (empty/null → keep original). |

### History and Diagnostics

| File | Responsibilities | Caveats |
|---|---|---|
| `task_history.rs` | SQLite: `task_history_jobs` (aggregated) + `task_history_events` (append-only). | `finished_at` is only set on terminal status. |
| `logs.rs` | `LogStore`—in-memory ring buffer (500 entries). Non-persistent. | Drops oldest entries when full. |
| `test_print.rs` | Calibration test page generator. | Uses default printer + paper. |
| `agent_guard.rs` | TCP port probing to detect running instances. | GUI aborts launch; headless returns `RuntimeError::AlreadyRunning`. |

## App Crates

### Desktop (`apps/desktop`)

| File | Responsibilities | Caveats |
|---|---|---|
| `src-tauri/src/main.rs` | Binary entry point. | |
| `src-tauri/src/lib.rs` | Tauri application setup: runtime builder, tray, command service assembly (online + offline executors), window close interception. | Re-exports `print_bridge_runtime` and `print_bridge_core` modules. Hides to tray on window close. |
| `src-tauri/src/cli.rs` | Desktop CLI dispatch. | |
| `src-tauri/src/product_cli.rs` | `DesktopProductCommandAdapter`—autostart via `auto_launch`, language settings. | macOS uses `LaunchAgent=false` (uses `.plist`). Linux resolves `APPIMAGE` path. |
| `src-tauri/src/commands.rs` | Tauri commands: config CRUD, export/import, test connection, logs, task history, test print. | `print_test` restricted to settings UI origin. |
| `src-tauri/src/tray.rs` | System tray: open settings, test print, view logs, restart, autostart, quit. Fully localized. | `toggle_autostart` persists to config. |

### Server (`apps/server`)

| File | Responsibilities | Caveats |
|---|---|---|
| `src/main.rs` | Headless binary entry point. No args → help. `serve` → `RuntimeBuilder` + `AgentRuntime::start`. Other args → shared CLI via `run_cli_from`. | Uses `UnsupportedProductCommandAdapter` (autostart managed by systemd, fixed language). |
| `src/dependencies.rs` | `preflight()`—verifies `lp`/`lpstat`/`lpoptions` (CUPS), `soffice`/`libreoffice`, and Chrome/Chromium exist on PATH before serve starts. | |
| `src/paths.rs` | `system_paths()`—resolves `/etc/print-bridge`, `/var/lib/print-bridge`, `/run/print-bridge`. | |
| `src/readiness.rs` | Sends `READY=1` / `STOPPING=1` to systemd via `sd_notify`. | |
| `src/signals.rs` | Handles `SIGTERM` / `SIGINT` for graceful shutdown. | |

## Frontend (`apps/desktop/src`)

| File | Responsibilities | Caveats |
|---|---|---|
| `App.vue` (~71 KB) | Monolithic SFC containing entire UI: Settings, Remote, Web Allowlist, IP Allowlist, Tasks, About tabs. No router. | Single file containing all UI logic. Port modifications trigger an application restart in production. |
| `api.ts` | All API calls routed through `invoke()` (Tauri commands). No direct HTTP requests. | Must match Rust command signatures. |
| `types.ts` | Shared TypeScript types: `AgentConfig`, `PrinterInfo`, `PaperInfo`, `TaskHistoryJob`, `TaskHistoryEvent`. | Must match Rust core structs. |
| `i18n.ts` | vue-i18n v11 (composition mode). `zh-CN` (default) + `en`. ~140 keys. | Task status/source labels are inline lookup tables in App.vue. |
| `updater.ts` | Tauri updater wrapper: check, download+install, restart. | Progress updates delivered via `DownloadEvent` callback. |
| `main.ts` | Application entry point: creates Vue app, installs vue-i18n, mounts to DOM. | |

## Auxiliary Files

| Path | Purpose |
|---|---|
| `apps/desktop/src-tauri/tauri.conf.json` | Application configuration: identifier `com.vergil.printbridge`, window 960×680, updater endpoint |
| `apps/desktop/src-tauri/tauri.windows.conf.json` | Windows override configuration: bundles `SumatraPDF.exe` as a resource |
| `apps/desktop/src-tauri/capabilities/default.json` | Main window capabilities: core, dialog, opener defaults |
| `apps/desktop/src-tauri/capabilities/desktop.json` | Desktop capabilities: autostart, log, updater, process, dialog, opener |
| `apps/server/packaging/deb/control` | deb package metadata (`print-bridge-server`, depends on systemd + cups-client + libreoffice) |
| `apps/server/packaging/rpm/print-bridge.spec` | RPM spec file |
| `apps/server/packaging/systemd/print-bridge.service` | systemd unit: `Type=notify`, runs as `printbridge` user, `ProtectSystem=strict` |
| `scripts/release.mjs` | Interactive release: version synchronization, tag verification, push to `release` branch |
| `scripts/build-server-packages.sh` | Builds deb/rpm packages for headless server |
| `scripts/patch-updater-json.mjs` | Post-release: rewrites `latest.json` asset URLs |
| `scripts/release-version.mjs` | Version verification coordinating releases |
| `scripts/verify-config-transfer-examples.mjs` | Runs self-tests for all config transfer examples |
| `.github/workflows/release.yml` | CI: builds desktop (macOS ARM+Intel, Linux, Windows) and server (deb/rpm) packages |
| `.github/workflows/sync-release-notes.yml` | Synchronizes release notes |
| `docs/printbridge-technical.md` | Detailed technical documentation (Chinese) |
| `docs/printbridge-technical_en.md` | English technical documentation |
