import type { Account } from "../types";
import { ChevronIcon } from "../icons";

function remaining(account: Account, id: string): number | null {
  return account.lastUsage?.windows.find((window) => window.id === id)?.remainingPercent ?? null;
}

export function AccountRow({ account, selected, onSelect }: { account: Account; selected: boolean; onSelect: () => void }) {
  const weekly = remaining(account, "weekly");
  const state = account.authRequired ? "auth" : account.lastUsage?.freshness === "stale" ? "stale" : account.lastUsage ? "live" : "idle";

  return (
    <button className={`account-row ${selected ? "selected" : ""}`} onClick={onSelect}>
      <span className={`account-avatar state-${state}`}>{account.label.slice(0, 1).toUpperCase()}</span>
      <span className="account-row-copy">
        <strong>{account.label}</strong>
        <small>{account.email ?? "OpenAI account"}</small>
      </span>
      <span className="account-row-meta">
        <strong>{weekly == null ? "—" : `${Math.round(weekly)}%`}</strong>
        <small>weekly</small>
      </span>
      <ChevronIcon className="chevron" />
    </button>
  );
}
