# Multi-provider usage and notification upgrades plan

## Goal

Maintain Paseo Usage Bridge as a standalone Windows and macOS app that:

1. Monitors subscription usage across multiple AI providers.
2. Connects read-only to the local Paseo daemon to observe agent lifecycle and provider events.
3. Creates reliable native and phone notifications for important coding and usage events.
4. Keeps credentials isolated in the operating-system credential store and exposes only normalized, sanitized data through the localhost API.

## Current status

Version `0.2.0` implements the multi-provider account and usage architecture described in the first part of this document. The next major upgrade is the notification and Paseo-daemon integration milestone, targeted as version `0.3.0`.

The direct Paseo daemon connection is now the preferred source for task notifications. Reading Windows notifications should remain a fallback only, because the daemon exposes richer and more precise lifecycle events than the operating-system notification surface.

---

# Part 1: Multi-provider usage integrations

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

## Original implementation sequence

1. Generalize models and keyring storage with backward-compatible migration.
2. Refactor OpenAI into the common provider refresh contract.
3. Add Anthropic OAuth login, refresh, profile lookup, and usage parsing.
4. Add Antigravity Google OAuth, project discovery, quota-summary parsing, and model-quota fallback.
5. Add OpenCode Go dashboard authentication and parser.
6. Add provider-aware frontend connection and dynamic usage presentation.
7. Extend bridge schema fields without removing existing fields.
8. Add parser and model tests, then run frontend checks and Windows/macOS Rust CI.

---

# Part 2: Notifications and Paseo daemon integration

## Research basis

This plan is based on the public `getpaseo/paseo` source tree at commit:

```text
b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec
```

Relevant upstream files include:

```text
packages/server/src/server/agent/agent-manager.ts
packages/server/src/server/websocket-server.ts
packages/server/src/services/quota-fetcher/service.ts
packages/server/src/services/quota-fetcher/manifest.ts
packages/client/src/daemon-client.ts
packages/protocol/src/messages.ts
packages/protocol/src/agent-lifecycle.ts
packages/protocol/src/agent-attention-notification.ts
docs/providers.md
```

The exact protocol must be rechecked against the installed Paseo version before implementation because Paseo is actively developed.

## Paseo backend findings

### Daemon architecture

Paseo runs a local daemon that owns agent processes, agent state, provider integrations, worktrees, terminal activity, schedules, loops, and its WebSocket API. The desktop, web, mobile, and CLI clients connect to this daemon rather than managing agents themselves.

The bridge should therefore connect to the daemon as a read-only client instead of scraping the desktop UI, notification center, logs, or internal databases.

### Agent lifecycle states

Paseo defines these lifecycle states:

```text
initializing
idle
running
error
closed
```

The daemon's `AgentManager` publishes agent-state and agent-stream events. Agent snapshots include useful fields such as:

```text
agent ID
provider
model
workspace ID
agent title
status
created/updated timestamps
last user-message timestamp
last usage
last error
pending permissions
requires attention
attention reason
attention timestamp
provider unavailable state
```

The bridge should consume only the fields needed for notification decisions. It must not persist prompts, assistant responses, code, terminal output, or full timeline records.

### Agent stream events

The shared protocol exposes events including:

```text
thread_started
turn_started
turn_completed
turn_failed
turn_canceled
timeline
permission_requested
permission_resolved
attention_required
```

`turn_completed` can include per-turn token and cost usage. `turn_failed` includes an error and may include a code or diagnostic. Timeline events can contain tool-call state, todos, errors, and compaction activity, but most timeline content is too sensitive and noisy for notification use.

### Existing attention semantics

Paseo already has explicit attention reasons:

```text
finished
error
permission
```

The `attention_required` event can include:

```text
provider
reason
timestamp
shouldNotify
notification title
notification body
server ID
agent ID
```

Agent snapshots also retain `requiresAttention`, `attentionReason`, and `attentionTimestamp`. These snapshot fields are useful after reconnecting because they provide the current durable state even when a live event was missed.

Paseo's built-in notification titles currently map roughly to:

```text
finished   -> Agent finished
error      -> Agent needs attention
permission -> Agent needs permission
```

