import type { UsageWindow } from "../types";

function formatReset(value: string | null): string {
  if (!value) return "Reset time unavailable";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Reset time unavailable";
  return `Resets ${date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`;
}

export function UsageBar({ window }: { window: UsageWindow }) {
  const remaining = window.remainingPercent;
  const width = remaining == null ? 0 : Math.min(100, Math.max(0, remaining));
  const tone = remaining == null ? "neutral" : remaining <= 10 ? "danger" : remaining <= 30 ? "warning" : "good";

  return (
    <div className="usage-block">
      <div className="usage-heading">
        <div>
          <span className="usage-label">{window.label}</span>
          <strong>{remaining == null ? "Unavailable" : `${Math.round(remaining)}% remaining`}</strong>
        </div>
        {window.windowSeconds ? <span className="window-pill">{Math.round(window.windowSeconds / 3600)}h window</span> : null}
      </div>
      <div className="progress-track" aria-label={`${window.label} remaining`}>
        <span className={`progress-fill ${tone}`} style={{ width: `${width}%` }} />
      </div>
      <span className="reset-label">{formatReset(window.resetsAt)}</span>
    </div>
  );
}
