# Printing Pipeline

<div align="right">

[中文](./printing-pipeline.md)

</div>

The printing pipeline is the core processing pathway: from job acceptance through downloading, format conversion, platform-specific printing, and status tracking. All jobs—whether arriving via WebSocket or remote polling—pass through the same single serial FIFO queue.

## Serial Queue Architecture

The print queue is a strict FIFO implemented in `QueueState` (`queue.rs`) using `VecDeque<QueuedJob>`:

```rust
struct QueueState {
    pending: VecDeque<QueuedJob>,
    seen_job_ids: HashSet<String>,
    seen_batch_ids: HashSet<String>,
}
```

**Deduplication:** Every `job_id` is checked against `seen_job_ids` before acceptance. Duplicates are rejected with `JOB_DUPLICATED`. Batch jobs additionally check `batch_id` uniqueness against `seen_batch_ids`.

**Serial execution** is enforced by a single worker loop:

```rust
loop {
    if let Some(job) = state.queue.lock().await.pop_next() {
        process_job(&state, job).await;  // Next job is processed only after complete finish
        continue;
    }
    state.queue_notify.notified().await;  // Sleep until notified
}
```

The `print_lock` (`Arc<Mutex<()>>`) provides additional protection: even if multiple workers existed, only one platform print command can execute at any given time. Currently, only one worker runs per process.

## Job Acceptance Flow

Jobs enter the queue from two sources:

1. **WebSocket**—`protocol.rs` validates each `print`/`print_batch` message (`validate_for_acceptance`), then calls `queue.accept_job()` or `queue.accept_batch()`. The connection immediately receives a `JobStatus::Queued` event.

2. **Remote Polling**—`remote_worker.rs` validates remote tasks and calls `queue.accept_remote_job()` or `queue.accept_remote_batch()`, performing SQLite-based deduplication in `remote_store.rs`.

In both cases, `queue_notify.notify_one()` awakens the worker.

## Job Processing Pipeline (`process_job_inner`)

Once popped from the queue, each job follows this execution flow:

```text
Pop from queue
    │
    ├── Format = html / raw-html?
    │   ├── html: validate file_url (absolute http/https) → render via Chrome/Chromium
    │   └── raw-html: render inline HTML via Chrome/Chromium
    │       └── Browser launches with filtering proxy → CDP PrintToPDF → temp PDF → print
    │
    ├── Format = raw?
    │   └── Yes → decode base64 → resolve printer → print_raw() → track status
    │
    ├── Download file_url to temp file (pdf / image / office)
    │   ├── HTTP/HTTPS: streaming download with size limits (Content-Length + byte counter)
    │   └── data: URL: direct base64 decode
    │
    ├── Resolve printer (specified or default) + paper (specified or default)
    │
    ├── Convert to PDF if necessary
    │   ├── Office (docx/xlsx/pptx) → office_to_pdf() via LibreOffice or Windows converter chain
    │   ├── Image (PNG/JPEG) → image_to_pdf() via printpdf crate (fitted to paper, 203 DPI)
    │   └── PDF → normalize_pdf_path() (ensures .pdf extension for printing utilities)
    │
    ├── Submit to OS print queue via platform backend
    │   ├── Windows: SumatraPDF.exe -silent -print-to ...
    │   └── macOS/Linux: lp -d "{printer}" -n {copies} -o media={media}
    │
    ├── Track status (CUPS only)
    │
    └── Clean up temp files
```

If an `Err` occurs at any stage, the job is recorded as `JobStatus::Failed` with the error description.

## Format Detection

PrintBridge uses **magic byte detection** to verify that file contents match declared formats:

| Magic Bytes | Format | Detection Location |
|---|---|---|
| `%PDF-` | PDF | `document.rs` |
| `\x89PNG\r\n\x1a\n` | PNG | `document.rs` |
| `\xFF\xD8\xFF` | JPEG | `document.rs` |
| ZIP containing `word/document.xml` | Docx | `office.rs` |
| ZIP containing `xl/workbook.xml` | Xlsx | `office.rs` |
| ZIP containing `ppt/presentation.xml` | Pptx | `office.rs` |