This means the bridge does not need to guess task completion from toast text. It can consume the daemon's explicit reason.

### Provider usage inside Paseo

Paseo has a normalized provider-usage service with a five-minute cache. Provider usage is currently fetched on demand rather than pushed continuously. The current upstream manifest includes:

```text
Claude
Codex
Copilot
Cursor
Z.ai
Grok
Kimi
MiniMax
```

The generic provider response supports:

```text
provider ID and display name
availability status
plan label
usage windows
balances
provider-specific details
error state
```

The bridge's own connectors remain necessary for providers or account arrangements Paseo does not expose, including Google Antigravity and OpenCode Go. The bridge can also continue owning independent OpenAI and Anthropic accounts instead of relying on credentials from developer tools.

## Required notification events

The minimum required events are deliberately separated because they are not identical.

| Bridge event | Paseo source | Trigger | Default |
| --- | --- | --- | --- |
| `agent_stopped_coding` | `turn_completed`, `turn_failed`, `turn_canceled`, plus lifecycle transition | A foreground turn that was running is no longer running | On |
| `agent_task_finished` | `attention_required` with reason `finished`, or durable snapshot attention state | Paseo marks the agent's task/turn as finished and requiring attention | On |
| `agent_failed` | `turn_failed`, lifecycle `error`, or attention reason `error` | The agent stops because of an error | On |
| `agent_permission_required` | `permission_requested` or attention reason `permission` | Work is blocked waiting for user approval or an answer | Off initially |
| `provider_limit_warning` | Normalized provider usage windows | Remaining quota crosses a configured threshold | On |
| `provider_rate_limited` | `turn_failed`, provider notice, or diagnostic with a confirmed rate-limit signal | A live agent request is rejected because the provider limit was reached | On |
| `provider_unavailable` | Provider snapshot or usage status | Provider remains unavailable across repeated checks | Off initially |
| `daemon_disconnected` | WebSocket connection state | The bridge loses the Paseo daemon connection for a sustained period | Off initially |

### Definition: agent stopped coding

An agent has stopped coding when an active foreground turn ends for any reason:

```text
turn_completed
turn_failed
turn_canceled
```

A `running -> idle` transition is a supporting signal and reconnect fallback, not the only source of truth. The event must be deduplicated by daemon/server ID, agent ID, turn identity when available, event type, and timestamp.

A completed turn does not always mean the larger user goal is complete. Therefore `agent_stopped_coding` and `agent_task_finished` must remain separate settings.

### Definition: task finished

The strongest source is:

```text
attention_required.reason == "finished"
```

On reconnect, the bridge should also inspect agents whose snapshot has:

```text
requiresAttention == true
attentionReason == "finished"
```

The bridge should establish a baseline on first connection and must not send old completion alerts for agents that were already waiting before the bridge started.

### Definition: provider hitting limits

There are two distinct cases.

#### Predictive quota warning

A provider usage window crosses a remaining threshold, such as:

```text
25%
10%
0%
```

This uses normalized usage from:

1. Accounts connected directly to Paseo Usage Bridge.
2. Paseo daemon provider-usage responses for additional providers the bridge does not own.

#### Active rate-limit failure

A running agent receives a confirmed provider-limit failure. Prefer structured error codes, HTTP status `429`, provider diagnostics, or explicit provider notices. Use text matching only as a tightly constrained fallback.

Do not classify generic network failures, authentication failures, model-not-found errors, or context-window errors as provider quota exhaustion.

## Other useful events available later

The daemon exposes enough information to add optional alerts for:

- Agent initialization failure.
- Agent waiting for permission or an answer.
- Agent cancellation or interruption.
- Worktree setup completion or failure.
- A tool call failing repeatedly.
- Provider authentication becoming invalid.
- Provider process becoming unavailable.
- Daemon restart or disconnect.
- Scheduled run completion or failure.
- Loop completion, verification failure, or stop condition.
- Long-running agent with no stream activity for a configurable interval.
- Context-window usage approaching the model limit.
- A provider/model changing unexpectedly during a run.

These should not be enabled until their event semantics are verified and false-positive behavior is tested.

## Integration decision

### Primary: direct daemon WebSocket client

