# Notifications and Paseo Daemon Integration

Status: **Planned / research complete, implementation not started**  
Target release: **v0.3.0**  
Last updated: **2026-07-13**  
Paseo upstream reviewed: `getpaseo/paseo` at commit `b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec`

## Purpose

Add a unified notification system to Paseo Usage Bridge that can notify the user locally and on their phone about:

1. An agent stopping work.
2. An agent finishing its task.
3. An agent failing or waiting for permission/input.
4. A model provider approaching or reaching a usage limit.
5. A connected provider account crossing configured remaining-usage thresholds.

The preferred source for Paseo events is the **Paseo daemon protocol**, not Windows Notification Center. The daemon already owns agent lifecycle, turn events, provider usage data, task-attention state, and push-notification decisions. Reading those events directly is richer, more reliable, cross-platform, and lets this app create useful notifications Paseo does not currently emit by default.

Windows notification interception remains a fallback only if direct daemon integration proves impossible for a required event.

---

## High-level architecture

```text
Paseo daemon WebSocket ──────┐
                             │
Paseo provider-usage RPC ────┼── Event normalization ── Notification router
                             │                         ├── Native desktop
Bridge provider refreshes ───┘                         └── ntfy phone delivery
```

The notification router must receive normalized events. Paseo events, provider usage alerts, and future sources must not each implement their own delivery, retry, history, privacy, and deduplication logic.

---

## Paseo backend findings

### Daemon and clients

Paseo is a local client-server system. Its Node.js daemon manages the coding-agent processes. The Electron desktop app, mobile app, web app, and CLI connect to that daemon over a shared WebSocket protocol.

Relevant upstream areas:

- `packages/server`: daemon, agent orchestration, storage, provider adapters, WebSocket server.
- `packages/protocol`: authoritative wire schemas and shared event types.
- `packages/client`: reusable daemon WebSocket client.
- `packages/desktop`: Electron wrapper that launches and manages the daemon.
- `packages/app`: shared frontend used by desktop/mobile/web.

Primary references:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/docs/architecture.md>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/protocol/src/messages.ts>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/server/src/server/agent/agent-manager.ts>

### Agent lifecycle state

Paseo has these authoritative lifecycle states:

```text
initializing
idle
running
error
closed
```

An agent snapshot also exposes:

- Agent ID.
- Provider.
- Model.
- Working directory.
- Workspace ID.
- Title and labels.
- Created, updated, and last-user-message timestamps.
- Runtime session information.
- Last turn usage.
- Last error.
- Pending permissions.
- Whether attention is required.
- Attention reason and timestamp.
- Provider-unavailable state.

Primary references:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/protocol/src/agent-lifecycle.ts>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/protocol/src/messages.ts>

### Agent stream events

The daemon protocol already exposes the events needed for reliable notifications:

| Paseo event | Meaning | Proposed bridge event |
| --- | --- | --- |
| `turn_started` | Model began a foreground turn | `agent_started_working` |
| `turn_completed` | Model stopped normally; optional usage included | `agent_stopped_working` |
| `turn_failed` | Turn stopped because of an error; includes error and optional code/diagnostic | `agent_failed` or `provider_limit_hit` |
| `turn_canceled` | User/system canceled the turn | `agent_canceled` |
| `permission_requested` | Agent cannot continue without permission/input | `agent_needs_attention` |
| `attention_required: finished` | Paseo considers the agent finished and requiring user attention | `task_finished` |
| `attention_required: error` | Paseo considers the agent errored and requiring attention | `agent_failed` |
| `attention_required: permission` | Paseo considers the agent blocked on permission | `agent_needs_attention` |
| `agent_update` with `running -> idle` | State fallback indicating work stopped | fallback `agent_stopped_working` |
| `agent_update` with `running -> error` | State fallback indicating failure | fallback `agent_failed` |
| `agent_update` with `providerUnavailable` | Agent provider unavailable | `provider_unavailable` |

