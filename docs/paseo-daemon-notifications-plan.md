# Paseo Daemon Notifications and Usage Alerts

**Status:** Research complete; implementation not started  
**Target release:** `0.3.0`  
**Last updated:** 2026-07-13  
**Upstream research baseline:** `getpaseo/paseo` at commit `b4ab0d9db6e5668218e5aaa34f15ef3dd133e3ec`

This file is the implementation tracker for adding phone and desktop notifications to Paseo Usage Bridge. It replaces the earlier assumption that Windows toast interception should be the primary Paseo integration.

The preferred design is to connect directly to Paseo's local daemon and consume its structured agent and provider events. Windows Notification Center interception remains a fallback only.

## Product goals

At minimum, notify when:

- [ ] An agent stops coding and becomes idle.
- [ ] An agent finishes a task.
- [ ] An agent fails or is canceled.
- [ ] An agent needs permission or user attention.
- [ ] A model provider is approaching or has reached a usage limit.
- [ ] A model request fails because of a rate limit or exhausted quota.

Delivery channels:

- [ ] Native desktop notifications.
- [ ] Free phone notifications through ntfy.
- [ ] Independent toggles for each event type and delivery channel.

## Research findings

### Paseo architecture

Paseo runs a local daemon that owns agent processes and state. Desktop, mobile, web, and CLI clients connect to that daemon. The daemon is in `packages/server`; the shared wire protocol is in `packages/protocol`; the reusable client is in `packages/client`.

Relevant upstream paths:

- `packages/server/src/server/agent/agent-manager.ts`
- `packages/server/src/server/websocket-server.ts`
- `packages/server/src/services/quota-fetcher/`
- `packages/protocol/src/messages.ts`
- `packages/protocol/src/agent-lifecycle.ts`
- `packages/protocol/src/agent-attention-notification.ts`
- `packages/client/src/daemon-client.ts`
- `packages/cli/src/utils/client.ts`

The default TCP endpoint is `localhost:6767`, with WebSocket path `/ws`. Paseo can also use a configured TCP address, Unix socket, or Windows named pipe. The CLI discovers the endpoint from `PASEO_HOST`, `PASEO_LISTEN`, Paseo's PID file/config, and finally the default endpoint.

A client opens the WebSocket and sends a JSON hello message with:

```json
{
  "type": "hello",
  "clientId": "stable-client-id",
  "clientType": "cli",
  "protocolVersion": 1,
  "capabilities": {}
}
```

When a daemon password is configured, Paseo accepts bearer authentication. The bridge must store a manually supplied password in the operating-system credential store and must not expose it to the frontend after saving.

### Agent lifecycle and task events

Paseo's lifecycle states are:

```text
initializing
idle
running
error
closed
```

The daemon also emits structured stream events:

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

`attention_required` contains one of these reasons:

```text
finished
error
permission
```

It can also contain Paseo's already-generated notification title/body and identifiers for the server, agent, and reason.

Agent snapshots expose useful non-content metadata:

```text
agent ID
title
provider
model
workspace ID
working directory
created/updated timestamps
last user-message timestamp
lifecycle status
pending permission count
last error
requires-attention flag
attention reason/timestamp
provider-unavailable flag
last token/cost/context-window usage
```

### Provider usage

Paseo already has a normalized provider-usage service. It is fetch-on-demand rather than a push subscription. The client sends:

```json
{
  "type": "provider.usage.list.request",
  "requestId": "unique-request-id"
}
```

The response contains provider-neutral data:

```text
provider ID and display name
available/unavailable/error status
plan label
source label
fetch/next-refresh timestamps
usage windows
balances
provider-specific detail rows
error
```

A usage window can contain:

```text
used percentage
remaining percentage
reset time
predicted run-out time
shortfall percentage
warning tone
```

Paseo currently registers usage fetchers for:

```text
Claude
Codex
GitHub Copilot
Cursor
Z.AI
Grok
Kimi
MiniMax
```

The daemon caches usage for five minutes. Polling more frequently than every five minutes provides little value and may cause unnecessary provider requests.

### Existing Paseo notification behavior

Paseo already knows when an agent requires attention and builds notifications for:

```text
Agent finished
Agent needs attention
Agent needs permission
```

The daemon's internal callback distinguishes `finished`, `error`, and `permission`. Therefore, Paseo Usage Bridge should consume the structured daemon event rather than trying to rediscover these conditions from visible Windows notifications.

## Event definitions for Paseo Usage Bridge

### 1. Agent stopped coding

**Purpose:** Tell the user that active model work has stopped, even when Paseo does not classify the whole task as finished.

Primary trigger:

```text
turn_completed
```

State-transition fallback:

```text
running -> idle
```

Failure/cancel variants:

```text
turn_failed
turn_canceled
running -> error
running -> closed
```

