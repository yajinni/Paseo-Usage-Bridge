# Paseo Usage Bridge

A standalone Windows and macOS desktop app for authenticating multiple OpenAI accounts, displaying Codex subscription usage, and optionally exposing sanitized usage data to Paseo over localhost.

## What it does

- Authenticates each OpenAI account independently through browser OAuth.
- Stores access and refresh tokens in Windows Credential Manager or macOS Keychain.
- Queries the primary ChatGPT Codex usage endpoint: `https://chatgpt.com/backend-api/wham/usage`.
- Displays session, weekly, code-review, plan, reset-time, and credit information when returned.
- Retains last-known-good usage and marks it stale during transient failures.
- Refreshes OAuth tokens under a per-account lock.
- Exposes a bearer-protected loopback API at `http://127.0.0.1:47831/v1/paseo-usage`.
- Runs from one installer; end users do not need Node.js, Rust, Python, Docker, or a separate CLI.

## Current status

This repository contains the first working implementation scaffold and UI. The React production build is validated in CI, and Windows/macOS jobs run Rust compilation checks.

OpenAI does not currently document a public quota API or third-party desktop OAuth registration flow for this use case. The app therefore uses the OAuth client and internal usage endpoint used by Codex clients. Those pieces are intentionally isolated so they can be updated without changing the interface or Paseo integration contract.

## Security model

- Passwords are never requested or handled by the app.
- OAuth tokens remain in the native operating-system credential store.
- Account metadata and cached usage are stored separately in the app data directory.
- The local API binds only to `127.0.0.1`.
- The local API requires a random bearer token stored in the native credential store.
- The local API never returns access tokens, refresh tokens, ID tokens, or raw OpenAI responses.
- There is no fallback usage endpoint.

## Development

### Prerequisites

- Node.js 22+
- Rust stable
- Windows: Microsoft C++ Build Tools and WebView2
- macOS: Xcode Command Line Tools

### Run the web interface

```bash
npm install
npm run dev
```

### Run the desktop app

```bash
npm install
npm run tauri:dev
```

### Validate

```bash
npm run check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

### Build an installer

```bash
npm run tauri:build
```

Tauri generates the platform-appropriate Windows or macOS bundle under `src-tauri/target/release/bundle`.

## Local API

### Health

```http
GET http://127.0.0.1:47831/v1/health
```

### Usage

```http
GET http://127.0.0.1:47831/v1/paseo-usage
Authorization: Bearer <token shown in the Integration screen>
```

Response contract:

```json
{
  "schemaVersion": 1,
  "generatedAt": "2026-07-13T12:00:00Z",
  "accounts": [
    {
      "id": "local-account-id",
      "label": "Personal Plus",
      "email": "person@example.com",
      "plan": "plus",
      "status": "available",
      "windows": [
        {
          "id": "session",
          "label": "Session",
          "usedPercent": 18,
          "remainingPercent": 82,
          "resetsAt": "2026-07-13T17:00:00Z",
          "windowSeconds": 18000
        }
      ],
      "creditsUsd": 0,
      "fetchedAt": "2026-07-13T12:00:00Z",
      "error": null
    }
  ]
}
```

## Repository structure

```text
src/                    React dashboard
src-tauri/src/oauth.rs  OpenAI OAuth and callback flow
src-tauri/src/usage.rs  Usage retrieval, token refresh, and stale cache behavior
src-tauri/src/store.rs  Metadata and native credential storage
src-tauri/src/bridge_api.rs  Versioned localhost API
```

## License

MIT