Paseo distinguishes a turn ending from the agent semantically finishing its assignment. That distinction should be preserved:

- **Agent stopped working:** the current turn ended (`turn_completed`, `turn_failed`, `turn_canceled`, or state transition fallback).
- **Task finished:** Paseo emitted `attention_required` with reason `finished`.

This lets users choose whether they want every stopped turn, only completed tasks, or both.

### Existing Paseo attention notifications

Paseo already normalizes three attention reasons:

```text
finished
error
permission
```

Its notification builder creates titles such as:

- `Agent finished`
- `Agent needs attention`
- `Agent needs permission`

For a finished task, Paseo may include a Markdown-stripped preview of the latest assistant message. The bridge must not forward that preview by default. Generic privacy mode should send only the event, agent title, provider, and optional workspace label.

Primary reference:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/protocol/src/agent-attention-notification.ts>

### Existing Paseo push delivery

Paseo already has an Expo push-notification path for registered mobile clients. The daemon decides whether a connected client should show a notification and otherwise may send an Expo push notification.

The bridge still adds value because it can:

- Deliver through ntfy without relying on the Paseo mobile app.
- Offer custom event and threshold controls.
- Notify on normal turn completion even when Paseo does not create a user-facing notification.
- Correlate runtime provider failures with subscription usage.
- Combine Paseo events with Antigravity and OpenCode Go usage tracked independently by this app.
- Keep a local delivery history and retry queue.

Primary references:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/server/src/server/push/notifications.ts>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/server/src/server/push/push-service.ts>

### Paseo persistent state

Paseo uses file-backed JSON under `$PASEO_HOME`, which defaults to `~/.paseo`. Agent records are stored under:

```text
$PASEO_HOME/agents/{sanitized-cwd}/{agentId}.json
```

Persisted agent records include status, title, provider, workspace, timestamps, last error, and attention state. These files can provide a read-only fallback when the daemon WebSocket is unavailable, but they do not provide the same immediate or detailed turn stream.

The bridge must never modify Paseo files.

Primary reference:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/docs/data-model.md>

### Provider usage inside Paseo

Paseo has a normalized `ProviderUsageService` and a `provider.usage.list.request` RPC. Usage is currently **fetch-on-demand**, not pushed by the daemon. The service caches results for five minutes and returns provider-independent windows, balances, details, status, and errors.

Current upstream fetchers are registered for:

- Claude.
- Codex.
- Copilot.
- Cursor.
- Z.AI.
- Grok.
- Kimi.
- MiniMax.

Paseo currently does not register provider-usage fetchers for Antigravity or OpenCode Go. Paseo Usage Bridge must continue using its own connectors for those providers and for independently authenticated accounts.

Primary references:

- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/server/src/services/quota-fetcher/service.ts>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/packages/server/src/services/quota-fetcher/manifest.ts>
- <https://github.com/getpaseo/paseo/blob/b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec/docs/providers.md>

### Runtime provider-limit failures

`turn_failed` includes:

```text
provider
error
optional code
optional diagnostic
```

The shared protocol does not currently define a universal structured `rate_limited` failure kind. Immediate provider-limit notifications will therefore require provider-aware classification of real `turn_failed` fixtures.

The classifier must be conservative:

- Prefer a structured provider error code when one exists.
- Match exact known provider error patterns only.
- Do not classify every timeout, authentication failure, network failure, or generic model error as a usage limit.
- Store the provider, normalized failure category, and timestamp—not the raw full diagnostic by default.
- Add fixtures captured from real Claude, Codex, OpenCode, Copilot, and other supported provider failures before enabling a matcher.

Proactive threshold alerts from actual usage windows remain the primary way to warn that a provider is **approaching** its limit. Runtime failure classification provides the immediate **limit hit now** signal.

---

## Minimum notification requirements

### 1. Agent stopped coding/working

Authoritative triggers, in order:

1. `turn_completed`.
2. `turn_failed`.
3. `turn_canceled`.
4. Fallback `agent_update` transition from `running` to `idle`, `error`, or `closed` when no matching turn-end event was observed.