Implement a small read-only Paseo daemon client in Rust. Do not embed or copy the entire TypeScript client. Implement only the versioned protocol messages needed for:

```text
connection/authentication
server identity and version
initial agent directory snapshot
agent updates
agent stream events
provider usage list request/response
provider snapshot updates
reconnect and resynchronization
```

The client should use official protocol schemas and behavior as the reference, but all received data must be parsed defensively.

### Fallback: Windows notification listener

Windows notification interception remains an optional fallback when direct daemon access is impossible. It is not the preferred implementation because it:

- Loses lifecycle detail.
- Cannot reliably distinguish a stopped turn from a finished task.
- Requires additional Windows package capabilities and permission.
- Does not work as a common cross-platform design.
- Cannot create alerts for events Paseo does not already turn into operating-system notifications.

Do not implement Windows notification interception until the direct daemon integration has been attempted and documented as insufficient.

## Connection and authentication rules

The integration must not silently search for or read Paseo passwords, pairing secrets, or remote relay keys from Paseo configuration files.

The user configures:

```text
Daemon URL
Optional password or authorization value
Connection enabled/disabled
Reconnect behavior
```

Sensitive connection values must be stored in Windows Credential Manager or macOS Keychain. The React frontend may receive only masked configuration state.

The daemon connection must be read-only. Do not send agent prompts, permission responses, lifecycle commands, terminal input, file requests, git commands, worktree commands, or configuration changes.

## Initial protocol discovery checkpoint

Before writing the production daemon client, confirm the following against the current Paseo source and an installed local daemon:

1. Exact local WebSocket endpoint derived by the official desktop/client code.
2. Authentication handshake and supported authorization fields.
3. Required client ID, client type, version, and capability declarations.
4. Initial agent-list or directory-subscription request.
5. Agent snapshot/update event envelope.
6. Agent stream event envelope and turn identifiers.
7. Provider-usage request and response message names.
8. Provider snapshot update behavior.
9. Server-version compatibility behavior.
10. Reconnect behavior and whether subscriptions must be recreated.
11. Whether a read-only client can avoid advertising write-oriented capabilities.
12. Whether multiple local clients can connect without affecting desktop notifications.

Document the verified message examples in a fixture file with all personal paths, titles, prompts, and identifiers sanitized.

### Discovery stop condition

Do not begin the notification UI until a prototype can:

1. Connect to a local Paseo daemon.
2. Receive an initial agent snapshot.
3. Observe one `turn_started` event.
4. Observe the corresponding completed, failed, or canceled event.
5. Observe an `attention_required` event or its durable snapshot equivalent.
6. Request normalized provider usage without mutating Paseo state.
7. Reconnect without replaying old events as new alerts.

## Proposed backend structure

```text
src-tauri/src/paseo/
    mod.rs
    client.rs
    protocol.rs
    connection.rs
    events.rs
    classifier.rs

src-tauri/src/notifications/
    mod.rs
    model.rs
    settings.rs
    router.rs
    dedupe.rs
    history.rs
    ntfy.rs
    native.rs
    usage_alerts.rs
```

### Normalized event model

```text
NotificationEvent
    id
    source
    kind
    severity
    title
    sanitized body
    created timestamp
    daemon/server ID
    agent ID when applicable
    provider when applicable
    model when applicable
    workspace ID when applicable
    dedupe key
    privacy level
```

Do not include raw prompts, assistant messages, tool input/output, diffs, code, environment variables, or terminal output in the normalized event.

## Event processing and deduplication

### Startup baseline

On first connection or after settings are enabled:

1. Load the current agent snapshot.
2. Record current lifecycle and attention states.
3. Do not notify for states that already existed.
4. Begin notifying only for later transitions.

### Reconnect baseline

On reconnect:

1. Compare the new snapshot with the last persisted minimal state.
2. Use attention timestamps and updated timestamps to identify genuinely new states.
3. Never replay every currently idle or attention-requiring agent.
4. Expire dedupe records after a bounded retention period.

### Suggested dedupe keys

