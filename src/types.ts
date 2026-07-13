export type Provider = "openai" | "anthropic" | "antigravity" | "opencode_go";

export type UsageFreshness = "live" | "stale" | "unavailable" | "auth_required";

export interface UsageWindow {
  id: string;
  label: string;
  usedPercent: number | null;
  remainingPercent: number | null;
  resetsAt: string | null;
  windowSeconds: number | null;
}

export interface UsageSnapshot {
  plan: string | null;
  email: string | null;
  windows: UsageWindow[];
  creditsUsd: number | null;
  unlimitedCredits: boolean;
  fetchedAt: string;
  freshness: UsageFreshness;
  source: string;
}

export interface Account {
  id: string;
  label: string;
  provider: Provider;
  email: string | null;
  providerAccountId: string | null;
  chatgptAccountId: string | null;
  plan: string | null;
  createdAt: string;
  updatedAt: string;
  lastUsage: UsageSnapshot | null;
  lastError: string | null;
  authRequired: boolean;
}

export interface BridgeInfo {
  endpoint: string;
  token: string;
  running: boolean;
  error: string | null;
}

export interface DashboardSnapshot {
  accounts: Account[];
  bridge: BridgeInfo;
}

export interface LoginStart {
  attemptId: string;
  authorizationUrl: string;
  expiresAt: string;
}

export interface LoginStatus {
  attemptId: string;
  status: "waiting" | "complete" | "failed";
  message: string | null;
  account: Account | null;
}

export interface AppUpdateStatus {
  currentVersion: string;
  available: boolean;
  availableVersion: string | null;
  date: string | null;
  body: string | null;
}
