import { toForm, fromForm, errorList } from "./settings-form.js";
import { applyTheme } from "./theme.js";

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
    timezone: "",
    ip_lookup_url: "https://api.ipify.org",
    accept_invalid_certs: true,
    theme: "system",
    notify: { wan: true, ip: true, gateway: true, update: true },
  }),
  set_api_key: async () => null,
  save_settings: async () => null,
  test_connection: async () => "Connected (preview): 0 B/s down, 0 B/s up. Gateway timezone: Australia/Sydney",
  get_autostart: async () => false,
  set_autostart: async () => null,
  get_pinned: async () => false,
  set_pinned: async () => null,
  forget_pin: async () => null,
  app_version: async () => "0.1.0",
  discover_gateways: async () => [{ url: "https://192.168.1.1", model: "UDRULT" }],
  list_sites: async () => [{ name: "default", desc: "Default" }],
  check_update: async () => null,
  install_update: async () => null,
  open_log_folder: async () => null,
};
const invoke = tauri && tauri.core ? tauri.core.invoke : (cmd, args) => stub[cmd](args);

const FIELDS = ["gatewayUrl", "site", "pollSeconds", "billingDay", "timezone", "ipLookupUrl", "theme"];
const CHECKS = ["lookupEnabled", "acceptInvalidCerts", "notifyWan", "notifyIp", "notifyGateway", "notifyUpdate"];

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

// Autostart is OS state (registry), not part of settings.json, so it is read and
// written separately. The tray menu can change it too, hence the refresh on focus.
let autostartKnown = false;

async function refreshAutostart() {
  autostartKnown = await invoke("get_autostart");
  $("startWithWindows").checked = autostartKnown;
}

// Pin state is also kept in window.json and mirrored in the tray menu.
let pinnedKnown = false;

async function refreshPinned() {
  pinnedKnown = await invoke("get_pinned");
  $("pinToDesktop").checked = pinnedKnown;
}

function showUpdate(version, message) {
  $("installUpdate").hidden = !version;
  $("installUpdate").textContent = version ? "Install " + version + " and restart" : "Install and restart";
  if (message) $("updateStatus").textContent = message;
}

async function onCheckUpdate() {
  $("checkUpdate").disabled = true;
  $("updateStatus").textContent = "Checking...";
  try {
    const version = await invoke("check_update");
    showUpdate(version, version ? "Version " + version + " is available." : "You have the latest version.");
  } catch (err) {
    showUpdate(null, "Could not check for updates: " + errorList(err).join("; "));
  } finally {
    $("checkUpdate").disabled = false;
  }
}

async function onInstallUpdate() {
  setBusy(true);
  $("updateStatus").textContent = "Downloading the update. The widget restarts when it is installed.";
  try {
    await invoke("install_update");
  } catch (err) {
    $("updateStatus").textContent = "Update failed: " + errorList(err).join("; ");
    setBusy(false);
  }
}

function showFound(found) {
  const list = $("foundList");
  list.replaceChildren(
    ...found.map((f) => {
      const b = Object.assign(document.createElement("button"), {
        type: "button",
        textContent: f.url.replace("https://", "") + (f.model ? " (" + f.model + ")" : ""),
        title: "Use " + f.url,
      });
      b.addEventListener("click", () => {
        $("gatewayUrl").value = f.url;
        $("findStatus").textContent = "Using " + f.url + ". Save to keep it.";
      });
      return b;
    })
  );
  list.hidden = found.length === 0;
}

async function onFindGateway() {
  $("findGateway").disabled = true;
  $("findStatus").textContent = "Looking for UniFi consoles...";
  try {
    const found = await invoke("discover_gateways");
    showFound(found);
    $("findStatus").textContent =
      found.length === 0
        ? "None found. Enter the address shown in the UniFi app."
        : found.length === 1
          ? "Found one. Click it to use it."
          : "Found " + found.length + ". Click one to use it.";
    return found;
  } finally {
    $("findGateway").disabled = false;
  }
}

/** Fills the site picker from the gateway. Quietly does nothing if it cannot. */
async function refreshSites(settings, key) {
  try {
    const sites = await invoke("list_sites", { settings, key: key || null });
    $("sites").replaceChildren(
      ...sites.map((x) => Object.assign(document.createElement("option"), { value: x.name, label: x.desc }))
    );
    if (sites.length > 1) $("siteHint").textContent = "Your key can see " + sites.length + " sites. Pick one from the list.";
    else if (sites.length === 1) $("siteHint").textContent = 'The only site is "' + sites[0].name + '" (' + sites[0].desc + ").";
  } catch {
    // Not reachable yet, or no key: leave the field as typed.
  }
}

async function load() {
  const [settings, stored, version] = await Promise.all([
    invoke("get_settings"),
    refreshKeyHint(),
    invoke("app_version"),
    refreshAutostart(),
    refreshPinned(),
  ]);
  $("appVersion").textContent = version;
  writeForm(toForm(settings));
  applyTheme(settings.theme);
  if (!stored) {
    showResult("Enter your UniFi API key to start.", false);
    // First run: look for the gateway so the address is likely right already.
    onFindGateway()
      .then((found) => {
        if (found.length === 1 && $("gatewayUrl").value !== found[0].url) {
          $("gatewayUrl").value = found[0].url;
          $("findStatus").textContent = "Found your gateway at " + found[0].url + ".";
        }
      })
      .catch(() => {});
  } else {
    refreshSites(settings, null);
  }
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
    const wantAutostart = $("startWithWindows").checked;
    if (wantAutostart !== autostartKnown) {
      await invoke("set_autostart", { enabled: wantAutostart });
      autostartKnown = wantAutostart;
    }
    const wantPinned = $("pinToDesktop").checked;
    if (wantPinned !== pinnedKnown) {
      await invoke("set_pinned", { pinned: wantPinned });
      pinnedKnown = wantPinned;
    }
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
    refreshSites(settings, key);
  } catch (err) {
    showResult(errorList(err).join("; "), false);
  } finally {
    setBusy(false);
  }
}

async function onForgetPin() {
  showResult("");
  try {
    await invoke("forget_pin");
    showResult("Pinned certificate forgotten. The next connection pins the one it presents.", true);
  } catch (err) {
    showResult(errorList(err).join("; "), false);
  }
}

$("form").addEventListener("submit", onSave);
$("test").addEventListener("click", onTest);
$("forgetPin").addEventListener("click", onForgetPin);
$("checkUpdate").addEventListener("click", onCheckUpdate);
$("findGateway").addEventListener("click", () => onFindGateway().catch(() => {}));
$("installUpdate").addEventListener("click", onInstallUpdate);
$("openLogs").addEventListener("click", () =>
  invoke("open_log_folder").catch((err) => showResult(errorList(err).join("; "), false))
);
// The daily background check announces what it finds.
if (tauri && tauri.event) {
  tauri.event.listen("update-available", (e) => showUpdate(e.payload, "Version " + e.payload + " is available."));
}
$("lookupEnabled").addEventListener("change", syncLookup);
// Preview the chosen theme straight away; the value is saved with the other settings.
$("theme").addEventListener("change", () => applyTheme($("theme").value));
$("close").addEventListener("click", () => {
  if (tauri && tauri.window) tauri.window.getCurrentWindow().hide();
  else window.close();
});
window.addEventListener("focus", () => {
  // Do not clobber an unsaved change the user has made to the checkbox.
  if ($("startWithWindows").checked === autostartKnown) refreshAutostart().catch(() => {});
  if ($("pinToDesktop").checked === pinnedKnown) refreshPinned().catch(() => {});
});
load().catch((err) => showErrors(errorList(err)));