Rules:

- [ ] Emit only after the agent was observed running in the current bridge session.
- [ ] Do not notify for agents that were already idle when the bridge connected.
- [ ] Debounce duplicate `turn_completed` and `running -> idle` signals into one event.
- [ ] Default wording must not include prompt or response text.

Suggested notification:

```text
Title: Agent stopped coding
Body: <agent title> is now idle in <workspace>.
```

### 2. Task finished

Primary trigger:

```text
attention_required(reason = finished)
```

Fallback trigger when an older daemon lacks the attention event:

```text
turn_completed plus requiresAttention = true and attentionReason = finished
```

Rules:

- [ ] Treat this as separate from merely becoming idle.
- [ ] Use agent ID plus attention timestamp as the deduplication key.
- [ ] Ignore replayed/baseline attention state on first connection.
- [ ] Generic privacy mode must not forward Paseo's assistant-message preview.

Suggested notification:

```text
Title: Paseo task finished
Body: <agent title> finished in <workspace>.
```

### 3. Permission or attention required

Triggers:

```text
attention_required(reason = permission)
permission_requested
attention_required(reason = error)
```

Rules:

- [ ] Permission notifications are enabled by default.
- [ ] Do not include tool input, shell commands, code, paths, or permission metadata in generic mode.
- [ ] A resolved permission must clear the event's active state.

### 4. Provider usage is low

Primary source:

```text
provider.usage.list.response
```

Default remaining thresholds:

```text
25%
10%
0%
```

Rules:

- [ ] Poll every five minutes while connected.
- [ ] Alert only on a downward threshold crossing.
- [ ] Do not alert from unavailable/error data.
- [ ] Re-arm after the reset timestamp advances or remaining usage rises safely above the threshold.
- [ ] Dedupe by source, provider ID, window ID, threshold, and quota cycle.
- [ ] Support all generic windows and balances without provider-specific UI logic.

### 5. Immediate provider rate-limit failure

Primary source:

```text
turn_failed
```

Inspect only structured error fields:

```text
error
code
diagnostic
provider
model
```

Classify as a likely provider-limit event when the normalized fields indicate:

```text
HTTP 429
rate limit
quota exhausted
usage limit
capacity limit
retry-after
credits exhausted
```

Rules:

- [ ] Keep this classifier conservative.
- [ ] Preserve the original event as an agent failure even when it is also classified as a limit failure.
- [ ] Do not transmit full diagnostics to ntfy by default.
- [ ] Include provider/model name when available.

## Additional information available for future notifications

These are supported by Paseo's daemon/protocol but are outside the minimum release unless implementation is straightforward:

- [ ] Agent started or resumed.
- [ ] Agent process closed unexpectedly.
- [ ] Provider unavailable or provider catalog error.
- [ ] Context window nearing capacity from `lastUsage.contextWindowUsedTokens` and `contextWindowMaxTokens`.
- [ ] Cost threshold from `lastUsage.totalCostUsd`.
- [ ] Tool call failed or canceled.
- [ ] Worktree setup command failed, including exit code and duration.
- [ ] Compaction started/completed.
- [ ] Todo list completed.
- [ ] Delegated/sub-agent completed.
- [ ] Loop iteration completed, loop passed verification, loop exhausted iterations, or loop failed.
- [ ] Scheduled run completed or failed.
- [ ] GitHub pull-request checks finished, failed, or became mergeable.
- [ ] Terminal command/process requires attention where Paseo exposes structured terminal activity.

## Chosen integration architecture

```text
Paseo daemon WebSocket
        |
        v
PaseoDaemonConnector
        |
        +--> AgentStateTracker
        +--> ProviderUsagePoller
        +--> RateLimitClassifier
        |
        v
NotificationRouter
        |
        +--> Native desktop channel
        +--> ntfy phone channel
```

### Why direct daemon integration is preferred

- It receives exact lifecycle and attention reasons.
- It can create alerts Paseo does not currently surface as desktop toasts.
- It works even when the Paseo desktop window is hidden.
- It avoids inspecting unrelated Windows notifications.
- It can eventually support remote/headless Paseo daemons.
- It gives access to normalized provider usage and immediate model failures.

### Windows Notification Center fallback

Keep the Windows listener plan only as a compatibility fallback when:

- the daemon protocol cannot be reached,
- the installed Paseo version is too old,
- or a specific event exists only as a native toast.

Do not implement toast interception before the daemon connector spike is complete.

## Connection and discovery plan

