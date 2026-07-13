# Multi-provider usage integrations plan

## Goal

Extend Paseo Usage Bridge from OpenAI Codex-only monitoring to four provider connectors while keeping credentials isolated in the operating-system credential store and exposing only normalized, sanitized usage data through the localhost API.

## Providers and sources

| Provider | Authentication | Usage source | Stability label |
| --- | --- | --- | --- |
| OpenAI Codex | Browser OAuth with PKCE | `https://chatgpt.com/backend-api/wham/usage` | Undocumented provider endpoint |
| Anthropic Claude | Browser OAuth with PKCE using the Claude Code public client | `https://api.anthropic.com/api/oauth/usage` | Undocumented provider endpoint |
| Google Antigravity | Browser Google OAuth with offline refresh token | Internal Cloud Code `loadCodeAssist`, `retrieveUserQuotaSummary`, and `fetchAvailableModels` endpoints | Experimental internal API |
| OpenCode Go | OpenCode console workspace ID plus `auth` session cookie | Server-rendered `/workspace/<id>/go` dashboard | Experimental dashboard integration |

## Architecture

1. Add a provider discriminator to every account while preserving legacy OpenAI account metadata.
2. Replace the single OAuth-secret payload with a tagged provider-secret payload. Legacy OpenAI keychain entries must continue to load.
3. Keep provider network and parsing logic in separate Rust modules behind a common result/error contract.
4. Route account refreshes by provider, normalize all provider windows to `UsageWindow`, and retain last-known-good snapshots during transient failures.
5. Add provider-aware browser OAuth for OpenAI, Anthropic, and Antigravity.
6. Add a manual OpenCode Go connector that stores the workspace ID and console cookie only in the native credential store.
7. Update the React account flow so the user selects a provider and sees provider-specific connection instructions.
8. Update the dashboard, inspector, account list, and localhost response to include provider and source information without exposing credentials or raw responses.

## Security requirements

- Never write access tokens, refresh tokens, ID tokens, Google client credentials, or OpenCode cookies to logs or `accounts.json`.
- Store all provider secrets in Windows Credential Manager or macOS Keychain.
- Validate OAuth state on every callback and use PKCE where the provider supports it.
- Bind OAuth callbacks and the bridge API to loopback only.
- Limit OpenCode Go access to the read-only dashboard request.
- Do not perform inference requests to probe quota.
- Do not return provider secrets or raw provider payloads through Tauri commands or the localhost API.
- Keep successful cached usage visible and mark it stale when an undocumented endpoint changes or becomes unavailable.

## Implementation sequence

1. Generalize models and keyring storage with backward-compatible migration.
2. Refactor OpenAI into the common provider refresh contract.
3. Add Anthropic OAuth login, refresh, profile lookup, and usage parsing.
4. Add Antigravity Google OAuth, project discovery, quota-summary parsing, and model-quota fallback.
5. Add OpenCode Go dashboard authentication and parser.
6. Add provider-aware frontend connection and dynamic usage presentation.
7. Extend bridge schema fields without removing existing fields.
8. Add parser and model tests, then run frontend checks and Windows/macOS Rust CI.

## Validation

Required before calling the implementation complete:

```bash
npm run check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

Provider network calls cannot be exercised in CI without real user credentials. Unit tests therefore cover normalization, migration, and parser behavior, while authentication and live quota checks remain explicit runtime validation steps.