Default behavior:

- Disabled by default because this may be noisy.
- User can enable normal completion, failure, and cancellation separately.
- Deduplicate the stream event and lifecycle fallback.
- Do not treat a temporary pause between tool calls as the agent stopping.

Suggested message:

```text
Agent stopped working
“Fix authentication tests” completed its current turn using Codex.
```

### 2. Task finished

Authoritative trigger:

```text
attention_required.reason == "finished"
```

Fallback:

- An agent snapshot changes to `requiresAttention: true` with `attentionReason: "finished"` and a newer `attentionTimestamp`.

Default behavior:

- Enabled by default.
- This is distinct from every `turn_completed` event.
- Generic privacy mode does not include assistant-response text.

Suggested message:

```text
Paseo task finished
“Fix authentication tests” is ready for review.
```

### 3. Agent failed or needs attention

Triggers:

- `turn_failed`.
- `attention_required.reason == "error"`.
- `permission_requested`.
- `attention_required.reason == "permission"`.
- `providerUnavailable` becoming true.

Default behavior:

- Failures enabled.
- Permission/input alerts enabled.
- Provider unavailable alerts enabled only after a short debounce or repeated failure to prevent transient noise.

### 4. Provider approaching a limit

Sources:

1. Usage windows tracked by Paseo Usage Bridge.
2. Optional normalized results from Paseo's `provider.usage.list.request` for providers/accounts not represented in the bridge.

Default thresholds:

```text
25% remaining
10% remaining
0% remaining
```

Rules:

- Alert only on a downward threshold crossing.
- One alert per account/provider, window, threshold, and quota cycle.
- Re-arm when reset time changes or usage rises at least five percentage points above the threshold.
- Never generate threshold alerts from stale or unavailable data.
- Never convert a failed usage fetch into 0% remaining.

### 5. Provider limit hit during a task

Source:

- Conservative classification of `turn_failed.error`, `turn_failed.code`, and `turn_failed.diagnostic`.

Result:

```text
Provider limit reached
The Codex agent “Fix authentication tests” stopped because its provider reported a usage limit.
```

The first release must label uncertain classifications as `possible_provider_limit` and allow users to disable them. A matcher becomes authoritative only after tests with a real provider fixture.

---

## Proposed Paseo daemon adapter

### Connection

Implement a read-only daemon WebSocket client in the Rust backend.

Initial discovery order:

1. User-configured daemon URL.
2. Local Paseo configuration under `$PASEO_HOME`.
3. Default local endpoint, normally `ws://127.0.0.1:6767` with the correct WebSocket path determined during the protocol spike.

Support:

- Local direct WebSocket connection first.
- Paseo password/auth header when configured.
- Automatic reconnect with bounded exponential backoff.
- A stable bridge client ID.
- A client type/capability set that does not claim mobile push support.
- Server identity tracking to prevent events from two daemons being merged.

Do not use the JavaScript `@getpaseo/client` package inside the frontend because that would expose daemon credentials and event data to the webview. The Rust backend should implement the smallest required protocol surface. A bundled sidecar is a fallback only if protocol compatibility proves unreasonable.

### Required protocol operations

The spike must identify and test the exact frames required to:

- Complete the `hello` handshake.
- Receive `server_info` and server ID.
- Obtain the initial agent list/snapshots.
- Subscribe to agent updates and streams.
- Receive `attention_required` events.
- Request `provider.usage.list` data.
- Recover state after reconnect without replaying old notifications.
- Authenticate to a password-protected daemon.

### Initial baseline and replay protection

On first connection or reconnect:

1. Load the current agent snapshots.
2. Record lifecycle, attention timestamp, and latest known sequence/epoch.
3. Do not notify for existing finished/error/permission state.
4. Begin emitting notifications only for newer transitions/events.
5. Persist a bounded dedupe checkpoint so restarting the bridge does not resend old notifications.

Suggested dedupe keys:

