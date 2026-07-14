import { useEffect, useMemo, useState } from "react";
import { bridgeApi } from "../api";
import { BellIcon } from "../icons";
import type { Account, UsageAlertSetting, UsageWindow } from "../types";

const THRESHOLDS = [10, 20, 30, 40, 50];
const WINDOW_ORDER = ["five_hour", "weekly", "monthly"] as const;
type AlertWindowId = typeof WINDOW_ORDER[number];

function canonicalWindowId(window: UsageWindow): AlertWindowId | null {
  const id = window.id.toLowerCase().replaceAll("-", "_");
  const label = window.label.toLowerCase();
  if (id === "five_hour" || id === "rolling" || window.windowSeconds === 18_000 || label.includes("5 hour") || label.includes("five hour")) return "five_hour";
  if (id === "weekly" || window.windowSeconds === 604_800 || label.includes("weekly")) return "weekly";
  if (id === "monthly" || label.includes("monthly")) return "monthly";
  return null;
}

function windowLabel(windowId: AlertWindowId): string {
  switch (windowId) {
    case "five_hour": return "5 hour";
    case "weekly": return "Weekly";
    case "monthly": return "Monthly";
  }
}

function defaultSetting(windowId: AlertWindowId): UsageAlertSetting {
  return { windowId, enabled: false, thresholdPercent: 20 };
}

export function AccountAlertModal({
  account,
  onClose,
  onSaved,
}: {
  account: Account | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [settings, setSettings] = useState<UsageAlertSetting[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const availableWindows = useMemo(() => {
    const available = new Set<AlertWindowId>();
    for (const window of account?.lastUsage?.windows ?? []) {
      const windowId = canonicalWindowId(window);
      if (windowId) available.add(windowId);
    }
    return WINDOW_ORDER.filter((windowId) => available.has(windowId));
  }, [account]);

  useEffect(() => {
    if (!account) {
      setSettings([]);
      setError(null);
      setLoading(false);
      setSaving(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    void bridgeApi.getAccountAlerts(account.id)
      .then((saved) => {
        if (cancelled) return;
        setSettings(availableWindows.map((windowId) => saved.find((setting) => setting.windowId === windowId) ?? defaultSetting(windowId)));
      })
      .catch((cause) => {
        if (!cancelled) setError(String(cause));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, [account, availableWindows]);

  if (!account) return null;

  const updateSetting = (windowId: AlertWindowId, update: Partial<UsageAlertSetting>) => {
    setSettings((current) => current.map((setting) => setting.windowId === windowId ? { ...setting, ...update } : setting));
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await bridgeApi.saveAccountAlerts(account.id, settings);
      onSaved();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !saving && onClose()}>
      <section className="modal-card alert-settings-modal" role="dialog" aria-modal="true" aria-labelledby="alert-settings-title">
        <div className="modal-kicker">Account settings</div>
        <div className="alert-settings-heading">
          <span className="alert-settings-icon"><BellIcon /></span>
          <div>
            <h2 id="alert-settings-title">Usage alerts for {account.label}</h2>
            <p>Choose which quota windows should notify you when the remaining allowance reaches a threshold.</p>
          </div>
        </div>

        {loading ? <div className="waiting-panel"><span className="spinner" />Loading alert settings…</div> : null}

        {!loading && availableWindows.length ? (
          <div className="alert-window-list">
            {availableWindows.map((windowId) => {
              const setting = settings.find((candidate) => candidate.windowId === windowId) ?? defaultSetting(windowId);
              return (
                <div className={`alert-window-row ${setting.enabled ? "enabled" : ""}`} key={windowId}>
                  <label className="alert-window-toggle">
                    <input
                      type="checkbox"
                      checked={setting.enabled}
                      onChange={(event) => updateSetting(windowId, { enabled: event.target.checked })}
                    />
                    <span className="alert-checkbox" />
                    <span><strong>{windowLabel(windowId)}</strong><small>Notify once per quota period</small></span>
                  </label>
                  <label className="alert-threshold">
                    <span>At or below</span>
                    <select
                      value={setting.thresholdPercent}
                      disabled={!setting.enabled}
                      onChange={(event) => updateSetting(windowId, { thresholdPercent: Number(event.target.value) })}
                    >
                      {THRESHOLDS.map((threshold) => <option value={threshold} key={threshold}>{threshold}% remaining</option>)}
                    </select>
                  </label>
                </div>
              );
            })}
          </div>
        ) : null}

        {!loading && !availableWindows.length ? (
          <div className="alert-empty-state">
            <BellIcon />
            <strong>No alert windows detected yet</strong>
            <span>Refresh this account once so the app can detect its 5 hour, weekly, or monthly limits.</span>
          </div>
        ) : null}

        <div className="credential-note alert-notification-note">
          Alerts use the operating system notification system and fire only once for each quota period. Banner duration is controlled by Windows or macOS; the alert remains available in Notification Center according to your system settings.
        </div>
        {error ? <div className="error-panel modal-error">{error}</div> : null}
        <div className="modal-actions">
          <button className="button ghost" onClick={onClose} disabled={saving}>Cancel</button>
          <button className="button primary" onClick={save} disabled={loading || saving || !availableWindows.length}>{saving ? "Saving…" : "Save alerts"}</button>
        </div>
      </section>
    </div>
  );
}
