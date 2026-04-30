import type { Account, CredentialProfile } from "../types";
import {
  accountResetTime,
  accountStatusCompactText,
  accountUsagePercent,
  compareAccounts,
  hasAuthIssue,
  statusText,
} from "./viewModel";

export type IdentityAssetKind = "official_account" | "third_party_key";

export type IdentityAsset = {
  id: string;
  kind: IdentityAssetKind;
  title: string;
  subtitle: string;
  meta: string[];
  isActive: boolean;
  isRecommended: boolean;
  status: "healthy" | "warning" | "exhausted" | "auth_invalid" | "error" | "neutral";
  statusLabel: string;
  actionLabel: string;
  account: Account | null;
  profile: CredentialProfile;
};

function officialProfileForAccount(profiles: CredentialProfile[], account: Account) {
  return profiles.find(
    (profile) => profile.profile_kind === "official_account" && profile.linked_account_id === account.id,
  );
}

function keySubtitle(profile: CredentialProfile) {
  const usageTypeLabel = profile.usage_provider_type === "sub2api"
    ? "语聊统计"
    : profile.usage_provider_type === "new_api"
      ? "oneTop 统计"
      : null;
  return [profile.provider, profile.base_url, usageTypeLabel].filter(Boolean).join(" · ") || "第三方 Key";
}

function formatCompactCurrency(value: number | null | undefined, unit: string | null | undefined) {
  if (value === null || value === undefined || Number.isNaN(value)) return null;
  const normalizedUnit = unit?.trim() || "USD";
  if (normalizedUnit === "额度") {
    return `${Math.round(value)} ${normalizedUnit}`;
  }
  return `${normalizedUnit} ${value.toFixed(2)}`;
}

function keyBalanceLabel(profile: CredentialProfile) {
  const usage = profile.usage_summary;
  if (usage?.status !== "ready") return null;
  return formatCompactCurrency(usage.remaining ?? usage.balance, usage.unit);
}

function keyMeta(profile: CredentialProfile) {
  const usage = profile.usage_summary;
  const usageMeta = usage?.status === "ready"
    ? [
      formatCompactCurrency(usage.remaining ?? usage.balance, usage.unit),
      usage.detail_items[0]?.value ?? null,
    ]
    : [];

  return [
    profile.model ?? "未设置模型",
    profile.masked_secret ?? "未保存 key",
    ...usageMeta,
  ].filter(Boolean) as string[];
}

export function buildIdentityAssets({
  accounts,
  credentialProfiles,
  recommendedAccountId,
}: {
  accounts: Account[];
  credentialProfiles: CredentialProfile[];
  recommendedAccountId: number | null | undefined;
}): IdentityAsset[] {
  const officialAssets = [...accounts].sort(compareAccounts).map((account) => {
    const profile = officialProfileForAccount(credentialProfiles, account) ?? {
      id: -account.id,
      profile_kind: "official_account",
      provider: account.provider,
      nickname: account.nickname,
      status: account.status,
      is_active: account.is_active,
      base_url: null,
      model: null,
      masked_secret: null,
      secret_ref: account.session_ref,
      linked_account_id: account.id,
      usage_provider_type: null,
      usage_query_user: null,
      usage_query_app_version: null,
      usage_masked_secret: null,
      usage_summary: null,
    };

    return {
      id: `account:${account.id}`,
      kind: "official_account" as const,
      title: account.nickname,
      subtitle: account.account_email ?? account.profile_ref ?? "未读取到邮箱",
      meta: [
        "官方账号",
        accountStatusCompactText(account),
      ],
      isActive: account.is_active,
      isRecommended: recommendedAccountId === account.id,
      status: account.status,
      statusLabel: statusText[account.status],
      actionLabel: account.is_active ? "当前账号" : "切换并采样",
      account,
      profile,
    };
  });

  const keyAssets = credentialProfiles
    .filter((profile) => profile.profile_kind === "third_party_key")
    .map((profile) => ({
      id: `key:${profile.id}`,
      kind: "third_party_key" as const,
      title: profile.nickname,
      subtitle: keySubtitle(profile),
      meta: keyMeta(profile),
      isActive: profile.is_active,
      isRecommended: false,
      status: "neutral" as const,
      statusLabel: profile.is_active ? "当前 Key" : "可启用",
      actionLabel: profile.is_active ? "当前 Key" : "启用",
      account: null,
      profile,
    }));

  return [...officialAssets, ...keyAssets].sort((left, right) => {
    if (left.isActive !== right.isActive) return left.isActive ? -1 : 1;
    if (left.isRecommended !== right.isRecommended) return left.isRecommended ? -1 : 1;
    if (left.isActive && right.isActive && left.kind !== right.kind) {
      return left.kind === "third_party_key" ? -1 : 1;
    }
    if (left.kind !== right.kind) return left.kind === "official_account" ? -1 : 1;
    return left.title.localeCompare(right.title, "zh-Hans-CN");
  });
}