```text
paseo:{serverId}:{agentId}:turn:{epoch}:{seq}:{eventType}
paseo:{serverId}:{agentId}:attention:{reason}:{timestamp}
paseo:{serverId}:{agentId}:permission:{requestId}
```

### File-state fallback

When the daemon cannot be reached, optionally watch `$PASEO_HOME/agents/**/*.json` read-only.

The fallback may detect:

- `lastStatus` changes.
- `requiresAttention` changes.
- New `attentionReason`/`attentionTimestamp`.
- New `lastError`.
- Provider/model/title/workspace metadata.

Limitations must be visible in the UI:

- No exact `turn_started`, `turn_completed`, or `turn_canceled` stream.
- No provider-usage RPC.
- Possible delay from file-write timing.
- Less reliable deduplication.

Do not use log scraping as the default source.

---

## Normalized event model

```text
NotificationEvent
  id
  source                 paseo_daemon | bridge_usage | paseo_usage
  kind                   agent_started_working
                         agent_stopped_working
                         task_finished
                         agent_failed
                         agent_canceled
                         agent_needs_attention
                         provider_limit_warning
                         provider_limit_hit
                         provider_unavailable
  severity               info | warning | error
  createdAt
  dedupeKey
  serverId?
  agentId?
  taskTitle?
  workspaceId?
  workspaceLabel?
  provider?
  model?
  accountId?
  usageWindowId?
  threshold?
  resetAt?
  sanitizedSummary?
  confidence             authoritative | fallback | inferred
```

Raw prompts, assistant responses, code, terminal output, file contents, and complete provider diagnostics are not part of the persistent event model.

---

## Notification delivery

### Native desktop

Use the Tauri notification plugin for bridge-created notifications.

Do not create a second local toast merely because Paseo itself already displayed one. Direct daemon events should be routed according to user settings, and duplicate local display should be suppressed when detectable.

### ntfy phone delivery

Store the ntfy topic and optional token in Windows Credential Manager/macOS Keychain. Keep non-secret settings in a versioned atomic JSON file.

Requirements:

- Hosted `https://ntfy.sh` or custom self-hosted server.
- HTTPS required except explicit loopback development servers.
- Test-notification command.
- Persistent bounded outbox.
- Retry transient network failures.
- Honor `Retry-After` for throttling.
- Do not retry authentication/configuration errors indefinitely.
- Mask the topic in frontend responses.

### Privacy modes

`generic` — default:

```text
Paseo task finished
A coding task is ready for review.
```

`title`:

```text
Paseo task finished
“Fix authentication tests” is ready for review.
```

`preview` — explicit opt-in:

- May include Paseo's sanitized assistant preview.
- Show a warning that notification content is sent through the configured ntfy server.

Never include secrets, environment variables, raw terminal output, code excerpts, or absolute paths.

---

## Implementation phases and checklist

### Phase 0 — Baseline and protocol capture

- [ ] Confirm current `main` passes `npm run check` and `npm run build`.
- [ ] Confirm Rust tests/checks pass on Windows and macOS.
- [ ] Record installed Paseo version and daemon endpoint.
- [ ] Capture a sanitized WebSocket session for hello, agent list, one normal turn, finish, failure, and permission request.
- [ ] Confirm password-protected daemon authentication flow.
- [ ] Confirm the exact request/response shape for `provider.usage.list.request`.
- [ ] Document protocol compatibility assumptions and upstream version detection.

Stop if the bridge cannot connect read-only without modifying Paseo or exposing credentials to the frontend.

### Phase 1 — Notification foundation

- [ ] Add notification domain models.
- [ ] Add versioned notification settings store.
- [ ] Add native-keyring storage for ntfy credentials.
- [ ] Add persistent dedupe state.
- [ ] Add bounded delivery history.
- [ ] Add bounded retry outbox.
- [ ] Add notification router.
- [ ] Add ntfy channel and test command.
- [ ] Add native desktop notification channel.

### Phase 2 — Paseo daemon adapter

