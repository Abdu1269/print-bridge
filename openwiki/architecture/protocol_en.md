# Print Protocol and API

<div align="right">

[中文](./protocol.md)

</div>

PrintBridge exposes a WebSocket endpoint for browser-initiated real-time printing. Configuration, logging, printer discovery, and diagnostics are handled via Tauri commands (Desktop) or CLI/local IPC (headless)—there is no HTTP REST API.

The WebSocket protocol is served by the Axum server on `0.0.0.0:{port}` (default `17890`). The router only exposes `/ws`; no other HTTP routes are available.

## WebSocket API

### Connection

```text
ws://127.0.0.1:17890/ws
```

During the WebSocket handshake, the server validates the `Origin` header against `config.security.allowed_origins`. If the Origin is not in the allowlist, the upgrade request is rejected. The connecting client's IP must also pass the IP allowlist check (applied as middleware across all routes including `/ws`).

### Client → Server Messages

All messages are JSON with a `type` discriminator tag (`#[serde(tag = "type")]`).

| Message Type | Fields | Purpose |
|---|---|---|
| `ping` | `time` | Heartbeat; server replies with `pong` |
| `get_printers_list` | — | Request the list of available printers |
| `get_printer_info` | `printer_name` | Request details for a specific printer |
| `get_print_queue` | — | Request the current print queue status |
| `print` | `request_id`, `job` (`format`, `file_url`/`data_base64`/`html`, `printer_name?`, `copies?`, `paper?`, `wait_ms?`) | Submit a single print job |
| `print_batch` | `request_id`, `batch_id`, `jobs[]` | Atomically submit multiple jobs |

### Server → Client Messages

| Message Type | Purpose |
|---|---|
| `pong` | Heartbeat response, containing `time` |
| `printers_list` | Response to `get_printers_list` |
| `printer_info` | Response to `get_printer_info` |
| `print_queue` | Response to `get_print_queue` |
| `job_status` | Job status update (pushed asynchronously) |
| `error` | Error response, containing `code` and `message` |

### Job Lifecycle

Each WebSocket connection only receives `job_status` events for jobs it submitted (tracked via per-connection `accepted_job_ids: HashSet`).

```text
Queued → Downloading → Printing → Submitted → Completed
                                          ↘ Failed
                                          ↘ Unknown
                                          ↘ Cancelled
```

| Status | Meaning |
|---|---|
| `queued` | The job has entered the serial queue |
| `downloading` | Downloading file content from `file_url` |
| `printing` | Converting (if needed) and submitting to the OS print queue |
| `submitted` | The job has been accepted by the OS print queue (terminal status for reports) |
| `completed` | The OS reports the job has completed printing (CUPS only; not trackable on Windows) |
| `failed` | The job failed at any stage |
| `unknown` | The OS cannot determine the final status |
| `cancelled` | The job was cancelled |

### Single Print Job Example

```json
{
  "type": "print",
  "request_id": "REQ-001",
  "job_id": "JOB-001",
  "format": "pdf",
  "printer_name": "Office Printer",
  "file_url": "https://example.com/label.pdf",
  "copies": 1,
  "paper": { "width_mm": 60, "height_mm": 40 }
}
```

- `printer_name` is optional—falls back to the configured default printer
- `paper` is optional—falls back to the configured default paper
- `copies` is optional—defaults to 1; must be ≤ `limits.max_copies`

### Raw Print Job Example

```json
{
  "type": "print",
  "request_id": "REQ-RAW-001",
  "job_id": "JOB-RAW-001",
  "format": "raw",
  "printer_name": "TSC TE244",
  "data_base64": "XlhB..."
}
```

Raw jobs do **not** support `file_url`, `paper`, or `copies`. The raw bytes from `data_base64` are submitted directly to the OS print queue as-is. PrintBridge does not parse or generate device commands (ESC/POS, TSPL, ZPL, EPL, PCL, PostScript).

### HTML Print Job Example

```json
{
  "type": "print",
  "request_id": "REQ-HTML-001",
  "job_id": "JOB-HTML-001",
  "format": "html",
  "file_url": "https://example.com/receipt.html",
  "wait_ms": 2000
}
```

HTML jobs render the web page in a headless Chrome/Chromium/Edge browser, export it to PDF, and then submit it through the regular PDF printing path. The `file_url` must be an absolute `http` or `https` URL. The `wait_ms` field (default 1000, max 30000) controls how long the renderer waits for page stability before exporting to PDF.

All browser resource requests pass through a filtering proxy that blocks non-public network destinations (loopback, private IPs, link-local, multicast). This prevents untrusted HTML pages from mounting SSRF attacks.