```text
paseo:<server-id>:<agent-id>:turn-stopped:<turn-id-or-timestamp>
paseo:<server-id>:<agent-id>:finished:<attention-timestamp>
paseo:<server-id>:<agent-id>:permission:<request-id>
paseo:<server-id>:<agent-id>:error:<event-timestamp>
usage:<source>:<account-or-provider>:<window>:<threshold>:<reset-cycle>
rate-limit:<server-id>:<agent-id>:<provider>:<turn-id-or-timestamp>
```

## Provider usage source priority

The bridge may receive usage data from both its own provider connectors and the Paseo daemon. Avoid duplicate cards and duplicate alerts.

Use this priority:

1. A bridge-owned account connection is authoritative for that exact account.
2. Paseo daemon provider usage supplements providers not connected directly in the bridge.
3. Never merge two accounts only because they use the same provider name.
4. Merge only when a stable provider account identity is available and matches.
5. Preserve the original source label in every snapshot and alert decision.

For providers where Paseo exposes only a machine-level CLI account and the bridge has a separate independently authenticated account, display them as separate sources unless identity equivalence is proven.

## Usage alert rules

Default thresholds:

```text
25% remaining
10% remaining
0% remaining
```

Generate an alert only on a downward crossing:

```text
previous remaining > threshold
new remaining <= threshold
```

Do not send repeated alerts while the value remains below the same threshold.

Re-arm a threshold when:

- The reset timestamp changes to a new quota cycle, or
- Remaining usage rises at least five percentage points above the threshold.

Never generate a threshold alert from stale or unavailable data. A failed refresh is not equivalent to zero remaining.

When usage alerts are first enabled, store the current values as the baseline and do not send a burst of historical low-usage alerts.

## Phone delivery through ntfy

ntfy is the initial free phone-notification channel.

Store in the native credential store:

```text
private topic
optional access token
optional username/password
```

Store non-secret configuration separately:

```text
server URL
enabled state
privacy mode
thresholds
event toggles
```

Delivery requirements:

- Send from the Rust backend, not React.
- Require HTTPS except for loopback/self-hosted development addresses.
- Provide a test-notification command.
- Use a bounded persistent outbox.
- Retry transient failures with backoff.
- Honor `Retry-After` for HTTP `429`.
- Do not indefinitely retry authentication or validation failures.
- Never log the topic, token, password, or complete request URL.

## Native desktop notifications

Add native notifications for bridge-generated events such as usage warnings and provider errors.

Do not create a duplicate local notification for a Paseo event when Paseo itself already displayed the same desktop notification. The settings should allow separate control of:

```text
native usage alerts
phone usage alerts
phone Paseo task alerts
native Paseo task alerts only when Paseo did not already notify
```

## Privacy modes

### Generic, default

```text
Agent stopped coding
A Paseo agent is waiting for you.
```

```text
Task finished
A Paseo task completed on your computer.
```

### Include agent title

```text
Task finished
“Fix scheduling validation” completed.
```

### Detailed

May include a sanitized provider, model, workspace label, and short final preview only after an explicit opt-in warning.

Never include by default:

- Prompts.
- Assistant response text.
- Code or diffs.
- Tool input/output.
- Terminal output.
- Absolute filesystem paths.
- Environment variables.
- API keys or credentials.
- Git URLs containing credentials.

## Settings interface

Add a dedicated `Notifications` section with:

### Paseo daemon

```text
Enable Paseo connection
Daemon URL
Masked authentication status
Connection state
Server version
Last connected time
Test connection
```

### Paseo events

```text
Agent stopped coding
Task finished
Agent failed
Permission required
Provider rate-limited
Daemon disconnected
Privacy mode
```

### Usage limits

```text
Enable usage alerts
25% threshold
10% threshold
Exhausted threshold
Session windows
Weekly windows
Monthly windows
Model-specific windows
Per-provider/account mute
```

### Delivery

```text
Native desktop notifications
Enable ntfy
ntfy server URL
Masked topic status
Masked authentication status
Send test notification
Last delivery state
```

### History

Keep only the latest bounded set of sanitized delivery records:

```text
timestamp
event kind
sanitized title
destination
delivered or failed
```

Do not store full message bodies or secrets in history.

## Implementation phases

### Phase 0: protocol and compatibility spike

