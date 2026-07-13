import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { bridgeApi } from "../api";
import type { Account, LoginStatus, Provider } from "../types";

const providerOptions: Array<{ id: Provider; label: string; detail: string }> = [
  { id: "openai", label: "OpenAI Codex", detail: "ChatGPT Plus, Pro, Business, or other Codex-enabled plans" },
  { id: "anthropic", label: "Anthropic Claude", detail: "Claude Pro or Max through Anthropic OAuth" },
  { id: "antigravity", label: "Google Antigravity", detail: "Google OAuth and Cloud Code quota data" },
  { id: "opencode_go", label: "OpenCode Go", detail: "Workspace dashboard session and exact Go windows" },
];

function providerName(provider: Provider): string {
  return providerOptions.find((option) => option.id === provider)?.label ?? provider;
}

export function AddAccountModal({
  open,
  initialLabel,
  initialProvider,
  onClose,
  onAdded,
}: {
  open: boolean;
  initialLabel?: string;
  initialProvider?: Provider;
  onClose: () => void;
  onAdded: (account: Account) => void;
}) {
  const [label, setLabel] = useState("");
  const [provider, setProvider] = useState<Provider>("openai");
  const [workspaceId, setWorkspaceId] = useState("");
  const [authCookie, setAuthCookie] = useState("");
  const [status, setStatus] = useState<LoginStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setLabel("");
      setProvider("openai");
      setWorkspaceId("");
      setAuthCookie("");
      setStatus(null);
      setBusy(false);
      setError(null);
    } else {
      setLabel(initialLabel ?? "");
      setProvider(initialProvider ?? "openai");
    }
  }, [open, initialLabel, initialProvider]);

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
          setError(next.message ?? `${providerName(provider)} authentication failed.`);
        }
      } catch (cause) {
        window.clearInterval(timer);
        setBusy(false);
        setError(String(cause));
      }
    }, 900);
    return () => window.clearInterval(timer);
  }, [status, onAdded, provider]);

  if (!open) return null;

  const begin = async () => {
    setBusy(true);
    setError(null);
    try {
      if (provider === "opencode_go") {
        const account = await bridgeApi.addOpenCodeGoAccount(
          label.trim() || "OpenCode Go",
          workspaceId.trim(),
          authCookie.trim(),
        );
        onAdded(account);
        return;
      }
      const start = await bridgeApi.startLogin(label.trim() || providerName(provider), provider);
      setStatus({ attemptId: start.attemptId, status: "waiting", message: null, account: null });
      await openUrl(start.authorizationUrl);
    } catch (cause) {
      setBusy(false);
      setError(String(cause));
    }
  };

  const providerCopy = provider === "opencode_go"
    ? "OpenCode does not expose Go quota through its API key. Enter the workspace ID and the value of the console's auth cookie. Both remain in the operating-system credential store."
    : `Finish the ${providerName(provider)} login in your browser. Passwords never pass through this app.`;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-labelledby="add-account-title">
        <div className="modal-kicker">Provider connection</div>
        <h2 id="add-account-title">Add a usage account</h2>
        <p>{providerCopy}</p>

        <label className="field-label" htmlFor="account-provider">Provider</label>
        <select
          id="account-provider"
          className="text-input"
          value={provider}
          onChange={(event) => {
            setProvider(event.target.value as Provider);
            setStatus(null);
            setError(null);
          }}
          disabled={busy || Boolean(initialProvider)}
          autoFocus
        >
          {providerOptions.map((option) => (
            <option key={option.id} value={option.id}>{option.label} — {option.detail}</option>
          ))}
        </select>

        <label className="field-label field-spaced" htmlFor="account-label">Account label</label>
        <input
          id="account-label"
          className="text-input"
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder={providerName(provider)}
          disabled={busy}
        />

        {provider === "opencode_go" ? (
          <>
            <label className="field-label field-spaced" htmlFor="workspace-id">Workspace ID</label>
            <input
              id="workspace-id"
              className="text-input"
              value={workspaceId}
              onChange={(event) => setWorkspaceId(event.target.value)}
              placeholder="wrk_..."
              disabled={busy}
            />
            <label className="field-label field-spaced" htmlFor="auth-cookie">OpenCode console auth cookie</label>
            <textarea
              id="auth-cookie"
              className="text-input secret-input"
              value={authCookie}
              onChange={(event) => setAuthCookie(event.target.value)}
              placeholder="Paste the auth cookie value, with or without auth="
              disabled={busy}
              rows={3}
            />
            <div className="credential-note">The cookie permits access to your OpenCode console. It is stored only in Keychain or Credential Manager and used only for the read-only Go dashboard request.</div>
          </>
        ) : null}

        {status?.status === "waiting" ? (
          <div className="waiting-panel"><span className="spinner" />Waiting for the browser callback…</div>
        ) : null}
        {error ? <div className="error-panel modal-error">{error}</div> : null}
        <div className="modal-actions">
          <button className="button ghost" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="button primary" onClick={begin} disabled={busy}>
            {busy ? "Connecting…" : provider === "opencode_go" ? "Connect OpenCode Go" : `Continue with ${providerName(provider)}`}
          </button>
        </div>
      </section>
    </div>
  );
}
