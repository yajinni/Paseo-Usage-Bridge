import type { DragEvent, KeyboardEvent } from "react";
import type { Account, Provider, UsageWindow } from "../types";
import { ChevronIcon, EditIcon, LinkIcon, RefreshIcon, SettingsIcon, TrashIcon } from "../icons";
import { ProviderIcon } from "./ProviderIcon";

function providerName(provider: Provider): string {
  switch (provider) {
    case "openai": return "OpenAI Codex";
    case "anthropic": return "Anthropic Claude";
    case "antigravity": return "Google Antigravity";
    case "opencode_go": return "OpenCode Go";
  }
}

function windowRemaining(account: Account, target: "five_hour" | "weekly"): number | null {
  const window = account.lastUsage?.windows.find((candidate: UsageWindow) => {
    const id = candidate.id.toLowerCase().replaceAll("-", "_");
    const label = candidate.label.toLowerCase();
    if (target === "five_hour") {
      return id === "five_hour" || id === "rolling" || candidate.windowSeconds === 18_000 || label.includes("5 hour") || label.includes("five hour");
    }
    return id === "weekly" || candidate.windowSeconds === 604_800 || label.includes("weekly");
  });
  return window?.remainingPercent ?? null;
}

function lastRefreshed(value: string | null | undefined): string {
  if (!value) return "Never refreshed";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Refresh time unavailable";
  return `Refreshed ${date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`;
}

function RemainingStat({ label, value }: { label: string; value: number | null }) {
  return (
    <span className="account-window-stat">
      <strong>{value == null ? "—" : `${Math.round(value)}%`}</strong>
      <small>{label}</small>
    </span>
  );
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
  onSettings,
  onMove,
}: {
  account: Account;
  selected: boolean;
  busy: string | null;
  onSelect: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onRename: () => void;
  onRemove: () => void;
  onSettings: () => void;
  onMove: (sourceAccountId: string, targetAccountId: string) => void;
}) {
  const fiveHour = windowRemaining(account, "five_hour");
  const weekly = windowRemaining(account, "weekly");
  const state = account.authRequired ? "auth" : account.lastUsage?.freshness === "stale" ? "stale" : account.lastUsage ? "live" : "idle";
  const refreshBusy = busy === `refresh:${account.id}`;
  const renameBusy = busy === `rename:${account.id}`;
  const removeBusy = busy === `remove:${account.id}`;

  const activate = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect();
    }
  };

  const startDrag = (event: DragEvent<HTMLDivElement>) => {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("application/x-paseo-account", account.id);
    event.dataTransfer.setData("text/plain", account.id);
    event.currentTarget.classList.add("dragging");
  };

  const finishDrag = (event: DragEvent<HTMLDivElement>) => {
    event.currentTarget.classList.remove("dragging");
  };

  const dropAccount = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const sourceAccountId = event.dataTransfer.getData("application/x-paseo-account") || event.dataTransfer.getData("text/plain");
    if (sourceAccountId && sourceAccountId !== account.id) onMove(sourceAccountId, account.id);
  };

  return (
    <div className={`account-row-shell ${selected ? "expanded" : ""}`}>
      <div
        className={`account-row ${selected ? "selected" : ""}`}
        role="button"
        tabIndex={0}
        aria-expanded={selected}
        draggable={busy == null}
        onClick={onSelect}
        onKeyDown={activate}
        onDragStart={startDrag}
        onDragEnd={finishDrag}
        onDragOver={(event) => {
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        }}
        onDrop={dropAccount}
      >
        <span className="account-provider-stack">
          <span className={`account-provider-icon state-${state}`}><ProviderIcon provider={account.provider} /></span>
          <button
            type="button"
            className="account-refresh-icon"
            aria-label={`Refresh ${account.label}`}
            title="Refresh usage"
            disabled={refreshBusy || account.authRequired}
            onClick={(event) => {
              event.stopPropagation();
              onRefresh();
            }}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <RefreshIcon className={refreshBusy ? "spinning" : ""} />
          </button>
        </span>
        <span className="account-row-copy">
          <strong>{account.label}</strong>
          <small>{account.email ?? providerName(account.provider)}</small>
          <small className="account-refresh-time">{lastRefreshed(account.lastUsage?.fetchedAt)}</small>
        </span>
        <span className="account-row-meta">
          {fiveHour != null ? <RemainingStat label="5 hour" value={fiveHour} /> : null}
          <RemainingStat label="weekly" value={weekly} />
        </span>
        <ChevronIcon className="chevron" />
      </div>

      {selected ? (
        <div className="account-row-actions">
          {account.authRequired ? (
            <button className="sidebar-action primary-action" onClick={onReconnect}><LinkIcon />Reconnect</button>
          ) : null}
          <button className="sidebar-action" onClick={onRename} disabled={renameBusy}><EditIcon />Rename</button>
          <button className="sidebar-action danger-text" onClick={onRemove} disabled={removeBusy}><TrashIcon />Remove</button>
          <button className="sidebar-action" onClick={onSettings}><SettingsIcon />Settings</button>
        </div>
      ) : null}
    </div>
  );
}