- [ ] Create a stable bridge client ID and persist it locally.
- [ ] Try `PASEO_HOST` when explicitly configured.
- [ ] Discover Paseo home and read the daemon PID/config endpoint without modifying either file.
- [ ] Support `ws://<host>/ws` and `wss://<host>/ws` first.
- [ ] Add Windows named-pipe and Unix-socket support after the TCP implementation passes.
- [ ] Fall back to `ws://localhost:6767/ws`.
- [ ] Allow a manual host override in Settings.
- [ ] Allow an optional daemon password stored in the native keychain.
- [ ] Reconnect with bounded exponential backoff.
- [ ] Show connected, reconnecting, authentication-required, incompatible, and offline states.
- [ ] Confirm the server protocol/version before processing events.

The first implementation should use a small clean-room Rust WebSocket client. Do not copy or vendor Paseo's AGPL client source into this MIT repository. The bridge only needs a narrow subset of the public JSON protocol.

## Startup and replay safety

On each daemon connection:

1. Send the protocol hello.
2. Retrieve or observe the current agent snapshots.
3. Save current lifecycle and attention timestamps as the baseline.
4. Request current provider usage as the baseline.
5. Do not notify for baseline conditions.
6. Begin processing only newer state transitions and stream events.

Persist only deduplication state required to survive bridge restarts. Old tasks or notifications must not be resent after reboot.

## Privacy and data minimization

Paseo's stream can contain assistant text, reasoning, shell commands, file paths, code, and tool inputs. The bridge must not store or forward this content.

The daemon connector should parse only:

```text
event type
event timestamp
agent ID/title
workspace ID/name
provider/model
lifecycle transition
attention reason
permission ID/kind without input
usage totals and context-window counters
structured error code and a sanitized classification
provider usage windows/balances
```

Requirements:

- [ ] Ignore timeline content unless a future feature explicitly requires a non-content status field.
- [ ] Never persist prompts, assistant responses, reasoning, tool inputs, command output, code, or file contents.
- [ ] Never log raw WebSocket messages.
- [ ] Never send raw provider diagnostics to ntfy.
- [ ] Generic ntfy messages are the default.
- [ ] A separate explicit setting is required before including agent titles or workspace names.
- [ ] Unrelated daemon data is discarded immediately.

## Interaction with direct provider accounts

Paseo Usage Bridge already tracks independently authenticated provider accounts. Paseo daemon usage is an additional local source, not a replacement.

Source precedence:

1. Use direct bridge accounts for account-specific usage and reset times.
2. Use Paseo daemon usage for providers not configured directly in the bridge.
3. Always use Paseo runtime failures for immediate rate-limit alerts because they describe the agent that was interrupted.

Prevent duplicate alerts:

- [ ] Mark every alert with `source = direct_account` or `source = paseo_daemon`.
- [ ] Suppress a daemon threshold alert when a matching direct provider account already generated the same threshold/cycle alert.
- [ ] Do not merge two accounts merely because they use the same provider.
- [ ] When Paseo does not expose an account identifier, label the data as host-level/provider-level rather than account-level.

## ntfy delivery

- [ ] Store server URL, topic, and optional access token in the native credential store.
- [ ] Default server to `https://ntfy.sh`.
- [ ] Require HTTPS except for localhost/self-hosted development.
- [ ] Add a test-notification command.
- [ ] Add a bounded persistent retry outbox.
- [ ] Honor 429 retry timing.
- [ ] Never expose the saved topic/token through the localhost API.
- [ ] Mask configured secrets in the frontend.

## Settings UI

Add a dedicated **Notifications** section with:

### Paseo daemon

- [ ] Enable integration.
- [ ] Auto-discovered endpoint.
- [ ] Manual endpoint override.
- [ ] Optional password.
- [ ] Connection state and daemon version.
- [ ] Test connection.
- [ ] Last event received timestamp.

### Agent alerts

- [ ] Agent stopped coding.
- [ ] Task finished.
- [ ] Agent failed.
- [ ] Agent canceled.
- [ ] Permission required.
- [ ] Provider rate-limit failure.
- [ ] Include agent title toggle.
- [ ] Include workspace name toggle.

### Usage alerts

- [ ] Enable usage alerts.
- [ ] 25%, 10%, and 0% thresholds.
- [ ] Per-window toggles.
- [ ] Direct-account and Paseo-daemon source toggles.
- [ ] Per-provider mute controls.

### Delivery

- [ ] Native desktop notifications.
- [ ] ntfy phone notifications.
- [ ] Test notification.
- [ ] Last 50 sanitized delivery results.

## Implementation phases

### Phase 0 — daemon protocol spike

- [ ] Confirm the installed Paseo desktop daemon endpoint on Windows.
- [ ] Connect to `/ws` from a small Rust test client.
- [ ] Complete hello/authentication.
- [ ] Observe agent snapshot/update events.
- [ ] Observe `turn_started`, `turn_completed`, and `attention_required` from a real task.
- [ ] Request `provider.usage.list` and parse the response.
- [ ] Confirm behavior when the daemon restarts.
- [ ] Document exact messages from the installed version using sanitized fixtures.

