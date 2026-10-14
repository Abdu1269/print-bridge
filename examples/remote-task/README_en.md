# Remote Task Examples

<div align="right">

[中文](./README.md)

</div>

This directory contains server-side examples for PrintBridge remote task polling. They simulate a business server, providing the same remote task interface to PrintBridge:

```text
GET  /print-task  Fetch tasks
POST /print-task  Receive status reports
```

Default listening address:

```text
http://127.0.0.1:18080/print-task
```

Default Bearer Token:

```text
dev-token
```

In the PrintBridge settings page under Remote Task configuration, enter:

```text
Task API URL: http://127.0.0.1:18080/print-task
Bearer Token: dev-token
```

## Node

Run:

```bash
node examples/remote-task/node/remote-task-server.mjs
```

By default, this returns a single PDF task, using a sample print file from the JSSDK examples assets:

```text
https://raw.githubusercontent.com/vergil-lai/print-bridge-jssdk/main/examples/assets/printbridge-a4-sample.pdf
```

To modify the port, token, or file URL, change the constants directly in the example code.

## PHP

Run:

```bash
php -S 127.0.0.1:18080 examples/remote-task/php/remote-task-server.php
```

By default, this returns a single image task, using a sample print file from the JSSDK examples assets:

```text
https://raw.githubusercontent.com/vergil-lai/print-bridge-jssdk/main/examples/assets/printbridge-a4-sample.jpg
```

To modify the token, file URL, or format, change the variables directly in the example code.

## Go

Run:

```bash
go run examples/remote-task/go/remote-task-server.go
```

By default, this returns a batch task containing both PDF and JPG files from the JSSDK examples assets:

```text
https://raw.githubusercontent.com/vergil-lai/print-bridge-jssdk/main/examples/assets/printbridge-a4-sample.pdf
https://raw.githubusercontent.com/vergil-lai/print-bridge-jssdk/main/examples/assets/printbridge-a4-sample.jpg
```

To modify the port, token, or file URL, change the constants directly in the example code.

## Connection Test

When saving remote configuration or clicking "Test Connection" in PrintBridge, a test request containing the following header is sent:

```text
X-PrintBridge-Test: true
```

All three examples respond to test `GET` and test `POST` requests with `204 No Content`, without issuing real tasks or writing status.

## Status Reporting

After PrintBridge fetches a task, it reports the remote status back via `POST /print-task`. The examples print the received JSON to the terminal.

The main statuses received currently are:

```text
accepted
success
failed
```

`success` indicates that the task has been submitted to the system print queue; it does not mean the printer has physically ejected the paper.

## Polling and Deduplication

These examples are stateless and return the same fixed task on every `GET`. PrintBridge tracks remote tasks by `job_id`, and a `job_id` that has already been processed will not enter the print queue again.
