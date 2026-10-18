import { toForm, fromForm, errorList } from "./settings-form.js";

const $ = (id) => document.getElementById(id);
const tauri = window.__TAURI__;

// Outside Tauri (plain browser preview) fall back to an in-memory stub.
const stub = {
  has_api_key: async () => false,
  get_settings: async () => ({
    gateway_url: "https://192.168.1.1",
    site: "default",
    poll_interval_ms: 2000,
    billing_day: 1,
    timezone: "Australia/Sydney",
    ip_lookup_url: "https://api.ipify.org",
    accept_invalid_certs: true,
  }),
  set_api_key: async () => null,
  save_settings: async () => null,
  test_connection: async () => "Connected (preview): 0 B/s down, 0 B/s up",
};
const invoke = tauri && tauri.core ? tauri.core.invoke : (cmd, args) => stub[cmd](args);

const FIELDS = ["gatewayUrl", "site", "pollSeconds", "billingDay", "timezone", "ipLookupUrl"];
const CHECKS = ["lookupEnabled", "acceptInvalidCerts"];

function readForm() {
  const f = {};
  for (const id of FIELDS) f[id] = $(id).value;
  for (const id of CHECKS) f[id] = $(id).checked;
  return f;
}

function writeForm(f) {
  for (const id of FIELDS) $(id).value = f[id];
  for (const id of CHECKS) $(id).checked = f[id];
  syncLookup();
}

function syncLookup() {
  $("ipLookupUrl").disabled = !$("lookupEnabled").checked;
}

function showErrors(list) {
  const ul = $("errors");
  ul.replaceChildren(...list.map((m) => Object.assign(document.createElement("li"), { textContent: m })));
  ul.hidden = list.length === 0;
}

function showResult(message, ok) {
  const p = $("result");
  p.hidden = !message;
  p.textContent = message || "";
  p.className = "result " + (ok ? "ok" : "fail");
}

function setBusy(busy) {
  for (const b of document.querySelectorAll("button")) b.disabled = busy;
}

function collect() {
  const { settings, errors } = fromForm(readForm());
  showErrors(errors);
  return settings;
}

async function refreshKeyHint() {
  const stored = await invoke("has_api_key");
  $("apiKey").placeholder = stored ? "Stored. Leave blank to keep it." : "Paste API key";
  return stored;
}

async function load() {
  const [settings, stored] = await Promise.all([invoke("get_settings"), refreshKeyHint()]);
  writeForm(toForm(settings));
  if (!stored) showResult("Enter your UniFi API key to start.", false);
  if (typeof Intl.supportedValuesOf === "function") {
    $("zones").replaceChildren(
      ...Intl.supportedValuesOf("timeZone").map((z) => Object.assign(document.createElement("option"), { value: z }))
    );
  }
}

async function onSave(e) {
  e.preventDefault();
  showResult("");
  const settings = collect();
  if (!settings) return;
  setBusy(true);
  try {
    const key = $("apiKey").value.trim();
    if (key) await invoke("set_api_key", { key });
    await invoke("save_settings", { settings });
    $("apiKey").value = "";
    await refreshKeyHint();
    showErrors([]);
    showResult("Saved. Changes apply on the next poll.", true);
  } catch (err) {
    showErrors(errorList(err));
  } finally {
    setBusy(false);
  }
}

async function onTest() {
  showResult("");
  const settings = collect();
  if (!settings) return;
  setBusy(true);
  try {
    const key = $("apiKey").value.trim();
    showErrors([]);
    showResult(await invoke("test_connection", { settings, key: key || null }), true);
  } catch (err) {
    showResult(errorList(err).join("; "), false);
  } finally {
    setBusy(false);
  }
}

$("form").addEventListener("submit", onSave);
$("test").addEventListener("click", onTest);
$("lookupEnabled").addEventListener("change", syncLookup);
$("close").addEventListener("click", () => {
  if (tauri && tauri.window) tauri.window.getCurrentWindow().hide();
  else window.close();
});
load().catch((err) => showErrors(errorList(err)));
