import type { Account, Provider } from "../types";
import { ChevronIcon } from "../icons";

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

export function AccountRow({ account, selected, onSelect }: { account: Account; selected: boolean; onSelect: () => void }) {
  const weekly = weeklyRemaining(account);
  const state = account.authRequired ? "auth" : account.lastUsage?.freshness === "stale" ? "stale" : account.lastUsage ? "live" : "idle";

  return (
    <button className={`account-row ${selected ? "selected" : ""}`} onClick={onSelect}>
      <span className={`account-avatar state-${state}`}>{account.label.slice(0, 1).toUpperCase()}</span>
      <span className="account-row-copy">
        <strong>{account.label}</strong>
        <small>{account.email ?? providerName(account.provider)}</small>
      </span>
      <span className="account-row-meta">
        <strong>{weekly == null ? "—" : `${Math.round(weekly)}%`}</strong>
        <small>weekly</small>
      </span>
      <ChevronIcon className="chevron" />
    </button>
  );
}