- [ ] Add daemon endpoint discovery/configuration.
- [ ] Implement WebSocket hello/server-info handshake.
- [ ] Implement authentication.
- [ ] Implement reconnect/backoff.
- [ ] Load initial agent snapshots without notifying.
- [ ] Subscribe to agent updates and streams.
- [ ] Normalize `turn_started/completed/failed/canceled`.
- [ ] Normalize finished/error/permission attention events.
- [ ] Normalize provider-unavailable transitions.
- [ ] Persist reconnect checkpoints.
- [ ] Add daemon status and last-event diagnostics to settings UI.

### Phase 3 — Paseo runtime notifications

- [ ] Add “agent stopped working” notification option.
- [ ] Add “task finished” notifications.
- [ ] Add task failure notifications.
- [ ] Add permission/input notifications.
- [ ] Add cancellation notifications.
- [ ] Add provider-unavailable debounce.
- [ ] Add per-event native/phone routing toggles.
- [ ] Add per-agent/internal-agent filtering.
- [ ] Do not notify for Paseo internal agents unless explicitly enabled.

### Phase 4 — Usage-limit notifications

- [ ] Hook threshold evaluation into successful bridge provider refreshes.
- [ ] Add 25%, 10%, and exhausted defaults.
- [ ] Add crossing-only logic and quota-cycle re-arming.
- [ ] Add per-account mute.
- [ ] Add session/weekly/monthly/model-window toggles.
- [ ] Request Paseo provider usage on a five-minute background cadence when enabled.
- [ ] Merge Paseo usage results by source without pretending they are the same account as independently connected bridge accounts.
- [ ] Add runtime `turn_failed` limit classifier framework.
- [ ] Capture and test real provider-limit fixtures before enabling each matcher.
- [ ] Mark inferred limit events with their confidence.

### Phase 5 — User interface

- [ ] Add a dedicated Notifications navigation page.
- [ ] Add ntfy setup, masked status, and test button.
- [ ] Add native-notification permission/status.
- [ ] Add Paseo daemon endpoint and connection status.
- [ ] Add event toggles and destination toggles.
- [ ] Add privacy mode.
- [ ] Add usage thresholds and window selection.
- [ ] Add per-account mute controls.
- [ ] Add latest 50 sanitized delivery-history rows.
- [ ] Add “send current usage summary.”
- [ ] Add tray menu notification status and test action.

### Phase 6 — Fallback and hardening

- [ ] Add optional read-only `$PASEO_HOME/agents` fallback.
- [ ] Show `live daemon`, `file fallback`, or `disconnected` source status.
- [ ] Add upstream protocol-version compatibility errors.
- [ ] Add malformed/unexpected-frame handling.
- [ ] Add privacy review for logs, persistence, and frontend IPC.
- [ ] Confirm no ntfy credential appears in logs or frontend snapshots.
- [ ] Confirm localhost bridge API remains backward-compatible.

### Phase 7 — Release

- [ ] Run all validation commands.
- [ ] Perform installed Windows end-to-end test with real Paseo.
- [ ] Perform installed macOS end-to-end test with real Paseo.
- [ ] Test offline ntfy queue/retry.
- [ ] Test bridge restart without duplicate notifications.
- [ ] Test Paseo daemon restart/reconnect.
- [ ] Test multiple concurrent agents.
- [ ] Test multiple Paseo daemon identities if supported.
- [ ] Bump all app versions to `0.3.0`.
- [ ] Build signed updater artifacts.
- [ ] Publish only after Windows and macOS validation passes.

---

## Test matrix

### Agent event tests

- [ ] `turn_started` creates one start event when enabled.
- [ ] `turn_completed` creates one stopped event.
- [ ] `turn_failed` creates failure and stopped semantics without duplicate phone messages.
- [ ] `turn_canceled` is distinguishable from successful completion.
- [ ] `attention_required: finished` creates one task-finished event.
- [ ] `turn_completed` followed by `attention_required: finished` follows the user's selected notification policy.
- [ ] Permission request and attention event deduplicate.
- [ ] Lifecycle fallback does not duplicate a stream event.
- [ ] Existing attention state at startup is not replayed.
- [ ] Reconnect replay is not resent.
- [ ] Internal agents are ignored by default.
- [ ] Concurrent agents remain isolated.

