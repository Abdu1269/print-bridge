# Remote Task Polling

<div align="right">

[中文](./remote-polling.md)

</div>

Remote task polling allows PrintBridge to run as an unattended agent on workstations, store terminals, or warehouse computers. The business server maintains pending print tasks, while the local agent periodically polls for tasks, submits them to the local operating system print queue, and reports execution status back to the server.

This is ideal for scenarios where backend systems create tasks and require specific physical terminals to print automatically—manufacturing labels, shipping waybills, picking lists, receipts, and more.

## Polling Protocol

The agent uses the same configured `remote.endpoint_url` for both operations:

```text
GET  {endpoint_url}   → Fetch pending tasks
POST {endpoint_url}   → Report task status
```

### Authentication Headers

```text
Authorization: Bearer <bearer_token>        (if configured)
X-PrintBridge-Device-Id: <device_id>        (if configured)
X-PrintBridge-Device-Name: <device_name>    (if configured)
```

All three fields are optional and independent—only configured values are transmitted.

### Task Fetch Response

The `GET` response is flexible:
- `204 No Content` or empty body → No tasks
- `null` → No tasks
- Single task object → One task
- Array of task objects → Multiple tasks

Tasks use the same field structure as WebSocket jobs:

```json
{
  "type": "print",
  "request_id": "REQ-001",
  "job_id": "JOB-001",
  "format": "pdf",
  "file_url": "https://example.com/label.pdf",
  "copies": 1
}
```

Batch tasks include a `batch_id` and a `jobs` array. `job_id` serves as the deduplication key—tasks with an already-seen `job_id` are silently skipped.

### Status Report Request Body

```json
{
  "event": "status",
  "event_id": "8c3f0f3a-...",
  "request_id": "REQ-001",
  "job_id": "JOB-001",
  "status": "success",
  "message": "submitted to system print queue",
  "occurred_at": "2026-07-06T10:00:00Z",
  "device_id": "f77160d2-...",
  "device_name": "packing-station-01"
}
```

`event_id` is a locally generated UUID v4 persisted in SQLite—servers can use it as an idempotency key.

## Status Mapping

PrintBridge reports only three remote statuses:

| Local Queue Status | Reported Remote Status |
|---|---|
| `queued` | `accepted` |
| `submitted` | `success` |
| `failed` | `failed` |
| `cancelled` | `failed` |

`downloading`, `printing`, `completed`, and `unknown` are **not** reported to the remote server—they remain recorded only in local logs and task history.

## Worker Loop (`remote_worker.rs`)

The remote worker runs an infinite loop:

1. Reads configuration; if `remote.enabled` is false → `await` `remote_notify` (wakes upon configuration change).
2. **Poll:** `fetch_tasks` → validate → deduplicate → enqueue into print queue.
3. **Report:** Deliver pending status events from SQLite outbox.
4. If a configuration error occurs (HTTP 401/403/404) → `await` `remote_notify` (ceases bombing the server).
5. Sleep for `poll_interval_seconds` (default 10, minimum 3).
6. Repeat.

## SQLite Persistence (`remote_store.rs`)

Remote state is persisted in `remote.sqlite3`, which contains two tables:

### `remote_jobs`—Deduplication Table

| Column | Purpose |
|---|---|
| `job_id` (PRIMARY KEY) | Deduplication key |
| `request_id`, `batch_id` | Original request tracking |
| `status` | Current status |
| `first_seen_at`, `updated_at` | Timestamps |

`record_job_if_new` uses `INSERT OR IGNORE`—returning `true` only on initial insertion. This provides cross-restart deduplication: if the agent restarts, tasks already in the queue are not reenqueued.

### `remote_status_events`—Status Outbox

| Column | Purpose |
|---|---|
| `event_id` (PRIMARY KEY) | UUID v4, idempotency key |
| `job_id`, `status`, `message` | Status to report |
| `delivery_state` | `pending` / `delivered` / `abandoned` |
| `retry_count` | Incremented on failure |
| `next_retry_at` | Timestamp with exponential backoff |
| `last_error` | Most recent failure error message |

A unique index on `(job_id, status)` prevents duplicate status reports for each job.

### Delivery with Exponential Backoff

`report_pending_once` fetches up to 20 pending events and posts them sequentially:
- **Success** (HTTP 2xx) → `mark_delivered`
- **Failure** (non-2xx or network error) → `mark_delivery_failed` → increments `retry_count`, sets `next_retry_at` with exponential backoff, abandons after exceeding `max_report_retries` (default 10).
- **Configuration Error** (401/403/404) → propagates immediately, pausing polling and reporting until configuration is corrected.

## Connection Test

The "Test Connection" button in Settings and remote configuration saves trigger a test via `remote_client::test_connection`:
1. Sends a `GET` request to the endpoint with the `X-PrintBridge-Test: true` header.
2. Sends a `POST` request with the same test header.
3. The server should return `204 No Content` for test requests.

## Configuration Error Handling

HTTP 401, 403, and 404 are classified as **configuration errors**—not transient failures. When encountered, the remote worker pauses both polling and status reporting, waiting on `remote_notify` (triggered when the user updates remote settings). This prevents repeated spamming of misconfigured or unauthorized endpoints.

## Server Implementation Examples

Reference implementations in `examples/remote-task/` demonstrate the server-side HTTP API:

| Language | File | Sample Task |
|---|---|---|
| Node.js | `remote-task-server.mjs` | Single PDF (`JOB-NODE-PDF`) |
| PHP | `remote-task-server.php` | Single image (`JOB-PHP-IMAGE`) |
| Go | `remote-task-server.go` | PDF + Image batch (`BATCH-GO-SAMPLE`) |

All three examples:
- Listen on `127.0.0.1:18080`
- Use the Bearer token `dev-token`
- Return `204 No Content` for requests with `X-PrintBridge-Test: true`
- Return task JSON on `GET`; log status and return `204` on `POST`

## Source Code Reference

| Domain | File |
|---|---|
| HTTP Client (Fetch + Report + Test) | `crates/runtime/src/remote_client.rs` |
| Task/Batch Protocol Structs | `crates/core/src/remote_protocol.rs` |
| SQLite Storage (Deduplication + Outbox) | `crates/runtime/src/remote_store.rs` |
| Worker Loop (Polling + Reporting + Backoff) | `crates/runtime/src/remote_worker.rs` |
