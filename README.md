# Paseo Usage Bridge

[![Validate](https://github.com/yajinni/Paseo-Usage-Bridge/actions/workflows/validate.yml/badge.svg)](https://github.com/yajinni/Paseo-Usage-Bridge/actions/workflows/validate.yml)

A standalone Windows and macOS desktop app for monitoring AI subscription usage across OpenAI Codex, Anthropic Claude, Google Antigravity, and OpenCode Go. It can optionally expose normalized, sanitized usage data to Paseo over localhost.

## Supported providers

| Provider | Authentication | Usage source |
| --- | --- | --- |
| OpenAI Codex | Browser OAuth with PKCE | ChatGPT Codex `wham/usage` endpoint |
| Anthropic Claude | Browser OAuth with PKCE | Anthropic OAuth usage endpoint |
| Google Antigravity | Browser Google OAuth with offline refresh token | Internal Cloud Code quota APIs |
| OpenCode Go | Workspace ID and OpenCode console `auth` cookie | Server-rendered Go dashboard |

The Anthropic, Antigravity, and OpenCode Go integrations rely on provider interfaces that are not documented as stable third-party APIs. Each connector is isolated so it can be repaired without changing the dashboard or localhost response contract. Last-known-good results remain visible and are marked stale when a provider changes or temporarily rejects a request.

## What it does

- Authenticates OpenAI, Anthropic, and Antigravity accounts independently in the browser.
- Connects OpenCode Go through its exact server-rendered rolling, weekly, and monthly dashboard values.
- Stores OAuth tokens and OpenCode console cookies in Windows Credential Manager or macOS Keychain.
- Displays provider-reported usage percentages, remaining quota, reset times, plan information, and credits when available.
- Retains last-known-good usage and marks it stale during transient failures.
- Refreshes provider credentials under a per-account lock.
- Exposes a bearer-protected loopback API at `http://127.0.0.1:47831/v1/paseo-usage`.
- Checks GitHub Releases for signed updates at startup and every six hours.
- Runs from one installer; end users do not need Node.js, Rust, Python, Docker, OpenCode, Claude Code, or another CLI.

## Connecting providers

### OpenAI Codex

Choose **OpenAI Codex** in the Add Account window and complete the browser login. The app requests only the OAuth access needed to identify the account and read Codex subscription usage.

### Anthropic Claude

Choose **Anthropic Claude** and complete the browser login. The app reads the five-hour, seven-day, model-specific, and extra-usage windows returned by Anthropic's OAuth usage service.

### Google Antigravity

Choose **Google Antigravity** and complete the Google consent screen. The app keeps the offline refresh token in the native keychain, discovers the Cloud AI Companion project, and reads the account's quota-summary or model-quota response.

### OpenCode Go

OpenCode currently does not expose Go plan percentages through an API-key-authenticated usage endpoint. The connector therefore needs:

- The OpenCode workspace ID, such as `wrk_...`.
- The value of the `auth` cookie from the signed-in OpenCode console.

The cookie is stored only in the native credential store and is used only for a read-only request to `https://opencode.ai/workspace/<workspace-id>/go`. It is never written to `accounts.json`, returned to the React frontend after submission, logged, or exposed through the localhost API.

## Security model

- Passwords are never requested or handled by the app.
- OAuth tokens and OpenCode session cookies remain in the native operating-system credential store.
- Account metadata and cached usage are stored separately in the app data directory.
- OAuth callback listeners bind only to loopback.
- The local API binds only to `127.0.0.1`.
- The local API requires a random bearer token stored in the native credential store.
- The local API never returns access tokens, refresh tokens, ID tokens, session cookies, or raw provider responses.
- The app does not perform inference requests merely to probe usage limits.
- Application updates must pass Tauri signature verification before installation.
- The updater private key is stored only as a GitHub Actions repository secret.

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

Provider network calls require real user credentials and are not exercised in CI. Parser, migration, and normalization behavior is covered by unit tests; each live login flow must also be exercised in a packaged Windows or macOS build before release.

### Build an installer

```bash
npm run tauri:build
```

Tauri generates the platform-appropriate Windows or macOS bundle under `src-tauri/target/release/bundle`.

## Releases and automatic updates

The `Publish desktop release` workflow builds Windows, macOS Apple Silicon, and macOS Intel packages. It also uploads signed updater artifacts and a `latest.json` manifest to the GitHub Release.

The repository requires these Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`: the complete contents of the updater private-key file.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: optional; leave unset when the key has no password.

Never commit or share the private key. Keep a secure backup: losing it prevents installed copies from accepting future updates.

Every release must use a newer semantic version in all three locations:

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Version `0.1.1` was the first updater-enabled build. Version `0.2.0` adds the multi-provider account and usage architecture.

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
      "label": "Personal Claude",
      "provider": "anthropic",
      "email": "person@example.com",
      "providerAccountId": "provider-account-id",
      "plan": "max",
      "status": "available",
      "source": "anthropic_oauth_usage",
      "windows": [
        {
          "id": "five_hour",
          "label": "5 hour",
          "usedPercent": 18,
          "remainingPercent": 82,
          "resetsAt": "2026-07-13T17:00:00Z",
          "windowSeconds": 18000
        }
      ],
      "creditsUsd": null,
      "fetchedAt": "2026-07-13T12:00:00Z",
      "error": null
    }
  ]
}
```

## Repository structure

```text
src/                              React dashboard
src-tauri/src/oauth.rs            Provider browser OAuth and callback flows
src-tauri/src/providers/          Provider-specific usage clients and parsers
src-tauri/src/usage.rs            Common refresh, cache, and stale-state behavior
src-tauri/src/store.rs            Metadata and native credential storage
src-tauri/src/bridge_api.rs       Versioned localhost API

docs/provider-integrations-plan.md  Implementation and security plan
```

## License

MIT