### Provider-limit tests

- [ ] Usage crossing 25% sends once.
- [ ] Remaining below 25% does not repeat.
- [ ] Crossing 10% sends a separate event.
- [ ] Reset timestamp change re-arms thresholds.
- [ ] Stale/unavailable data sends no threshold alert.
- [ ] Authentication failure is not misclassified as exhausted usage.
- [ ] Real structured rate-limit error is classified correctly.
- [ ] Generic network timeout is not classified as a rate limit.
- [ ] Provider-specific matcher tests are isolated.

### Delivery tests

- [ ] Native notification works in an installed package.
- [ ] ntfy test reaches the phone.
- [ ] Topic/token never reaches frontend state.
- [ ] Topic/token never appears in logs.
- [ ] Offline messages queue and later deliver.
- [ ] 401/403 stops retries and shows configuration error.
- [ ] 429 honors retry timing.
- [ ] Outbox and history stay within their size limits.

---

## Validation commands

```bash
npm install
npm run check
npm run test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Never claim a command passed unless it was actually run.

---

## Security and privacy rules

- Connect to the Paseo daemon read-only for observation/RPC requests needed by this feature.
- Do not send commands to agents or mutate tasks.
- Do not edit Paseo files.
- Do not expose daemon passwords, auth headers, ntfy topics, or tokens to the React frontend.
- Keep secrets in Windows Credential Manager or macOS Keychain.
- Bind any new local service only to loopback.
- Do not persist raw prompts, assistant messages, code, terminal output, or complete error diagnostics.
- Do not forward internal-agent content by default.
- Do not log raw WebSocket frames in production.
- Sanitize task titles before notification delivery.
- Keep the existing `/v1/paseo-usage` response contract backward-compatible.

---

## Open questions for the protocol spike

- [ ] What exact WebSocket URL/path does the bundled desktop daemon advertise on each platform?
- [ ] Can the daemon endpoint and password/auth mechanism be discovered safely from local configuration, or should the user enter them?
- [ ] Which initial agent-list request is best for a minimal third-party client?
- [ ] Are agent stream events replayed automatically after reconnect, or must the bridge explicitly resubscribe/fetch history?
- [ ] Which protocol/app version should the bridge advertise?
- [ ] Which client capabilities should be omitted to ensure Paseo still sends its own intended notifications correctly?
- [ ] Can one desktop installation manage more than one daemon endpoint?
- [ ] What real error/code/diagnostic payloads do Claude, Codex, OpenCode, Copilot, and Pi produce when limits are hit?
- [ ] Should provider usage from Paseo be shown in the dashboard or used only for notifications in v0.3.0?
- [ ] Can agent titles and workspace labels be fetched without requesting timeline contents?

---

## Acceptance criteria

The notification work is complete when:

1. A real Paseo `attention_required: finished` event reaches the phone exactly once.
2. A normal agent turn ending can optionally produce an “agent stopped working” alert.
3. A failed or canceled turn is not presented as a successful task finish.
4. Permission/input-required events reach the phone exactly once.
5. Existing or replayed events are not resent after bridge or daemon restart.
6. Provider usage crossing 25%, 10%, or 0% sends one alert per threshold and quota cycle.
7. A verified runtime rate-limit failure creates an immediate provider-limit notification.
8. Network/auth failures are not falsely labeled as quota exhaustion.
9. Antigravity and OpenCode Go continue to use the bridge's independent usage connectors.
10. No prompt, code, terminal output, path, or secret is transmitted in generic privacy mode.
11. ntfy credentials remain in the native credential store.
12. The current localhost usage API remains compatible.
13. Signed Windows and macOS update packages still build and install.
