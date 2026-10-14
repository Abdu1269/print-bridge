# Security Model

<div align="right">

[中文](./security.md)

</div>

PrintBridge runs on the user's local computer with access to connected physical printers. Security is safeguarded through a dual-layer allowlist architecture (IP + Origin), encrypted configuration transfer for mass deployments, and clear trust boundaries.

## Dual-Layer Allowlist Architecture

All incoming requests must pass through **two layers** of access control:

```text
Incoming Request (HTTP or WebSocket)
    │
    ├── Layer 1: IP Allowlist Middleware (All routes)
    │   Extracts client IP from Axum ConnectInfo<SocketAddr>
    │   → Rejected if not in allowed_ips
    │
    └── Layer 2: WebSocket Origin Check (/ws only)
        Validates Origin header against allowed_origins
        → WebSocket upgrade rejected if Origin is not in the list
```

### IP Allowlist (`ip_whitelist.rs`)

- **`127.0.0.1` is always present and cannot be removed** (`REQUIRED_LOOPBACK_IP`).
- All loopback IPs (`is_loopback()`) are unconditionally permitted regardless of list contents.
- Supports individual IPv4/IPv6 addresses and CIDR ranges (powered by the `ipnet` crate).
- **Explicitly rejects:** `0.0.0.0`, `::`, `0.0.0.0/0`, `::/0`, and any `/0` prefix—allow-all wildcards are strictly prohibited.
- Client IP is extracted solely from Axum `ConnectInfo`—**`X-Forwarded-For` and other proxy headers are untrusted and ignored**.
- Wildcard or range syntax like `*` or `-` is not supported—use standard CIDR notation for ranges.

### Origin Allowlist (`config.security.allowed_origins`)

- Validates the browser page's `Origin` header (protocol + hostname + port).
- **Exact matching required**—`https://example.com` ≠ `https://example.com:443` ≠ `http://example.com`.
- Only validates the **connecting page's Origin**, not the domain hosting print asset files.
- Applied during the WebSocket handshake (`server.rs`, `ws_handler`).

## Network Binding

The server binds to `0.0.0.0:{port}`—all network interfaces. This enables LAN devices to reach the local agent. The `service.host` field in the configuration is a compatibility placeholder (`127.0.0.1`) and does not restrict the bind interface.

**Security Implication:** If the machine is connected to an untrusted network, the port is reachable. The IP allowlist serves as the primary line of defense—always keep it restricted to trusted addresses.

## Configuration Transfer Encryption (`crates/cli/src/config_transfer.rs`)

Config export and import use authenticated encryption to enable secure batch deployments across workstations.

### Cryptographic Parameters

| Parameter | Value |
|---|---|
| KDF | Argon2id v19 |
| Memory Cost | 19,456 KiB |
| Iterations | 2 |
| Parallelism | 1 |
| Cipher | AES-256-GCM |
| Key Length | 32 bytes |
| Salt | 16 bytes (random, `OsRng`) |
| Nonce | 12 bytes (random, `OsRng`) |
| Auth Tag | 16 bytes |
| Key Material | Zeroized after use |

### Encrypted Envelope Format

```json
{
  "format": "printbridge-config-encrypted",
  "version": 1,
  "crypto": {
    "kdf": "argon2id13",
    "memory_kib": 19456,
    "iterations": 2,
    "parallelism": 1,
    "cipher": "aes-256-gcm",
    "tag_bytes": 16,
    "salt": "<base64>",
    "nonce": "<base64>"
  },
  "payload": "<base64(ciphertext || tag)>"
}
```

### Export Process

1. Select config sections to include (port, origins, remote settings, etc.)—each controlled by a boolean flag (`ExportConfigOptions`).
2. Construct a `ConfigTransferPayload` containing only the selected fields.
3. Generate a random salt and nonce; derive the encryption key via Argon2id.
4. Encrypt the JSON payload using AES-256-GCM.
5. Write the envelope to disk (default filename: `printbridge-config.json`).
6. Compute the SHA-256 hash of the file contents for import verification.

Passwords may be empty—empty passwords still undergo the full encryption derivation pipeline. If the Authorization Token is included with an empty password, the UI requires explicit user confirmation.

### Import Process

1. Read the encrypted file and calculate its SHA-256 hash (for user verification).
2. Decrypt with the provided password.
3. **Preview**—Display a field-by-field diff of current values vs. new values (`ImportPreview`).
4. After user confirmation → **merge field-by-field** into the active configuration.
5. Only fields present in the file are overwritten; omitted fields retain current values.
6. Persist the `normalized()` configuration.

**Bearer Token Protection:** If the imported file's token field is missing, `null`, or an empty string, the existing token is preserved. Only non-empty strings overwrite the active token.

### Cross-Language Generation

ERP or automated deployment systems can generate importable config files independently without running PrintBridge. Reference implementations in `examples/config-transfer/` implement the exact same encryption specification:

| Language | Libraries |
|---|---|
| Node.js | `libsodium-wrappers-sumo` + Node `crypto` |
| PHP | `sodium_crypto_pwhash` + `openssl_encrypt` |
| Go | `golang.org/x/crypto/argon2` + `crypto/aes` |

All implementations share identical constants and cross-validated test vectors. Verify via:

```bash
pnpm verify:config-transfer-examples
```

## Trust Boundaries and Best Practices

- Add only **trusted business systems** to the Origin allowlist.
- Add only **trusted client IP / CIDR ranges** to the IP allowlist.
- Even though the service listens on the LAN, **never** expose the port directly to untrusted public networks.
- Enforce access control on the **business system side** regarding who can submit print tasks and which files are processed.
- **Do not** expose sensitive file URLs to untrusted pages—the Origin check validates only the connecting page, not asset download hosts.
- The `service.host` field is cosmetic—always treat the service as listening across all network interfaces.

## Source Code Reference

| Domain | File |
|---|---|
| IP Allowlist Logic | `crates/core/src/ip_whitelist.rs` |
| Config Encryption / Decryption | `crates/cli/src/config_transfer.rs` |
| WebSocket IP Middleware | `crates/runtime/src/server.rs` (`ip_whitelist_middleware`) |
| WebSocket Origin Validation | `crates/runtime/src/server.rs` (`ws_handler`) |
| Config Normalization (127.0.0.1, IP list) | `crates/core/src/config.rs` |
