import { useCallback, useEffect, useMemo, useState } from "react";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { bridgeApi } from "./api";
import { AccountAlertModal } from "./components/AccountAlertModal";
import { AccountRow } from "./components/AccountRow";
import { AddAccountModal } from "./components/AddAccountModal";
import { UsageBar } from "./components/UsageBar";
import {
  CopyIcon,
  GaugeIcon,
  LinkIcon,
  PlusIcon,
  RefreshIcon,
  SettingsIcon,
  ShieldIcon,
  UsersIcon,
} from "./icons";
import type { Account, AppUpdateStatus, BridgeInfo, DashboardSnapshot, Provider } from "./types";

type Section = "accounts" | "integration" | "settings";
type UpdateBusy = "checking" | "installing" | null;

type NextResetSummary = {
  value: string;
  helper: string;
};

const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

function providerName(provider: Provider): string {
  switch (provider) {
    case "openai": return "OpenAI Codex";
    case "anthropic": return "Anthropic Claude";
    case "antigravity": return "Google Antigravity";
    case "opencode_go": return "OpenCode Go";
  }
}

function formatTime(value: string | null | undefined): string {
  if (!value) return "Never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

function accountState(account: Account): { label: string; className: string } {
  if (account.authRequired || account.lastUsage?.freshness === "auth_required") return { label: "Auth needed", className: "danger" };
  if (!account.lastUsage) return { label: "Not refreshed", className: "neutral" };
  if (account.lastUsage.freshness === "stale") return { label: "Stale", className: "warning" };
  if (account.lastUsage.freshness === "unavailable") return { label: "Unavailable", className: "neutral" };
  return { label: "Live", className: "success" };
}

function accountNeedsAttention(account: Account): boolean {
  return Boolean(
    account.authRequired
    || account.lastError
    || !account.lastUsage
    || account.lastUsage.freshness !== "live",
  );
}

function nextResetSummary(accounts: Account[]): NextResetSummary {
  const now = Date.now();
  const candidates = accounts.flatMap((account) =>
    (account.lastUsage?.windows ?? []).flatMap((window) => {
      if (!window.resetsAt) return [];
      const resetAt = new Date(window.resetsAt).getTime();
      if (!Number.isFinite(resetAt) || resetAt <= now) return [];
      return [{ resetAt, account: account.label, window: window.label }];
    }),
  );

  if (!candidates.length) {
    return { value: "—", helper: "No upcoming reset reported" };
  }

  candidates.sort((left, right) => left.resetAt - right.resetAt);
  const next = candidates[0];
  const remainingMinutes = Math.max(1, Math.ceil((next.resetAt - now) / 60_000));
  const value = remainingMinutes < 60
    ? `${remainingMinutes}m`
    : remainingMinutes < 24 * 60
      ? `${Math.ceil(remainingMinutes / 60)}h`
      : `${Math.ceil(remainingMinutes / (24 * 60))}d`;

  return {
    value,
    helper: `${next.account} · ${next.window}`,
  };
}

function copy(value: string) {
  return navigator.clipboard.writeText(value);
}

export default function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [section, setSection] = useState<Section>("accounts");
  const [addOpen, setAddOpen] = useState(false);
  const [alertAccount, setAlertAccount] = useState<Account | null>(null);
  const [loginLabel, setLoginLabel] = useState("");
  const [loginProvider, setLoginProvider] = useState<Provider | undefined>(undefined);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [appUpdate, setAppUpdate] = useState<AppUpdateStatus | null>(null);
  const [updateBusy, setUpdateBusy] = useState<UpdateBusy>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);

  const openAdd = useCallback((account?: Account) => {
    setLoginLabel(account?.label ?? "");
    setLoginProvider(account?.provider);
    setAddOpen(true);
  }, []);

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

  const checkForUpdate = useCallback(async (showError = false) => {
    setUpdateBusy("checking");
    try {
      const status = await bridgeApi.checkForUpdate();
      setAppUpdate(status);
      setUpdateError(null);
    } catch (cause) {
      const message = String(cause);
      setUpdateError(message);
      if (showError) setError(message);
    } finally {
      setUpdateBusy(null);
    }
  }, []);

  const installUpdate = useCallback(async () => {
    setUpdateBusy("installing");
    setUpdateError(null);
    try {
      await bridgeApi.installUpdate();
    } catch (cause) {
      const message = String(cause);
      setUpdateError(message);
      setError(message);
      setUpdateBusy(null);
    }
  }, []);

  useEffect(() => {
    void load();
    void isEnabled().then(setAutostart).catch(() => setAutostart(false));
    void checkForUpdate(false);
    const interval = window.setInterval(() => void checkForUpdate(false), UPDATE_CHECK_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [load, checkForUpdate]);

  const accounts = snapshot?.accounts ?? [];
  const selected = accounts.find((account) => account.id === selectedId) ?? null;
  const selectedState = selected ? accountState(selected) : null;
  const needsAttention = accounts.filter(accountNeedsAttention).length;
  const nextReset = nextResetSummary(accounts);

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

  const moveAccount = async (sourceAccountId: string, targetAccountId: string) => {
    if (!snapshot || sourceAccountId === targetAccountId || busy) return;
    const previousAccounts = snapshot.accounts;
    const sourceIndex = previousAccounts.findIndex((account) => account.id === sourceAccountId);
    if (sourceIndex < 0) return;

    const reordered = [...previousAccounts];
    const [moved] = reordered.splice(sourceIndex, 1);
    const targetIndex = reordered.findIndex((account) => account.id === targetAccountId);
    if (targetIndex < 0) return;
    reordered.splice(targetIndex, 0, moved);

    setSnapshot({ ...snapshot, accounts: reordered });
    setBusy("reorder-accounts");
    try {
      const saved = await bridgeApi.reorderAccounts(reordered.map((account) => account.id));
      setSnapshot((current) => current ? { ...current, accounts: saved } : current);
    } catch (cause) {
      setSnapshot((current) => current ? { ...current, accounts: previousAccounts } : current);
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
    if (!window.confirm(`Remove ${account.label}? This deletes its stored provider credentials from the operating-system credential store.`)) return;
    setBusy(`remove:${account.id}`);
    try {
      await bridgeApi.removeAccount(account.id);
      if (alertAccount?.id === account.id) setAlertAccount(null);
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
      return <SettingsView
        autostart={autostart}
        onToggleAutostart={toggleAutostart}
        update={appUpdate}
        updateBusy={updateBusy}
        updateError={updateError}
        onCheckForUpdate={() => void checkForUpdate(true)}
        onInstallUpdate={() => void installUpdate()}
      />;
    }
    return (
      <AccountsView
        accounts={accounts}
        selected={selected}
        selectedState={selectedState}
        needsAttention={needsAttention}
        nextReset={nextReset}
        onAdd={() => openAdd()}
        onRefreshAll={refreshAll}
        busy={busy}
      />
    );
  }, [section, snapshot?.bridge, busy, autostart, accounts, selected, selectedState, needsAttention, nextReset.value, nextReset.helper, appUpdate, updateBusy, updateError, checkForUpdate, installUpdate, openAdd]);

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

        <div className="sidebar-section-title"><span>Usage accounts</span></div>
        <div className="account-list">
          {accounts.length ? accounts.map((account) => (
            <AccountRow
              key={account.id}
              account={account}
              selected={account.id === selectedId}
              busy={busy}
              onSelect={() => {
                setSelectedId(account.id);
                setSection("accounts");
              }}
              onRefresh={() => void refreshOne(account.id)}
              onReconnect={() => openAdd(account)}
              onRename={() => void rename(account)}
              onRemove={() => void remove(account)}
              onSettings={() => setAlertAccount(account)}
              onMove={(sourceAccountId, targetAccountId) => void moveAccount(sourceAccountId, targetAccountId)}
            />
          )) : <button className="empty-account" onClick={() => openAdd()}><PlusIcon /><span>Add your first account</span></button>}
        </div>

        <div className="sidebar-footer">
          <span className={`connection-dot ${snapshot?.bridge.running ? "online" : "offline"}`} />
          <div><strong>{snapshot?.bridge.running ? "Bridge online" : "Bridge offline"}</strong><small>{snapshot?.bridge.endpoint ?? "Starting…"}</small></div>
        </div>
      </aside>

      <main className="main-stage">
        {error ? <div className="global-error"><span>{error}</span><button onClick={() => setError(null)}>Dismiss</button></div> : null}
        {appUpdate?.available ? (
          <div className="update-banner">
            <div><RefreshIcon /><span><strong>Version {appUpdate.availableVersion} is available</strong><small>The signed update is ready to download from GitHub Releases.</small></span></div>
            <button className="button primary" disabled={updateBusy === "installing"} onClick={() => void installUpdate()}>{updateBusy === "installing" ? "Installing…" : "Restart and update"}</button>
          </div>
        ) : null}
        {snapshot ? content : <div className="loading-screen"><span className="spinner" />Loading bridge…</div>}
      </main>

      <AddAccountModal
        open={addOpen}
        initialLabel={loginLabel}
        initialProvider={loginProvider}
        onClose={() => setAddOpen(false)}
        onAdded={async (account) => {
          setAddOpen(false);
          setSelectedId(account.id);
          try { await bridgeApi.refreshAccount(account.id); } catch { /* cached or newly connected account remains available */ }
          await load();
        }}
      />
      <AccountAlertModal
        account={alertAccount}
        onClose={() => setAlertAccount(null)}
        onSaved={async () => {
          setAlertAccount(null);
          await load();
        }}
      />
    </div>
  );
}