**Stop condition:** Do not build the full notification UI until this spike proves that one real agent completion and one provider-usage response can be consumed without reading content fields.

### Phase 1 — notification foundation

- [ ] Add normalized notification event models.
- [ ] Add versioned settings and deduplication stores.
- [ ] Add native notification channel.
- [ ] Add ntfy channel and retry outbox.
- [ ] Add test-notification commands.

### Phase 2 — Paseo daemon connector

- [ ] Implement endpoint discovery and manual override.
- [ ] Implement WebSocket hello/auth/reconnect.
- [ ] Implement agent baseline and state tracker.
- [ ] Implement task-finished/idle/failure/permission events.
- [ ] Implement sanitized history.

### Phase 3 — provider-limit alerts

- [ ] Poll Paseo provider usage every five minutes.
- [ ] Add threshold crossing and quota-cycle deduplication.
- [ ] Add immediate rate-limit failure classifier.
- [ ] Reuse the same threshold engine for direct provider accounts.
- [ ] Add source precedence and duplicate suppression.

### Phase 4 — frontend and tray

- [ ] Add Notifications navigation page.
- [ ] Add connection/setup controls.
- [ ] Add event and privacy toggles.
- [ ] Add test buttons and delivery history.
- [ ] Add tray status and test action.

### Phase 5 — validation and release

- [ ] Unit-test lifecycle transitions and deduplication.
- [ ] Unit-test usage threshold crossings and reset re-arming.
- [ ] Unit-test rate-limit classification.
- [ ] Unit-test ntfy retry behavior and secret masking.
- [ ] Add sanitized Paseo protocol fixtures.
- [ ] Test against a real installed Windows Paseo daemon.
- [ ] Test daemon restart and bridge restart replay safety.
- [ ] Run frontend, Rust, Windows, and macOS validation.
- [ ] Bump all version locations to `0.3.0`.
- [ ] Publish signed updater artifacts only after validation passes.

## Expected repository changes

```text
src-tauri/src/notifications/
src-tauri/src/paseo/
src-tauri/src/model.rs
src-tauri/src/state.rs
src-tauri/src/store.rs
src-tauri/src/usage.rs
src-tauri/src/lib.rs
src-tauri/Cargo.toml
src-tauri/capabilities/default.json
src/App.tsx
src/api.ts
src/types.ts
src/notifications.css
README.md
.github/workflows/validate.yml
```

Suggested backend modules:

```text
src-tauri/src/paseo/discovery.rs
src-tauri/src/paseo/client.rs
src-tauri/src/paseo/protocol.rs
src-tauri/src/paseo/tracker.rs
src-tauri/src/paseo/rate_limits.rs
src-tauri/src/notifications/model.rs
src-tauri/src/notifications/settings.rs
src-tauri/src/notifications/router.rs
src-tauri/src/notifications/native.rs
src-tauri/src/notifications/ntfy.rs
src-tauri/src/notifications/outbox.rs
src-tauri/src/notifications/usage_alerts.rs
```

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

## Minimum acceptance criteria

- [ ] A real Paseo agent changing from active work to idle generates exactly one configured alert.
- [ ] A real `attention_required: finished` event generates exactly one task-finished alert.
- [ ] Permission and error events are distinguishable from normal completion.
- [ ] Provider usage crosses 25%, 10%, and 0% thresholds without repeated alerts in the same quota cycle.
- [ ] A runtime 429/rate-limit failure identifies the interrupted agent and provider without forwarding raw diagnostics.
- [ ] Existing tasks and low quotas do not create a notification burst when the bridge starts.
- [ ] Unrelated Paseo message content is neither stored nor logged.
- [ ] ntfy secrets remain in the operating-system credential store.
- [ ] The current localhost usage API remains backward compatible.
- [ ] Signed desktop updates continue to work.

## Upstream source map

Research was based on these Paseo source files:

- `README.md` — daemon/client architecture and default port.
- `packages/protocol/src/agent-lifecycle.ts` — lifecycle states.
- `packages/protocol/src/messages.ts` — agent stream, snapshots, provider usage, and request/response schemas.
- `packages/protocol/src/agent-attention-notification.ts` — finished/error/permission notification semantics.
- `packages/server/src/server/agent/agent-manager.ts` — state transitions, attention callback, and agent metadata.
- `packages/server/src/services/quota-fetcher/service.ts` — five-minute provider usage cache.
- `packages/server/src/services/quota-fetcher/manifest.ts` — registered usage providers.
- `packages/client/src/daemon-client.ts` — hello handshake, event model, and `listProviderUsage` request.
- `packages/cli/src/utils/client.ts` — daemon endpoint discovery and bearer authentication.
- `docs/providers.md` — provider usage fetch-on-demand contract.