export function activeIdentityAsset(assets: IdentityAsset[]) {
  return assets.find((asset) => asset.kind === "third_party_key" && asset.isActive)
    ?? assets.find((asset) => asset.isActive)
    ?? null;
}

export function bestKeyFallbackIdentity(assets: IdentityAsset[]) {
  return assets.find((asset) => asset.kind === "third_party_key" && !asset.isActive) ?? null;
}

export function recommendedIdentityAsset({
  assets,
  recommendedAccountId,
}: {
  assets: IdentityAsset[];
  recommendedAccountId: number | null | undefined;
}) {
  return assets.find((asset) => asset.account?.id === recommendedAccountId) ?? bestKeyFallbackIdentity(assets);
}

export function dashboardIdentityCandidates({
  assets,
  activeIdentity,
  recommendedIdentity,
  canSwitch,
}: {
  assets: IdentityAsset[];
  activeIdentity: IdentityAsset | null;
  recommendedIdentity: IdentityAsset | null;
  canSwitch: (account: Account) => boolean;
}) {
  return assets.filter((asset) => {
    if (asset.id === activeIdentity?.id || asset.id === recommendedIdentity?.id) return true;
    if (asset.kind === "third_party_key") return !asset.isActive;
    return Boolean(asset.account && canSwitch(asset.account));
  });
}

export function identityKindLabel(asset: IdentityAsset | null | undefined) {
  if (!asset) return "未设置身份";
  return asset.kind === "third_party_key" ? "Key" : "官方账号";
}

export function identityShellSubtitle(asset: IdentityAsset | null | undefined) {
  if (!asset) return "当前：未设置身份";
  if (asset.kind === "third_party_key") {
    const model = asset.profile.model ? ` · ${asset.profile.model}` : "";
    return `当前：Key · ${asset.title}${model}`;
  }
  return `当前：官方账号 · ${asset.title}`;
}

export function identitySummaryText(asset: IdentityAsset | null | undefined) {
  if (!asset) return "暂无活跃身份";
  if (asset.kind === "third_party_key") {
    return `${asset.title} · Key · ${asset.profile.model ?? "未设置模型"}`;
  }
  return `${asset.title} · ${asset.account ? accountStatusCompactText(asset.account) : "官方账号"}`;
}

export function identityUsageValue(asset: IdentityAsset, key: "window_5h_percent" | "window_7d_percent") {
  if (asset.kind === "third_party_key") {
    if (key === "window_5h_percent") {
      return keyBalanceLabel(asset.profile) ?? "--";
    }
    return "";
  }
  return accountUsagePercent(asset.account, key);
}

export function identityResetValue(asset: IdentityAsset, key: "estimated_reset_5h_at" | "estimated_reset_7d_at") {
  if (asset.kind === "third_party_key") return "不适用";
  return accountResetTime(asset.account, key);
}

export function identityCanActivate(asset: IdentityAsset, canSwitch: (account: Account) => boolean) {
  if (asset.isActive) return false;
  if (asset.kind === "third_party_key") return true;
  return Boolean(asset.account && canSwitch(asset.account));
}

export function identityRiskRank(asset: IdentityAsset) {
  if (asset.kind === "third_party_key") return asset.isActive ? 0 : 2;
  if (!asset.account) return 4;
  if (asset.account.is_active) return 0;
  if (!hasAuthIssue(asset.account) && (asset.account.status === "healthy" || asset.account.status === "warning")) return 1;
  if (hasAuthIssue(asset.account) || asset.account.status === "error") return 3;
  return 4;
}
