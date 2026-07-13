# Implementation status

The first desktop implementation includes:

- Tauri 2 desktop shell for Windows and macOS
- original three-panel React interface
- multi-account OpenAI browser OAuth with PKCE and localhost callbacks
- native operating-system credential storage
- per-account token-refresh locking
- Codex usage retrieval through `/backend-api/wham/usage` only
- session, weekly, code-review, credit, plan, and reset-time normalization
- last-known-good usage with live, stale, and authentication-required states
- bearer-protected loopback API for optional Paseo integration
- tray behavior, single-instance behavior, and optional start at login

The OpenAI OAuth client and usage endpoint are internal Codex interfaces rather than documented third-party APIs. They are isolated in `src-tauri/src/oauth.rs` and `src-tauri/src/usage.rs` so maintenance does not spread through the interface or integration contract.
