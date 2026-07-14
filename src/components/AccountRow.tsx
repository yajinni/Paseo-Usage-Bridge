import type { Account, Provider } from "../types";
import { ChevronIcon, EditIcon, LinkIcon, RefreshIcon, TrashIcon } from "../icons";

function providerName(provider: Provider): string {
  switch (provider) {
    case "openai": return "OpenAI Codex";
    case "anthropic": return "Anthropic Claude";
    case "antigravity": return "Google Antigravity";
    case "opencode_go": return "OpenCode Go";
  }
}

function weeklyRemaining(account: Account): number | null {
  return account.lastUsage?.windows.find((window) =>
    window.id === "weekly" || window.windowSeconds === 604_800,
  )?.remainingPercent ?? null;
}

function lastRefreshed(value: string | null | undefined): string {
  if (!value) return "Never refreshed";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Refresh time unavailable";
  return `Refreshed ${date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`;
}

export function AccountRow({
  account,
  selected,
  busy,
  onSelect,
  onRefresh,
  onReconnect,
  onRename,
  onRemove,
}: {
  account: Account;
  selected: boolean;
  busy: string | null;
  onSelect: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onRename: () => void;
  onRemove: () => void;
}) {
  const weekly = weeklyRemaining(account);
  const state = account.authRequired ? "auth" : account.lastUsage?.freshness === "stale" ? "stale" : account.lastUsage ? "live" : "idle";
  const refreshBusy = busy === `refresh:${account.id}`;
  const renameBusy = busy === `rename:${account.id}`;
  const removeBusy = busy === `remove:${account.id}`;

  return (
    <div className={`account-row-shell ${selected ? "expanded" : ""}`}>
      <button className={`account-row ${selected ? "selected" : ""}`} onClick={onSelect} aria-expanded={selected}>
        <span className={`account-avatar state-${state}`}>{account.label.slice(0, 1).toUpperCase()}</span>
        <span className="account-row-copy">
          <strong>{account.label}</strong>
          <small>{account.email ?? providerName(account.provider)}</small>
          <small className="account-refresh-time">{lastRefreshed(account.lastUsage?.fetchedAt)}</small>
        </span>
        <span className="account-row-meta">
          <strong>{weekly == null ? "—" : `${Math.round(weekly)}%`}</strong>
          <small>weekly</small>
        </span>
        <ChevronIcon className="chevron" />
      </button>

      {selected ? (
        <div className="account-row-actions">
          {account.authRequired ? (
            <button className="sidebar-action primary-action" onClick={onReconnect}><LinkIcon />Reconnect</button>
          ) : (
            <button className="sidebar-action primary-action" onClick={onRefresh} disabled={refreshBusy}><RefreshIcon />{refreshBusy ? "Refreshing…" : "Refresh usage"}</button>
          )}
          <button className="sidebar-action" onClick={onRename} disabled={renameBusy}><EditIcon />Rename</button>
          <button className="sidebar-action danger-text" onClick={onRemove} disabled={removeBusy}><TrashIcon />Remove</button>
        </div>
      ) : null}
    </div>
  );
}
