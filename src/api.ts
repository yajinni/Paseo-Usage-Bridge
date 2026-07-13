import { invoke } from "@tauri-apps/api/core";
import type { Account, BridgeInfo, DashboardSnapshot, LoginStart, LoginStatus } from "./types";

export const bridgeApi = {
  snapshot: () => invoke<DashboardSnapshot>("get_dashboard_snapshot"),
  startLogin: (label: string) => invoke<LoginStart>("start_login", { label }),
  loginStatus: (attemptId: string) => invoke<LoginStatus>("get_login_status", { attemptId }),
  refreshAccount: (accountId: string) => invoke<Account>("refresh_account", { accountId }),
  refreshAll: () => invoke<Account[]>("refresh_all"),
  renameAccount: (accountId: string, label: string) =>
    invoke<Account>("rename_account", { accountId, label }),
  removeAccount: (accountId: string) => invoke<void>("remove_account", { accountId }),
  regenerateToken: () => invoke<BridgeInfo>("regenerate_bridge_token"),
};