function AccountsView(props: {
  accounts: Account[];
  selected: Account | null;
  selectedState: { label: string; className: string } | null;
  needsAttention: number;
  nextReset: NextResetSummary;
  onAdd: () => void;
  onRefreshAll: () => void;
  busy: string | null;
}) {
  const { accounts, selected } = props;
  const windows = selected?.lastUsage?.windows ?? [];

  return (
    <div className="content-scroll">
      <header className="page-header">
        <div><span className="eyebrow">AI subscriptions</span><h1>Usage dashboard</h1><p>Monitor OpenAI Codex, Anthropic Claude, Google Antigravity, and OpenCode Go from one native desktop app.</p></div>
        <div className="header-actions"><button className="button ghost" onClick={props.onRefreshAll} disabled={props.busy === "refresh-all"}><RefreshIcon />{props.busy === "refresh-all" ? "Refreshing…" : "Refresh all"}</button><button className="button primary" onClick={props.onAdd}><PlusIcon />Add account</button></div>
      </header>

      <section className="summary-grid summary-grid-three">
        <SummaryCard label="Connected accounts" value={String(accounts.length)} helper="Subscriptions being monitored" icon={<UsersIcon />} />
        <SummaryCard label="Needs attention" value={String(props.needsAttention)} helper={props.needsAttention ? "Reconnect or refresh an account" : "All accounts are current"} icon={<ShieldIcon />} tone={props.needsAttention ? "warning" : "success"} />
        <SummaryCard label="Next reset" value={props.nextReset.value} helper={props.nextReset.helper} icon={<GaugeIcon />} />
      </section>

      {selected ? (
        <section className="selected-panel">
          <div className="section-heading">
            <div>
              <span className="eyebrow">Selected account</span>
              <h2>{selected.label}</h2>
              <p className="selected-account-meta">{selected.email ?? providerName(selected.provider)} · Last refreshed {formatTime(selected.lastUsage?.fetchedAt)}</p>
            </div>
            <div className="badge-row">
              <span className={`status-pill ${props.selectedState?.className}`}>{props.selectedState?.label}</span>
              <span className="plan-pill">{providerName(selected.provider)}</span>
              {selected.plan ? <span className="plan-pill">{selected.plan}</span> : null}
            </div>
          </div>
          {selected.lastError ? <div className="error-panel selected-account-error">{selected.lastError}</div> : null}
          <div className="usage-grid">
            {windows.map((window) => <div className="usage-card wide" key={window.id}><UsageBar window={window} /></div>)}
            {selected.lastUsage?.creditsUsd != null || selected.lastUsage?.unlimitedCredits ? (
              <div className="usage-card compact"><span className="usage-label">Credits</span><strong>{selected.lastUsage.unlimitedCredits ? "Unlimited" : `$${selected.lastUsage.creditsUsd?.toFixed(2)}`}</strong><small>Provider-reported remaining credit balance</small></div>
            ) : null}
            {!windows.length ? <div className="usage-card wide"><EmptyMetric label="Provider usage" /></div> : null}
          </div>
        </section>
      ) : (
        <section className="welcome-panel"><GaugeIcon /><h2>Connect a provider account</h2><p>Each account is authenticated separately and its credentials remain in the operating-system credential manager.</p><button className="button primary" onClick={props.onAdd}><PlusIcon />Add account</button></section>
      )}
    </div>
  );
}

