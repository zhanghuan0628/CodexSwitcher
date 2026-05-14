import assert from "node:assert/strict";
import test from "node:test";

const moduleUrl = process.env.IDENTITY_POLICY_MODULE;
if (!moduleUrl) {
  throw new Error("IDENTITY_POLICY_MODULE is required");
}

const { refreshPlanForIdentityAction } = await import(moduleUrl);

test("official account switch refreshes identity state without foreground sampling", () => {
  assert.deepEqual(refreshPlanForIdentityAction("switch-official-account"), {
    overview: true,
    credentialProfiles: true,
    keyUsage: false,
    supportingData: true,
    sampling: false,
  });
});

test("third-party key activation refreshes both identity sources without key usage fetch", () => {
  assert.deepEqual(refreshPlanForIdentityAction("activate-third-party-key"), {
    overview: true,
    credentialProfiles: true,
    keyUsage: false,
    supportingData: false,
    sampling: false,
  });
});

test("manual status refresh stays lightweight and separate from explicit sampling", () => {
  assert.deepEqual(refreshPlanForIdentityAction("refresh-status"), {
    overview: true,
    credentialProfiles: true,
    keyUsage: false,
    supportingData: false,
    sampling: false,
  });

  assert.equal(refreshPlanForIdentityAction("sample-now").sampling, true);
});
