// The default policy bundled with this release (ARCHITECTURE 7.1, 10).
// The app never replaces it with a node's /v1/policy unless the user chooses
// to on the Policy screen. `now: null` means "the time of each query".
//
// trusted_attestors lists the attestor of the demo dataset produced by
// `perch-node demo`. A real release lists real attestors here.

export const DEFAULT_POLICY = Object.freeze({
  now: null,
  trusted_attestors: [],
  flag_threshold: 3,
  flag_thresholds: { review: 3 },
  flags_require_verified: true,
  seller_hidden_listing_limit: 3,
  seller_min_distinct_flaggers: 9,
  verified_purchase_requires_verified_buyer: true,
  price_requires_domain_verified: true,
  future_ts_tolerance_ms: 600000,
});

export function defaultPolicy() {
  return structuredClone(DEFAULT_POLICY);
}
