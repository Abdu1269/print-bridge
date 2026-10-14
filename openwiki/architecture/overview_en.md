# Architecture Overview

<div align="right">

[中文](./overview.md)

</div>

PrintBridge is a Cargo workspace delivering two mutually exclusive products: a Desktop GUI and a Linux headless service. Both products use `print-bridge` as the executable filename, but are provided by different packages.

## Workspace

```text
crates/core       Configuration, protocol, allowlists, task and print models
crates/runtime    AgentState, print adapters, queue, worker, WebSocket and local IPC
crates/cli        Command, CommandResult, CommandService, config import/export, and IPC client
apps/desktop      Vue frontend and Tauri backend
apps/server       Linux headless entry point with deb/rpm/systemd packaging
```

The dependency direction is `core <- cli <- runtime <- desktop/server`. `core` does not depend on Tauri, Axum, Clap, or SQLite.

## Command Flow

GUI, CLI, and headless local management uniformly construct strongly-typed `Command` instances and pass them to `CommandService`. The running Agent processes commands over a Unix socket (named pipes boundary on Windows); offline fallback is only permitted on explicit `NotRunning` errors—permission, protocol, or runtime errors are never misidentified as the Agent not being started.

Local IPC uses a 4-byte big-endian length prefix and JSON envelope, with protocol version 1 and an 8 MiB maximum frame size. The Unix socket is located at `agent.sock` within the runtime directory, with fixed permissions of `0660`.

## Network Interfaces

The Agent's Axum router only exposes `/ws`. The browser printing protocol, heartbeats, printer queries, and task statuses all communicate via WebSocket; configuration, logs, import/export, and test printing do not provide a REST API.

## Product Formats

- Desktop: Starts the GUI when launched without arguments; `serve` is an unknown command.
- Headless: Displays help when launched without arguments; only starts the Agent on explicit `print-bridge serve`.
- Linux headless packages are automatically started by systemd after installation, running under a dedicated `printbridge` system user.

## Runtime Lifecycle

`RuntimeBuilder` creates configuration, data, and runtime directories, and assembles stores/adapters. `AgentRuntime::start` binds the WebSocket listener, starts IPC, the queue, and the remote worker, returning an `AgentHandle`. All tasks share a cancellation token; on shutdown, the runtime waits for the worker, WebSocket, and IPC to complete, and cleans up the Unix socket.
