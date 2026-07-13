import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { bridgeApi } from "../api";
import type { Account, LoginStatus } from "../types";

export function AddAccountModal({ open, initialLabel, onClose, onAdded }: { open: boolean; initialLabel?: string; onClose: () => void; onAdded: (account: Account) => void }) {
  const [label, setLabel] = useState("");
  const [status, setStatus] = useState<LoginStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setLabel("");
      setStatus(null);
      setBusy(false);
      setError(null);
    } else {
      setLabel(initialLabel ?? "");
    }
  }, [open, initialLabel]);

  useEffect(() => {
    if (!status || status.status !== "waiting") return;
    const timer = window.setInterval(async () => {
      try {
        const next = await bridgeApi.loginStatus(status.attemptId);
        setStatus(next);
        if (next.status === "complete" && next.account) {
          window.clearInterval(timer);
          onAdded(next.account);
        }
        if (next.status === "failed") {
          window.clearInterval(timer);
          setBusy(false);
          setError(next.message ?? "OpenAI authentication failed.");
        }
      } catch (cause) {
        window.clearInterval(timer);
        setBusy(false);
        setError(String(cause));
      }
    }, 900);
    return () => window.clearInterval(timer);
  }, [status, onAdded]);

  if (!open) return null;

  const begin = async () => {
    setBusy(true);
    setError(null);
    try {
      const start = await bridgeApi.startLogin(label.trim() || "Codex account");
      setStatus({ attemptId: start.attemptId, status: "waiting", message: null, account: null });
      await openUrl(start.authorizationUrl);
    } catch (cause) {
      setBusy(false);
      setError(String(cause));
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-labelledby="add-account-title">
        <div className="modal-kicker">OpenAI OAuth</div>
        <h2 id="add-account-title">Add a Codex account</h2>
        <p>Give the account a label, then finish the OpenAI login in your browser. Passwords never pass through this app.</p>
        <label className="field-label" htmlFor="account-label">Account label</label>
        <input id="account-label" className="text-input" value={label} onChange={(event) => setLabel(event.target.value)} placeholder="Personal Plus" disabled={busy} autoFocus />
        {status?.status === "waiting" ? (
          <div className="waiting-panel"><span className="spinner" />Waiting for the browser callback…</div>
        ) : null}
        {error ? <div className="error-panel">{error}</div> : null}
        <div className="modal-actions">
          <button className="button ghost" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="button primary" onClick={begin} disabled={busy}>{busy ? "Waiting…" : "Continue with OpenAI"}</button>
        </div>
      </section>
    </div>
  );
}