If the declared format does not match the detected bytes, a `FORMAT_MISMATCH` error is returned.

## Image → PDF Conversion

Images are converted to a single-page PDF using the `printpdf` crate (`document.rs`, `image_to_pdf`):

- The image is **fitted within** the target paper dimensions (centered, preserving aspect ratio)
- Default assumed resolution: **203 DPI** (label printer standard)
- Paper dimensions are derived from the job's `paper` field or configuration defaults

## Office → PDF Conversion

Office documents (docx/xlsx/pptx) are converted to PDF via the host platform's native Office software (`office.rs` + `office/`). On macOS/Linux, LibreOffice (`soffice`/`libreoffice`) is invoked in an isolated profile with macro security set to maximum. On Windows, Microsoft Office COM, WPS Office COM, and LibreOffice are attempted in order according to format; fallback only occurs if the converter is unavailable, cannot prove the new process belongs to the task, or necessary security controls are missing. Failures after opening the document do not fall back. COM conversion does not reuse or terminate existing user processes. Conversions have a 120-second timeout, and output appearance depends on the selected software, fonts, and system environment.

## HTML Rendering Pipeline

HTML and raw-html jobs bypass the download phase and are rendered to a temporary PDF by `HtmlRenderer` (default: `BrowserHtmlRenderer`):

1. **Browser Discovery**—Searches platform-specific paths for Chrome, Chromium, or Edge.
2. **Filtering Proxy**—A local HTTP proxy intercepts all browser resource requests. `ResourcePolicy` blocks non-public IPs (loopback, private, link-local, multicast, `file:`, `data:` protocols). DNS is resolved before connection to prevent DNS rebinding.
3. **Rendering**—The browser navigates to the target URL (or loads inline HTML), waits for `wait_ms` milliseconds (default 1000, max 30000), and exports to PDF via CDP `Page.printToPDF`.
4. The generated temporary PDF is submitted through the standard PDF printing path and then cleaned up.

If a blocked resource is detected, rendering fails with `HtmlRenderError::BlockedResource` and the job is marked as failed.

## Download Security (`download.rs`)

`download_to_temp` enforces two layers of security:
1. **Content-Length Header Check**—Rejects downloads before streaming starts if the header exceeds the limit.
2. **Streaming Byte Counter**—Enforces the byte limit continuously while reading data.

Downloads have a configurable timeout (`limits.download_timeout_seconds`, default 30s). Partially downloaded files are cleaned up upon error.

Supported URL protocols: `http://`, `https://`, `data:application/pdf;base64,...`.

## Status Tracking

After a job is submitted to the OS print queue:

| Platform | Tracked? | Mechanism |
|---|---|---|
| macOS/Linux | **Yes** | `lpstat -W completed -o`—Checks whether the system job ID appears in the completed jobs list |
| Windows | **No** | Neither SumatraPDF nor Win32 Spooler API exposes reliable completion states; `tracking_supported: false` |

On macOS/Linux, tracking results can be `Completed`, `Failed`, or `Unknown` (if the job cannot be located).

## Deduplication and Idempotency

- **WebSocket:** In-memory `seen_job_ids` in `QueueState`—persists for the process lifetime.
- **Remote Polling:** `remote_jobs` table in `remote.sqlite3`—persists across restarts (see [Remote Task Polling](remote-polling_en.md)).

Both use `job_id` as the deduplication key.

## Source Code Reference

| Domain | File |
|---|---|
| Queue State + Worker Loop + Job Processing | `crates/runtime/src/queue.rs` |
| Format Detection + Image to PDF | `crates/runtime/src/document.rs` |
| Office Detection + Conversion | `crates/runtime/src/office.rs`, `office/libreoffice.rs`, `office/windows.rs` |
| HTML Rendering (Browser, Proxy, Policy) | `crates/runtime/src/html/` |
| Download to Temp File | `crates/runtime/src/download.rs` |
| Platform Printing Backends | `crates/runtime/src/printing/mod.rs`, `cups.rs`, `windows.rs` |
| Message Validation + Job Types | `crates/core/src/protocol.rs` |
