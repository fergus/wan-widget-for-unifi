// Pure conversion between the backend Settings object and the form fields.
// Range and format validation stays in Rust (Settings::validate); this only
// guards what cannot be represented in JSON (non-numbers) and unit changes.

/** Backend settings -> form values (poll interval is shown in seconds). */
export function toForm(s) {
  return {
    gatewayUrl: s.gateway_url,
    site: s.site,
    pollSeconds: String(s.poll_interval_ms / 1000),
    billingDay: String(s.billing_day),
    timezone: s.timezone,
    lookupEnabled: s.ip_lookup_url !== "",
    ipLookupUrl: s.ip_lookup_url,
    acceptInvalidCerts: s.accept_invalid_certs,
  };
}

function toNumber(text) {
  const t = String(text).trim();
  return t === "" ? NaN : Number(t);
}

/** Form values -> { settings, errors }. `settings` is null when errors is non-empty. */
export function fromForm(f) {
  const errors = [];
  const pollSeconds = toNumber(f.pollSeconds);
  const billingDay = toNumber(f.billingDay);
  if (!Number.isFinite(pollSeconds)) errors.push("Poll interval must be a number of seconds");
  if (!Number.isInteger(billingDay)) errors.push("Billing day must be a whole number");

  const url = String(f.ipLookupUrl).trim();
  if (f.lookupEnabled && url === "") {
    errors.push("Enter a lookup URL or untick external IP lookup");
  }
  if (errors.length) return { settings: null, errors };

  return {
    settings: {
      gateway_url: String(f.gatewayUrl).trim(),
      site: String(f.site).trim(),
      poll_interval_ms: Math.round(pollSeconds * 1000),
      billing_day: billingDay,
      timezone: String(f.timezone).trim(),
      ip_lookup_url: f.lookupEnabled ? url : "",
      accept_invalid_certs: Boolean(f.acceptInvalidCerts),
    },
    errors: [],
  };
}

/** Tauri rejects with whatever the command returned: a string, an array, or an Error. */
export function errorList(err) {
  if (Array.isArray(err)) return err.map(String);
  if (err instanceof Error) return [err.message];
  return [String(err)];
}
