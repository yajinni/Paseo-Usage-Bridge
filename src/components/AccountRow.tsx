import { useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import type { Account, Provider, UsageWindow } from "../types";
import { ChevronIcon, EditIcon, LinkIcon, RefreshIcon, SettingsIcon, TrashIcon } from "../icons";
import { ProviderIcon } from "./ProviderIcon";

const DRAG_START_DISTANCE_PX = 6;

type PointerDragState = {
  pointerId: number;
  startX: number;
  startY: number;
  started: boolean;
  targetAccountId: string | null;
  targetElement: HTMLElement | null;
};

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

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest("button, a, input, select, textarea, [contenteditable='true']"));
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
  const pointerDrag = useRef<PointerDragState | null>(null);
  const suppressClick = useRef(false);
  const [pointerDragging, setPointerDragging] = useState(false);

  const activate = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect();
    }
  };

  const clearDropTarget = () => {
    const drag = pointerDrag.current;
    if (!drag) return;
    drag.targetElement?.classList.remove("drop-target");
    drag.targetElement = null;
    drag.targetAccountId = null;
  };

  const startPointerDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (busy != null || event.button !== 0 || isInteractiveTarget(event.target)) return;

    pointerDrag.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      started: false,
      targetAccountId: null,
      targetElement: null,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const updatePointerDrag = (event: PointerEvent<HTMLDivElement>) => {
    const drag = pointerDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;

    if (!drag.started) {
      const distance = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY);
      if (distance < DRAG_START_DISTANCE_PX) return;
      drag.started = true;
      setPointerDragging(true);
    }

    event.preventDefault();
    const targetElement = document.elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>(".account-row-shell[data-account-id]") ?? null;
    const targetAccountId = targetElement?.dataset.accountId ?? null;

    if (targetAccountId === drag.targetAccountId) return;

    clearDropTarget();
    if (targetElement && targetAccountId && targetAccountId !== account.id) {
      targetElement.classList.add("drop-target");
      drag.targetElement = targetElement;
      drag.targetAccountId = targetAccountId;
    }
  };

  const finishPointerDrag = (event: PointerEvent<HTMLDivElement>, commit: boolean) => {
    const drag = pointerDrag.current;
    if (!drag || drag.pointerId !== event.pointerId) return;

    const targetAccountId = commit && drag.started ? drag.targetAccountId : null;
    const didDrag = drag.started;
    clearDropTarget();
    pointerDrag.current = null;

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    if (!didDrag) return;

    event.preventDefault();
    event.stopPropagation();
    setPointerDragging(false);
    suppressClick.current = true;
    window.setTimeout(() => {
      suppressClick.current = false;
    }, 0);

    if (targetAccountId && targetAccountId !== account.id) {
      onMove(account.id, targetAccountId);
    }
  };

  return (
    <div className={`account-row-shell ${selected ? "expanded" : ""}`} data-account-id={account.id}>
      <div
        className={`account-row ${selected ? "selected" : ""}${pointerDragging ? " dragging" : ""}`}
        role="button"
        tabIndex={0}
        aria-expanded={selected}
        onClick={(event) => {
          if (suppressClick.current) {
            suppressClick.current = false;
            event.preventDefault();
            event.stopPropagation();
            return;
          }
          onSelect();
        }}
        onKeyDown={activate}
        onPointerDown={startPointerDrag}
        onPointerMove={updatePointerDrag}
        onPointerUp={(event) => finishPointerDrag(event, true)}
        onPointerCancel={(event) => finishPointerDrag(event, false)}
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