function SummaryCard({ label, value, helper, icon, tone }: { label: string; value: string; helper: string; icon: React.ReactNode; tone?: "success" | "warning" }) {
  return <div className={`summary-card ${tone ? `summary-${tone}` : ""}`}><span className="summary-icon">{icon}</span><div><small>{label}</small><strong>{value}</strong><span>{helper}</span></div></div>;
}

function EmptyMetric({ label }: { label: string }) {
  return <div className="empty-metric"><span>{label}</span><strong>Unavailable</strong><small>Refresh this account to retrieve its current limits.</small></div>;
}

function IntegrationView({ bridge, onRegenerate, busy }: { bridge: BridgeInfo | null; onRegenerate: () => void; busy: boolean }) {
  return (
    <div className="content-scroll narrow-content">
      <header className="page-header"><div><span className="eyebrow">Optional integration</span><h1>Paseo bridge API</h1><p>The desktop app exposes normalized, sanitized provider usage over localhost. Provider credentials never leave the native backend.</p></div></header>
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

function SettingsView({
  autostart,
  onToggleAutostart,
  update,
  updateBusy,
  updateError,
  onCheckForUpdate,
  onInstallUpdate,
}: {
  autostart: boolean;
  onToggleAutostart: () => void;
  update: AppUpdateStatus | null;
  updateBusy: UpdateBusy;
  updateError: string | null;
  onCheckForUpdate: () => void;
  onInstallUpdate: () => void;
}) {
  return (
    <div className="content-scroll narrow-content">
      <header className="page-header"><div><span className="eyebrow">Application</span><h1>Settings</h1><p>Control how the bridge behaves on this computer.</p></div></header>
      <section className="settings-card">
        <div className="settings-row"><div><strong>Start at login</strong><small>Keep usage available to Paseo after signing in.</small></div><button className={`toggle ${autostart ? "on" : ""}`} onClick={onToggleAutostart} aria-pressed={autostart}><span /></button></div>
        <div className="settings-row"><div><strong>Automatic updates</strong><small>Checks GitHub Releases at startup and every six hours.</small></div>{update?.available ? <button className="button primary" disabled={updateBusy !== null} onClick={onInstallUpdate}>{updateBusy === "installing" ? "Installing…" : `Install v${update.availableVersion}`}</button> : <button className="button ghost" disabled={updateBusy !== null} onClick={onCheckForUpdate}>{updateBusy === "checking" ? "Checking…" : "Check now"}</button>}</div>
        <div className="settings-row"><div><strong>Installed version</strong><small>{update?.available ? `Version ${update.availableVersion} is available.` : "The app installs only signed update packages."}</small></div><span className="setting-value mono">v{update?.currentVersion ?? "0.1.1"}</span></div>
        <div className="settings-row"><div><strong>Credential storage</strong><small>Windows Credential Manager or macOS Keychain.</small></div><span className="setting-value"><ShieldIcon />Native</span></div>
        <div className="settings-row"><div><strong>Usage cache</strong><small>Last-known-good results remain visible during transient provider failures.</small></div><span className="setting-value">5 minutes</span></div>
        <div className="settings-row"><div><strong>Provider connectors</strong><small>Each provider uses its own read-only quota or usage source.</small></div><span className="setting-value mono">4 enabled</span></div>
      </section>
      {update?.available && update.body ? <section className="update-notes"><strong>What changed in v{update.availableVersion}</strong><p>{update.body}</p>{update.date ? <small>Published {formatTime(update.date)}</small> : null}</section> : null}
      {updateError ? <div className="error-panel settings-update-error">{updateError}</div> : null}
      <section className="notice-card"><ShieldIcon /><div><strong>Independent account storage</strong><p>This application owns its provider credentials independently and does not read them from other developer tools.</p></div></section>
    </div>
  );
}