### Raw HTML Print Job Example

```json
{
  "type": "print",
  "request_id": "REQ-RAW-HTML-001",
  "job_id": "JOB-RAW-HTML-001",
  "format": "raw-html",
  "html": "<h1>Shipping Label</h1><p>Order #12345</p>",
  "wait_ms": 500
}
```

`raw-html` jobs carry inline HTML strings rather than an external URL. The same rendering pipeline is used (Chrome/Chromium via the filtering proxy). Inline HTML also loads assets through the proxy, so any referenced resources (images, stylesheets) must reside on public network destinations.

### Batch Print Job Example

```json
{
  "type": "print_batch",
  "request_id": "REQ-002",
  "batch_id": "BATCH-001",
  "jobs": [
    { "job_id": "A-001", "format": "image", "file_url": "https://example.com/a.png", "copies": 1 },
    { "job_id": "B-001", "format": "raw", "printer_name": "TSC TE244", "data_base64": "XlhB..." }
  ]
}
```

Batch jobs can mix PDF, image, Office, HTML, raw-html, and raw formats. The `batch_id` and all `job_id` values must be unique. The batch size is capped by `limits.max_batch_jobs` (default 20). Batch execution still uses the same single serial queue—jobs are not printed concurrently.

### Job Status Notification

```json
{
  "type": "job_status",
  "request_id": "REQ-001",
  "job_id": "JOB-001",
  "status": "queued",
  "message": "queued"
}
```

### Supported Formats

| Format | Input | Conversion |
|---|---|---|
| `pdf` | `file_url` or `data:application/pdf;base64,...` | None |
| `image` / `png` / `jpg` / `jpeg` | `file_url` | Image → PDF (fitted to paper size, 203 DPI) |
| `docx` / `xlsx` / `pptx` | `file_url` (HTTP/HTTPS only) | Office → PDF; macOS/Linux uses LibreOffice, Windows uses Microsoft Office → WPS Office → LibreOffice |
| `html` | `file_url` (absolute HTTP/HTTPS) | HTML → PDF via headless Chrome/Chromium (with SSRF protection proxy) |
| `raw-html` | `html` (inline string) | HTML → PDF via headless Chrome/Chromium (with SSRF protection proxy) |
| `raw` | `data_base64` | None—submitted as raw bytes |

Office tasks only support HTTP(S) `file_url`; data URLs are not supported. HTML tasks support the optional `wait_ms` field (0–30000ms, default 1000ms) to control rendering wait time.

### Error Codes

| Error Code | Meaning |
|---|---|
| `ORIGIN_NOT_ALLOWED` | The WebSocket Origin is not in the allowlist |
| `INVALID_MESSAGE` | Invalid message syntax or schema |
| `PRINTER_NOT_CONFIGURED` | No default printer is configured and none was specified |
| `PRINTER_NOT_FOUND` | The specified printer does not exist |
| `PAPER_NOT_CONFIGURED` | No default paper is configured and none was specified |
| `PAPER_NOT_FOUND` | The printer does not support this paper size |
| `DOWNLOAD_FAILED` | File download failed |
| `FILE_TOO_LARGE` | File exceeds maximum size limit |
| `UNSUPPORTED_FORMAT` | Unsupported document format |
| `FORMAT_MISMATCH` | Declared format does not match file magic bytes |
| `OFFICE_CONVERT_FAILED` | Office to PDF conversion failed |
| `PRINT_FAILED` | Submission to OS print queue failed |
| `JOB_DUPLICATED` | `job_id` already exists |
| `BATCH_DUPLICATED` | `batch_id` already exists |
| `BATCH_TOO_LARGE` | Batch exceeds maximum job limit |
| `COPIES_OUT_OF_RANGE` | Copy count exceeds allowed limit |
| `SERVICE_PORT_IN_USE` | Configured port is already in use |
| `INTERNAL_ERROR` | Unexpected internal server error |

## Source Code Reference

| Domain | File |
|---|---|
| WS Handler + IP Middleware | `crates/runtime/src/server.rs` |
| Message Types + Validation + Error Codes | `crates/core/src/protocol.rs` |
| Per-connection Status Filtering | `crates/runtime/src/server.rs` |
| Batch Acceptance + Deduplication | `crates/runtime/src/queue.rs` |
| HTML Rendering | `crates/runtime/src/html/` |

Detailed protocol examples can be found in `docs/printbridge-technical_en.md` (WebSocket API section). Browser integrations should use [print-bridge-sdk](https://github.com/vergil-lai/print-bridge-jssdk), which encapsulates this protocol.
