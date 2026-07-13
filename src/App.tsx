import { useCallback, useEffect, useMemo, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { bridgeApi } from "./api";
import { AccountRow } from "./components/AccountRow";
import { AddAccountModal } from "./components/AddAccountModal";
import { UsageBar } from "./components/UsageBar";
import {
  CopyIcon,
  EditIcon,
  GaugeIcon,
  LinkIcon,
  PlusIcon,
  RefreshIcon,
  SettingsIcon,
  ShieldIcon,
  TrashIcon,
  UsersIcon,
} from "./icons";
import type { Account, BridgeInfo, DashboardSnapshot, UsageWindow } from "./types";

type Section = "accounts" | "integration" | "settings";

function getWindow(account: Account | null, id: string): UsageWindow | null {
  return account?.lastUsage?.windows.find((window) => window.id === id) ?? null;
}

function formatTime(value: string | null | undefined): string {
  if (!value) return "Never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

function accountState(account: Account): { label: string; className: string } {
  if (account.authRequired) return { label: "Auth needed", className: "danger" };
  if (!account.lastUsage) return { label: "Not refreshed", className: "neutral" };
  if (account.lastUsage.freshness === "stale") return { label: "Stale", className: "warning" };
  return { label: "Live", className: "success" };
}

function minRemaining(accounts: Account[], id: string): number | null {
  const values = accounts
    .map((account) => getWindow(account, id)?.remainingPercent)
    .filter((value): value is number => typeof value === "number");
  return values.length ? Math.min(...values) : null;
}

function copy(value: string) {
  return navigator.clipboard.writeText(value);
}

export default function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [section, setSection] = useState<Section>("accounts");
  const [addOpen, setAddOpen] = useState(false);
  const [loginLabel, setLoginLabel] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);

  const load = useCallback(async () => {
    try {
      const next = await bridgeApi.snapshot();
      setSnapshot(next);
      setSelectedId((current) => current && next.accounts.some((account) => account.id === current) ? current : next.accounts[0]?.id ?? null);
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void load();
    void isEnabled().then(setAutostart).catch(() => setAutostart(false));
  }, [load]);

  const accounts = snapshot?.accounts ?? [];
  const selected = accounts.find((account) => account.id === selectedId) ?? null;
  const selectedState = selected ? accountState(selected) : null;
  const healthy = accounts.filter((account) => account.lastUsage?.freshness === "live" && !account.authRequired).length;
  const weeklyLow = minRemaining(accounts, "weekly");
  const sessionLow = minRemaining(accounts, "session");

  const refreshOne = async (id: string) => {
    setBusy(`refresh:${id}`);
    try {
      await bridgeApi.refreshAccount(id);
      await load();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  };

  const refreshAll = async () => {
    setBusy("refresh-all");
    try {
      await bridgeApi.refreshAll();
      await load();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  };

  const rename = async (account: Account) => {
    const label = window.prompt("Account label", account.label)?.trim();
    if (!label || label === account.label) return;
    setBusy(`rename:${account.id}`);
    try {
      await bridgeApi.renameAccount(account.id, label);
      await load();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (account: Account) => {
    if (!window.confirm(`Remove ${account.label}? This deletes its stored OAuth credentials from the operating-system credential store.`)) return;
    setBusy(`remove:${account.id}`);
    try {
      await bridgeApi.removeAccount(account.id);
      await load();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(null);
    }
  };

  const toggleAutostart = async () => {
    try {
      if (autostart) await disable();
      else await enable();
      setAutostart(await isEnabled());
    } catch (cause) {
      setError(String(cause));
    }
  };

  const content = useMemo(() => {
    if (section === "integration") {
      return <IntegrationView bridge={snapshot?.bridge ?? null} onRegenerate={async () => {
        setBusy("regenerate-token");
        try {
          const bridge = await bridgeApi.regenerateToken();
          setSnapshot((current) => current ? { ...current, bridge } : current);
        } catch (cause) {
          setError(String(cause));
        } finally {
          setBusy(null);
        }
      }} busy={busy === "regenerate-token"} />;
    }
    if (section === "settings") {
      return <SettingsView autostart={autostart} onToggleAutostart={toggleAutostart} />;
    }
    return (
      <AccountsView
        accounts={accounts}
        selected={selected}
        selectedState={selectedState}
        healthy={healthy}
        weeklyLow={weeklyLow}
        sessionLow={sessionLow}
        onAdd={() => { setLoginLabel(""); setAddOpen(true); }}
        onRefreshAll={refreshAll}
        onRefreshOne={refreshOne}
        onRename={rename}
        onRemove={remove}
        busy={busy}
      />
    );
  }, [section, snapshot?.bridge, busy, autostart, accounts, selected, selectedState, healthy, weeklyLow, sessionLow]);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark"><GaugeIcon /></span>
          <div><strong>Paseo Usage</strong><small>Bridge</small></div>
        </div>

        <nav className="primary-nav">
          <button className={section === "accounts" ? "active" : ""} onClick={() => setSection("accounts")}><UsersIcon />Accounts</button>
          <button className={section === "integration" ? "active" : ""} onClick={() => setSection("integration")}><LinkIcon />Integration</button>
          <button className={section === "settings" ? "active" : ""} onClick={() => setSection("settings")}><SettingsIcon />Settings</button>
        </nav>

        <div className="sidebar-section-title"><span>Codex accounts</span><button title="Add account" onClick={() => { setLoginLabel(""); setAddOpen(true); }}><PlusIcon /></button></div>
        <div className="account-list">
          {accounts.length ? accounts.map((account) => (
            <AccountRow key={account.id} account={account} selected={account.id === selectedId} onSelect={() => { setSelectedId(account.id); setSection("accounts"); }} />
          )) : <button className="empty-account" onClick={() => { setLoginLabel(""); setAddOpen(true); }}><PlusIcon /><span>Add your first account</span></button>}
        </div>

        <div className="sidebar-footer">
          <span className={`connection-dot ${snapshot?.bridge.running ? "online" : "offline"}`} />
          <div><strong>{snapshot?.bridge.running ? "Bridge online" : "Bridge offline"}</strong><small>{snapshot?.bridge.endpoint ?? "Starting…"}</small></div>
        </div>
      </aside>

      <main className="main-stage">
        {error ? <div className="global-error"><span>{error}</span><button onClick={() => setError(null)}>Dismiss</button></div> : null}
        {snapshot ? content : <div className="loading-screen"><span className="spinner" />Loading bridge…</div>}
      </main>

      <aside className="inspector">
        {selected ? (
          <>
            <div className="inspector-heading"><span className="account-avatar large">{selected.label.slice(0, 1).toUpperCase()}</span><div><strong>{selected.label}</strong><small>{selected.email ?? "OpenAI account"}</small></div></div>
            <div className="inspector-status"><span className={`status-pill ${selectedState?.className}`}>{selectedState?.label}</span><span className="plan-pill">{selected.plan ?? "Unknown plan"}</span></div>
            <dl className="detail-list">
              <div><dt>Last refreshed</dt><dd>{formatTime(selected.lastUsage?.fetchedAt)}</dd></div>
              <div><dt>Account ID</dt><dd className="truncate">{selected.chatgptAccountId ?? "Unavailable"}</dd></div>
              <div><dt>Credential storage</dt><dd>Native keychain</dd></div>
              <div><dt>Usage source</dt><dd>/wham/usage</dd></div>
            </dl>
            {selected.lastError ? <div className="inspector-error">{selected.lastError}</div> : null}
            <div className="inspector-actions">
              <button className="button primary full" disabled={busy === `refresh:${selected.id}`} onClick={() => void refreshOne(selected.id)}><RefreshIcon />{busy === `refresh:${selected.id}` ? "Refreshing…" : "Refresh usage"}</button>
              {selected.authRequired ? <button className="button ghost full" onClick={() => { setLoginLabel(selected.label); setAddOpen(true); }}><LinkIcon />Reconnect account</button> : null}
              <div className="split-actions"><button className="button ghost" onClick={() => void rename(selected)}><EditIcon />Rename</button><button className="button ghost danger-text" onClick={() => void remove(selected)}><TrashIcon />Remove</button></div>
            </div>
          </>
        ) : (
          <div className="empty-inspector"><ShieldIcon /><strong>No account selected</strong><span>Add an OpenAI account to begin.</span></div>
        )}
      </aside>

      <AddAccountModal open={addOpen} initialLabel={loginLabel} onClose={() => setAddOpen(false)} onAdded={async (account) => { setAddOpen(false); setSelectedId(account.id); try { await bridgeApi.refreshAccount(account.id); } catch { /* the account remains connected and can be refreshed later */ } await load(); }} />
    </div>
  );
}

function AccountsView(props: {
  accounts: Account[];
  selected: Account | null;
  selectedState: { label: string; className: string } | null;
  healthy: number;
  weeklyLow: number | null;
  sessionLow: number | null;
  onAdd: () => void;
  onRefreshAll: () => void;
  onRefreshOne: (id: string) => void;
  onRename: (account: Account) => void;
  onRemove: (account: Account) => void;
  busy: string | null;
}) {
  const { accounts, selected } = props;
  const session = getWindow(selected, "session");
  const weekly = getWindow(selected, "weekly");
  const review = getWindow(selected, "code_review");

  return (
    <div className="content-scroll">
      <header className="page-header">
        <div><span className="eyebrow">Codex subscriptions</span><h1>Usage dashboard</h1><p>Monitor multiple OpenAI accounts without installing or reading credentials from another CLI.</p></div>
        <div className="header-actions"><button className="button ghost" onClick={props.onRefreshAll} disabled={props.busy === "refresh-all"}><RefreshIcon />{props.busy === "refresh-all" ? "Refreshing…" : "Refresh all"}</button><button className="button primary" onClick={props.onAdd}><PlusIcon />Add account</button></div>
      </header>

      <section className="summary-grid">
        <SummaryCard label="Accounts" value={String(accounts.length)} helper="Connected subscriptions" icon={<UsersIcon />} />
        <SummaryCard label="Healthy" value={String(props.healthy)} helper="Live usage snapshots" icon={<ShieldIcon />} />
        <SummaryCard label="Lowest weekly" value={props.weeklyLow == null ? "—" : `${Math.round(props.weeklyLow)}%`} helper="Remaining across accounts" icon={<GaugeIcon />} />
        <SummaryCard label="Lowest session" value={props.sessionLow == null ? "—" : `${Math.round(props.sessionLow)}%`} helper="Remaining across accounts" icon={<GaugeIcon />} />
      </section>

      {selected ? (
        <section className="selected-panel">
          <div className="section-heading"><div><span className="eyebrow">Selected account</span><h2>{selected.label}</h2></div><div className="badge-row"><span className={`status-pill ${props.selectedState?.className}`}>{props.selectedState?.label}</span><span className="plan-pill">{selected.plan ?? "Unknown plan"}</span></div></div>
          <div className="usage-grid">
            <div className="usage-card wide">{session ? <UsageBar window={session} /> : <EmptyMetric label="Session usage" />}</div>
            <div className="usage-card wide">{weekly ? <UsageBar window={weekly} /> : <EmptyMetric label="Weekly usage" />}</div>
            <div className="usage-card compact"><span className="usage-label">Code review</span><strong>{review?.remainingPercent == null ? "—" : `${Math.round(review.remainingPercent)}%`}</strong><small>{review ? `Resets ${formatTime(review.resetsAt)}` : "No code-review window returned"}</small></div>
            <div className="usage-card compact"><span className="usage-label">Credits</span><strong>{selected.lastUsage?.creditsUsd == null ? "—" : `$${selected.lastUsage.creditsUsd.toFixed(2)}`}</strong><small>{selected.lastUsage?.unlimitedCredits ? "Unlimited credits" : "Remaining credit balance"}</small></div>
          </div>
        </section>
      ) : (
        <section className="welcome-panel"><GaugeIcon /><h2>Connect an OpenAI account</h2><p>Each account is authenticated separately and stored in the operating system credential manager.</p><button className="button primary" onClick={props.onAdd}><PlusIcon />Add account</button></section>
      )}

      <section className="all-accounts-section">
        <div className="section-heading"><div><span className="eyebrow">All accounts</span><h2>Subscription health</h2></div></div>
        <div className="account-table">
          {accounts.map((account) => {
            const state = accountState(account);
            const accountSession = getWindow(account, "session");
            const accountWeekly = getWindow(account, "weekly");
            return (
              <div className="account-table-row" key={account.id}>
                <span className="account-avatar">{account.label.slice(0, 1).toUpperCase()}</span>
                <div className="table-account"><strong>{account.label}</strong><small>{account.email ?? "OpenAI account"}</small></div>
                <div className="table-metric"><small>Session</small><strong>{accountSession?.remainingPercent == null ? "—" : `${Math.round(accountSession.remainingPercent)}%`}</strong></div>
                <div className="table-metric"><small>Weekly</small><strong>{accountWeekly?.remainingPercent == null ? "—" : `${Math.round(accountWeekly.remainingPercent)}%`}</strong></div>
                <span className={`status-pill ${state.className}`}>{state.label}</span>
                <button className="icon-button" title="Refresh" onClick={() => props.onRefreshOne(account.id)} disabled={props.busy === `refresh:${account.id}`}><RefreshIcon /></button>
              </div>
            );
          })}
          {!accounts.length ? <div className="empty-table">No accounts connected.</div> : null}
        </div>
      </section>
    </div>
  );
}

function SummaryCard({ label, value, helper, icon }: { label: string; value: string; helper: string; icon: React.ReactNode }) {
  return <div className="summary-card"><span className="summary-icon">{icon}</span><div><small>{label}</small><strong>{value}</strong><span>{helper}</span></div></div>;
}

function EmptyMetric({ label }: { label: string }) {
  return <div className="empty-metric"><span>{label}</span><strong>Unavailable</strong><small>Refresh this account to retrieve its current limits.</small></div>;
}

function IntegrationView({ bridge, onRegenerate, busy }: { bridge: BridgeInfo | null; onRegenerate: () => void; busy: boolean }) {
  return (
    <div className="content-scroll narrow-content">
      <header className="page-header"><div><span className="eyebrow">Optional integration</span><h1>Paseo bridge API</h1><p>The desktop app exposes sanitized usage data over localhost. OAuth tokens never leave the native backend.</p></div></header>
      <section className="integration-hero"><span className={`connection-badge ${bridge?.running ? "online" : "offline"}`}><span />{bridge?.running ? "Local API running" : "Local API unavailable"}</span><h2>{bridge?.endpoint ?? "Starting…"}</h2><p>Configure Paseo's external provider-usage adapter to read this versioned endpoint.</p></section>
      <section className="settings-card">
        <div className="settings-row"><div><strong>Endpoint</strong><small>Loopback only; never listens on the network.</small></div><div className="copy-value"><code>{bridge?.endpoint ?? "—"}</code><button className="icon-button" onClick={() => bridge && void copy(bridge.endpoint)}><CopyIcon /></button></div></div>
        <div className="settings-row"><div><strong>Bearer token</strong><small>Required for every usage request.</small></div><div className="copy-value secret"><code>{bridge?.token ?? "—"}</code><button className="icon-button" onClick={() => bridge && void copy(bridge.token)}><CopyIcon /></button></div></div>
        <div className="settings-row"><div><strong>Rotate token</strong><small>Existing integrations stop working until updated.</small></div><button className="button ghost" onClick={onRegenerate} disabled={busy}>{busy ? "Rotating…" : "Regenerate"}</button></div>
      </section>
      <section className="code-card"><div className="code-card-heading"><strong>Environment configuration</strong><button className="icon-button" onClick={() => bridge && void copy(`PASEO_EXTERNAL_PROVIDER_USAGE_URL=${bridge.endpoint}\nPASEO_EXTERNAL_PROVIDER_USAGE_TOKEN=${bridge.token}`)}><CopyIcon /></button></div><pre>{bridge ? `PASEO_EXTERNAL_PROVIDER_USAGE_URL=${bridge.endpoint}\nPASEO_EXTERNAL_PROVIDER_USAGE_TOKEN=${bridge.token}` : "Bridge is starting…"}</pre></section>
      {bridge?.error ? <div className="error-panel">{bridge.error}</div> : null}
    </div>
  );
}

function SettingsView({ autostart, onToggleAutostart }: { autostart: boolean; onToggleAutostart: () => void }) {
  return (
    <div className="content-scroll narrow-content">
      <header className="page-header"><div><span className="eyebrow">Application</span><h1>Settings</h1><p>Control how the bridge behaves on this computer.</p></div></header>
      <section className="settings-card">
        <div className="settings-row"><div><strong>Start at login</strong><small>Keep usage available to Paseo after signing in.</small></div><button className={`toggle ${autostart ? "on" : ""}`} onClick={onToggleAutostart} aria-pressed={autostart}><span /></button></div>
        <div className="settings-row"><div><strong>Credential storage</strong><small>Windows Credential Manager or macOS Keychain.</small></div><span className="setting-value"><ShieldIcon />Native</span></div>
        <div className="settings-row"><div><strong>Usage cache</strong><small>Last-known-good results remain visible during transient failures.</small></div><span className="setting-value">5 minutes</span></div>
        <div className="settings-row"><div><strong>Primary endpoint</strong><small>No secondary or probe fallback is used.</small></div><span className="setting-value mono">/backend-api/wham/usage</span></div>
      </section>
      <section className="notice-card"><ShieldIcon /><div><strong>Independent account storage</strong><p>This application owns its account credentials independently and does not read them from other developer tools.</p></div></section>
    </div>
  );
}
