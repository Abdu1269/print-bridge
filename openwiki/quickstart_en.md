# PrintBridge — Quick Start

<div align="right">

[中文](./quickstart.md)

</div>

PrintBridge is a local print agent running on the user's computer. It allows trusted web pages or remote business servers to send PDF files, images, Office documents, HTML pages, and raw printer commands to the local system print queue—designed for labels, logistics waybills, receipts, reports, and other business scenarios requiring reliable silent printing.

It **does not** replace printer drivers, nor does it bypass the operating system's print queue. It receives tasks, verifies origins, downloads or converts files, and submits jobs to the local operating system. Actual paper output is still handled by the system print queue, printer driver, and hardware.

## Tech Stack

| Layer | Technology |
|---|---|
| Framework | Tauri 2 |
| Frontend | Vue 3 + TypeScript |
| UI | shadcn-vue + Tailwind CSS |
| Build Tool | Vite |
| Backend | Rust + Axum + Tokio |
| Storage | JSON Configuration + SQLite |
| Office Conversion | LibreOffice (macOS/Linux) / Windows: Microsoft Office → WPS Office → LibreOffice |
| HTML Rendering | Headless Chrome/Chromium/Edge via CDP (with SSRF-prevention proxy) |
| Platform Printing | SumatraPDF (Windows) / CUPS `lp` (macOS/Linux) |

## How It Works

```text
Web Page / Remote Business Server
  │
  ├── WebSocket /ws (Browser → Agent, real-time)
  │   or
  ├── HTTP Polling (Remote Server → Agent, periodic)
  │
  ▼
PrintBridge (Verify Origin → Download → Convert → Serial Queue)
  │
  ▼
System Print Queue → Printer Driver → Printer Hardware
```

`submitted` / `success` indicates that the job has been accepted by the OS print queue; it **does not** mean the printer has physically ejected the paper.

## Getting Started with Development

```bash
pnpm install
pnpm tauri dev
```

Tauri starts the Vite development server at `http://localhost:1420/`. The local print service listens on `0.0.0.0:17890`.

## Core Features

- System tray resident, main window hidden by default
- Cross-platform: Windows, macOS, Linux
- Local HTTP/WebSocket service (default port `17890`)
- Website Origin allowlist + IP/CIDR allowlist (two-layer security)
- Supports PDF, PNG/JPEG images, Office (docx/xlsx/pptx), HTML pages, and raw printer commands (ESC/POS, TSPL, ZPL, EPL, PCL, PostScript)
- Serial print queue—no concurrent printer contention
- Remote task polling for unattended workstations
- CLI operational mode (`print-bridge serve`, `print-bridge printer`, `print-bridge doctor`, etc.)
- Encrypted configuration import/export for batch deployments
- In-app automatic updates

## Documentation Navigation

### [Architecture Overview](architecture/overview_en.md)
Cargo workspace structure (`core`, `runtime`, `cli`, `desktop`, `server`), Axum server, print queue worker, remote polling worker, and the local IPC command system.

### [Protocol and API](architecture/protocol_en.md)
WebSocket print protocol (message types, job lifecycle, status events, error codes), covering HTML, raw-html, PDF, images, Office, and raw formats. Essential for integration.

### [Printing Pipeline](workflows/printing-pipeline_en.md)
How jobs flow through the serial queue: HTML rendering (Chrome/Chromium), download → format detection → conversion (Office/images → PDF) → platform-specific print execution (SumatraPDF on Windows, CUPS `lp` on macOS/Linux).

### [Remote Task Polling](workflows/remote-polling_en.md)
Server-initiated task delivery: polling and reporting HTTP protocol, SQLite-based deduplication, outbox-based status reporting (with exponential backoff), and configuration error handling.

### [Security Model](domain/security_en.md)
Dual-layer allowlist architecture (IP + Origin), configuration transfer encryption (Argon2id + AES-256-GCM), and security best practices.

### [Configuration](domain/configuration_en.md)
Full configuration schema (`service`, `security`, `printing`, `limits`, `app`, `remote`), data directory locations, environment variables, and field reference.

### [Operations and Deployment](operations/deployment_en.md)
CLI command reference, headless `serve` mode, `doctor` diagnostics, systemd packaging, troubleshooting, and platform-specific deployment guidelines.

### [Source Map](source-map_en.md)
Developer codebase guide: file-by-file responsibilities, caveats, and key architectural decisions.

## Integration Methods

| Consumer | Protocol | Reference |
|---|---|---|
| Browser Web Page | WebSocket `/ws` | [Protocol and API](architecture/protocol_en.md) — or use [print-bridge-sdk](https://github.com/vergil-lai/print-bridge-jssdk) |
| Business Server | HTTP Polling | [Remote Task Polling](workflows/remote-polling_en.md) — server examples in `examples/remote-task/` |
| ERP / Batch Deployment | Encrypted Config | [Security Model](domain/security_en.md) — cross-language generator examples in `examples/config-transfer/` |
| Settings UI / Diagnostics | Local IPC | [Architecture Overview](architecture/overview_en.md) |

## Key Source Files

Quick reference for essential entry points:

| Domain | Path |
|---|---|
| Application Entry (GUI) | `apps/desktop/src-tauri/src/lib.rs` |
| Headless Entry | `apps/server/src/main.rs` |
| HTTP/WS Server | `crates/runtime/src/server.rs` |
| WebSocket Protocol | `crates/core/src/protocol.rs` |
| Print Queue + Pipeline | `crates/runtime/src/queue.rs` |
| HTML Rendering | `crates/runtime/src/html/` |
| Office Conversion | `crates/runtime/src/office.rs` |
| Configuration | `crates/core/src/config.rs` |
| CLI Framework | `crates/cli/src/` |
| Frontend | `apps/desktop/src/App.vue` |

## Existing Documentation

| Document | Language | Description |
|---|---|---|
| `README.md` | Chinese | Project overview and quick start |
| `README_en.md` | English | Project overview and quick start |
| `docs/printbridge-technical.md` | Chinese | Detailed protocol, API, configuration, and deployment |
| `docs/printbridge-technical_en.md` | English | Detailed protocol, API, configuration, and deployment |
