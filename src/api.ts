import { invoke } from "@tauri-apps/api/core";
import type { Account, AppUpdateStatus, BridgeInfo, DashboardSnapshot, LoginStart, LoginStatus, Provider } from "./types";

export const bridgeApi = {
  snapshot: () => invoke<DashboardSnapshot>("get_dashboard_snapshot"),
  startLogin: (label: string, provider: Provider) => invoke<LoginStart>("start_login", { label, provider }),
  addOpenCodeGoAccount: (label: string, workspaceId: string, authCookie: string) =>
    invoke<Account>("add_opencode_go_account", { label, workspaceId, authCookie }),
  loginStatus: (attemptId: string) => invoke<LoginStatus>("get_login_status", { attemptId }),
  refreshAccount: (accountId: string) => invoke<Account>("refresh_account", { accountId }),
  refreshAll: () => invoke<Account[]>("refresh_all"),
  renameAccount: (accountId: string, label: string) =>
    invoke<Account>("rename_account", { accountId, label }),
  removeAccount: (accountId: string) => invoke<void>("remove_account", { accountId }),
  regenerateToken: () => invoke<BridgeInfo>("regenerate_bridge_token"),
  checkForUpdate: () => invoke<AppUpdateStatus>("check_for_app_update"),
  installUpdate: () => invoke<void>("install_app_update"),
};
