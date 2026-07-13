# Repository guidance

This is a standalone Windows/macOS Tauri application. Keep account authentication, token refresh, quota retrieval, and secure storage inside the Rust backend. The React frontend must receive only sanitized account and usage data.

Before changing behavior:

1. Identify whether the change affects OAuth, credential storage, usage normalization, local API compatibility, or desktop packaging.
2. Keep the versioned local API backward-compatible whenever possible.
3. Never log or expose OAuth access tokens, refresh tokens, ID tokens, authorization codes, or raw OpenAI responses.
4. Do not add a secondary usage endpoint without an explicit product decision.
5. Do not introduce dependencies on external coding CLIs.

Validation:

```bash
npm run check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```
