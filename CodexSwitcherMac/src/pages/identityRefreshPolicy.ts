export type IdentityRefreshAction =
  | "switch-official-account"
  | "activate-third-party-key"
  | "refresh-status"
  | "sample-now";

export type IdentityRefreshPlan = {
  overview: boolean;
  credentialProfiles: boolean;
  keyUsage: boolean;
  supportingData: boolean;
  sampling: boolean;
};

const lightIdentitySync: IdentityRefreshPlan = {
  overview: true,
  credentialProfiles: true,
  keyUsage: false,
  supportingData: false,
  sampling: false,
};

export function refreshPlanForIdentityAction(action: IdentityRefreshAction): IdentityRefreshPlan {
  switch (action) {
    case "switch-official-account":
      return {
        ...lightIdentitySync,
        supportingData: true,
      };
    case "activate-third-party-key":
      return lightIdentitySync;
    case "refresh-status":
      return {
        overview: true,
        credentialProfiles: true,
        keyUsage: false,
        supportingData: true,
        sampling: true,
      };
    case "sample-now":
      return {
        overview: true,
        credentialProfiles: true,
        keyUsage: true,
        supportingData: true,
        sampling: true,
      };
  }
}