- Verify the installed Paseo daemon protocol.
- Capture sanitized fixtures.
- Prove read-only connection, lifecycle events, provider usage request, and reconnect behavior.
- Decide the minimum supported Paseo version.

### Phase 1: read-only Paseo daemon connector

- Add secure connection settings.
- Add WebSocket connection, authentication, reconnection, and initial synchronization.
- Parse only required messages.
- Expose connection status to the frontend.

### Phase 2: event classifier and state engine

- Normalize turn, attention, permission, error, and provider events.
- Add startup baselines and reconnect reconciliation.
- Add persistent bounded deduplication.
- Unit-test lifecycle transitions thoroughly.

### Phase 3: usage threshold engine

- Evaluate bridge-owned provider snapshots.
- Request optional Paseo daemon provider usage.
- Apply source priority and account separation.
- Add threshold crossing and reset-cycle behavior.

### Phase 4: notification delivery

- Add native notifications.
- Add secure ntfy configuration.
- Add delivery retries, outbox, test command, and sanitized history.

### Phase 5: frontend and tray controls

- Add the Notifications section.
- Add event toggles, privacy modes, connection status, threshold controls, and delivery status.
- Extend the tray menu with notification state and a test command.

### Phase 6: optional fallback research

- Reassess Windows notification interception only when direct daemon integration leaves a documented gap.
- Do not make Windows notification scraping a dependency of the core feature.

## Test requirements

### Daemon protocol

- Valid handshake fixture succeeds.
- Unsupported server versions fail clearly.
- Invalid or malformed messages do not crash the app.
- Reconnection restores subscriptions.
- Old snapshots do not create new alerts.
- No write-oriented commands are sent.

### Agent events

- `turn_started -> turn_completed` creates one stopped-coding event.
- `turn_started -> turn_failed` creates one stopped-coding and one failure event without duplicates.
- `turn_started -> turn_canceled` creates the configured cancellation/stopped event.
- `attention_required: finished` creates one task-finished event.
- Durable finished attention after first baseline does not alert until a newer timestamp appears.
- Permission requests dedupe by request ID.
- Internal or hidden agents are ignored when the protocol marks them as non-user-facing.

### Provider limits

- 25%, 10%, and 0% crossings alert once per reset cycle.
- Stale and unavailable usage never alerts as exhausted.
- Bridge and daemon usage do not create duplicate alerts for the same proven account/window.
- Different accounts on the same provider remain isolated.
- Structured rate-limit failures classify correctly.
- Authentication and network failures do not classify as quota exhaustion.

### Privacy

- Generic mode contains no agent title, path, prompt, or response.
- Detailed mode still strips code, secrets, terminal output, and absolute paths.
- ntfy secrets never appear in logs, settings JSON, frontend state, history, or localhost API responses.

### Delivery

- Successful ntfy delivery removes the outbox item.
- Transient failure retries with bounded backoff.
- HTTP `429` honors `Retry-After`.
- Authentication failures stop retrying.
- The outbox and history are size-bounded.

## Validation

Required before calling implementation complete:

```bash
npm run check
npm run test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Run Rust validation on both Windows and macOS. Provider network calls and a live Paseo daemon cannot be fully exercised in CI without a controlled runtime, so parser, protocol, state-transition, migration, privacy, and retry behavior must use sanitized fixtures and unit tests.

## Release acceptance criteria

Version `0.3.0` is ready only when:

1. The bridge connects read-only to a supported local Paseo daemon.
2. A completed foreground turn produces exactly one `agent_stopped_coding` event.
3. Paseo's explicit finished attention state produces exactly one `agent_task_finished` event.
4. Agent errors and confirmed provider rate limits are distinguished correctly.
5. Provider usage thresholds produce one alert per threshold and reset cycle.
6. Existing OpenAI, Anthropic, Antigravity, and OpenCode Go usage monitoring still works.
7. Paseo daemon usage can supplement additional providers without duplicate account alerts.
8. Native and ntfy delivery work while the main app window is hidden in the tray.
9. Reconnects and application restarts do not resend old events.
10. No prompts, code, terminal output, secrets, or raw provider payloads are persisted or transmitted by default.
11. The existing localhost usage API remains backward-compatible.
12. Signed installers and updater artifacts continue to work on Windows and macOS.
