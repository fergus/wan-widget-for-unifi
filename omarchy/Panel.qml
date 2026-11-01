import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Popup and data source for the UniFi WAN plugin. Polls the gateway through
// wan-fetch (which holds the API key and pins the certificate) and draws the same
// numbers as the Windows widget: throughput graph, month usage, IP, uptime, latency.
Panel {
  id: root
  moduleName: "io.github.fergus.wan-widget-for-unifi"
  ipcTarget: "io.github.fergus.wan-widget-for-unifi"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  readonly property var barIdentity: hostWidget || root

  readonly property string fetchScript: Qt.resolvedUrl("wan-fetch").toString().replace("file://", "")
  readonly property string keyFile: Quickshell.env("HOME") + "/.config/wan-widget-for-unifi/api-key"

  // ---- settings ------------------------------------------------------------
  readonly property string gatewayUrl: String(setting("gatewayUrl", "https://192.168.1.1"))
  readonly property string site: String(setting("site", "default"))
  readonly property int billingDay: Math.max(1, Math.min(31, parseInt(setting("billingDay", 1), 10) || 1))
  readonly property int pollMs: Math.max(1, Math.min(60, parseInt(setting("pollSeconds", 3), 10) || 3)) * 1000
  readonly property string ipLookupUrl: String(setting("ipLookupUrl", "https://api.ipify.org"))
  readonly property string siteBase: "/proxy/network/api/s/" + site

  // ---- state ---------------------------------------------------------------
  property var health: null
  property var device: null
  property var monthly: null
  property string publicIp: ""
  property var live: []            // {t, rx, tx} samples, at most an hour
  property var history: []         // rate points for the 24h / 7d windows
  property string graphWindow: "5m"
  property string problem: "Connecting..."   // empty once data flows
  property double lastOkMs: 0
  property double nowMs: Date.now()
  property real hoverX: -1
  property bool editingKey: false
  readonly property bool keyMissing: problem.indexOf("No API key") === 0

  readonly property bool stale: problem !== "" || (nowMs - lastOkMs) > 15000
  readonly property string glyph: "󰛳"   // nf-md-lan-connect
  readonly property string label: health && !stale
    ? "↓" + Model.formatRateShort(health.rx) + " ↑" + Model.formatRateShort(health.tx)
    : glyph + " --"
  readonly property string tooltip: problem !== "" ? problem
    : "UniFi WAN  ↓ " + Model.formatRate(health ? health.rx : 0) + "  ↑ " + Model.formatRate(health ? health.tx : 0)

  readonly property color fg: Color.popups.text
  readonly property color muted: Util.alpha(fg, 0.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  function open() { controller.show(); refreshNow() }
  function close() { controller.hide() }
  function toggle() { opened ? close() : open() }
  function switchPanel(direction) {
    if (bar && typeof bar.switchPanelFrom === "function")
      return bar.switchPanelFrom(barIdentity, direction)
    return false
  }

  function refreshNow() {
    pollHealth()
    pollDevice()
    pollMonthly()
    pollIp()
    if (graphWindow === "24h" || graphWindow === "7d") pollHistory()
  }

  // ---- fetching ------------------------------------------------------------
  // One curl at a time per endpoint; overlapping polls are dropped.
  component Fetcher: Process {
    id: fetcher
    property string reply: ""
    signal got(string text)
    signal failed(int code, string message)
    function run(method, path, body) {
      if (running) return
      var cmd = [root.fetchScript, method, root.gatewayUrl, path]
      if (body) cmd.push(body)
      command = cmd
      running = true
    }
    stdout: StdioCollector { id: out; waitForEnd: true }
    stderr: StdioCollector { id: err; waitForEnd: true }
    onExited: function(code) {
      if (code === 0) fetcher.got(out.text)
      else fetcher.failed(code, err.text.trim())
    }
  }

  function saveKey(text) {
    var key = text.trim()
    if (key === "") return
    keyProc.key = key
    keyProc.running = true
  }

  Process {
    id: keyProc
    property string key: ""
    command: [root.fetchScript, "setkey"]
    stdinEnabled: true
    onStarted: { write(key + "\n"); stdinEnabled = false }
    onExited: function(code) {
      key = ""
      stdinEnabled = true
      if (code === 0) {
        root.editingKey = false
        root.problem = "Connecting..."
        root.refreshNow()
      } else {
        root.problem = "Could not save the API key"
      }
    }
  }

  function noteFailure(code, message) {
    if (code === 10) problem = "No API key. Paste it below (saved to " + keyFile + ")"
    else if (code === 11) problem = "Cannot read the gateway certificate at " + gatewayUrl
    else if (code === 60 || code === 90) problem = "Gateway certificate changed. Run: " + fetchScript + " forget"
    else problem = message !== "" ? message : "Gateway unreachable"
  }

  function pollHealth() { healthFetch.run("GET", siteBase + "/stat/health", "") }
  function pollDevice() { deviceFetch.run("GET", siteBase + "/stat/device", "") }
  function pollMonthly() {
    var now = Date.now()
    var start = Model.cycleStart(now, billingDay)
    monthlyFetch.cycleStart = start
    monthlyFetch.run("POST", siteBase + "/stat/report/daily.site", Model.reportBody(start, now))
  }
  function pollHistory() {
    var now = Date.now()
    var week = graphWindow === "7d"
    historyFetch.bucketS = week ? 3600 : 300
    historyFetch.run("POST", siteBase + "/stat/report/" + (week ? "hourly" : "5minutes") + ".site",
      Model.reportBody(now - (week ? 7 * 24 : 24) * 3600 * 1000, now))
  }
  function pollIp() {
    if (ipLookupUrl === "") { publicIp = ""; return }
    ipProc.command = ["curl", "--silent", "--max-time", "8", ipLookupUrl]
    if (!ipProc.running) ipProc.running = true
  }

  Fetcher {
    id: healthFetch
    onGot: function(text) {
      var h = Model.parseHealth(text)
      if (!h) { root.problem = "Unexpected reply: check the site name and API key"; return }
      var now = Date.now()
      root.health = h
      root.problem = ""
      root.lastOkMs = now
      root.nowMs = now
      root.live = Model.pushSample(root.live, { t: now, rx: h.rx, tx: h.tx }, 3600)
    }
    onFailed: function(code, message) { root.noteFailure(code, message) }
  }

  Fetcher {
    id: deviceFetch
    onGot: function(text) { var d = Model.parseDevice(text); if (d) root.device = d }
  }

  Fetcher {
    id: monthlyFetch
    property double cycleStart: 0
    onGot: function(text) {
      var b = Model.parseReport(text)
      if (b) root.monthly = Model.monthlyTotals(b, cycleStart)
    }
  }

  Fetcher {
    id: historyFetch
    property int bucketS: 300
    onGot: function(text) {
      var b = Model.parseReport(text)
      if (b) root.history = Model.bucketRates(b, bucketS, Date.now())
    }
  }

  Process {
    id: ipProc
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        var ip = text.trim()
        // Only keep something that looks like an address, never an error page.
        if (/^[0-9a-fA-F:.]{3,45}$/.test(ip)) root.publicIp = ip
      }
    }
  }

  Timer {
    interval: root.pollMs
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: { root.nowMs = Date.now(); root.pollHealth() }
  }
  Timer {
    interval: 60000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.pollDevice()
  }
  Timer {
    interval: 300000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: { root.pollMonthly(); root.pollIp(); if (root.graphWindow === "24h" || root.graphWindow === "7d") root.pollHistory() }
  }

  onGraphWindowChanged: {
    hoverX = -1
    if (graphWindow === "24h" || graphWindow === "7d") { history = []; pollHistory() }
  }

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function refresh(): void { root.refreshNow() }
  }

  // ---- graph data ----------------------------------------------------------
  readonly property var windows: [
    { id: "5m", ms: 5 * 60 * 1000 }, { id: "1h", ms: 3600 * 1000 },
    { id: "24h", ms: 24 * 3600 * 1000 }, { id: "7d", ms: 7 * 24 * 3600 * 1000 }
  ]
  readonly property double windowMs: {
    for (var i = 0; i < windows.length; i++) if (windows[i].id === graphWindow) return windows[i].ms
    return 300000
  }
  readonly property var series: {
    var src = (graphWindow === "24h" || graphWindow === "7d") ? history : live
    var from = nowMs - windowMs
    return src.filter(function(s) { return s.t >= from })
  }
  readonly property var yAxis: Model.yScale(series)

  function fmtTime(ms) {
    var d = new Date(ms)
    var p = function(n) { return (n < 10 ? "0" : "") + n }
    var hm = p(d.getHours()) + ":" + p(d.getMinutes())
    return graphWindow === "7d" ? Qt.formatDate(d, "ddd") + " " + hm : (graphWindow === "5m" ? hm + ":" + p(d.getSeconds()) : hm)
  }

  function nearest(px, w) {
    if (!series.length) return null
    var t = nowMs - windowMs + (px / w) * windowMs
    var best = series[0]
    for (var i = 1; i < series.length; i++)
      if (Math.abs(series[i].t - t) < Math.abs(best.t - t)) best = series[i]
    return best
  }

  // ---- popup ---------------------------------------------------------------
  readonly property bool cgnat: Model.isCgnat(health ? health.wanIp : "") && publicIp !== ""

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    bar: root.bar
    owner: root.barIdentity
    open: root.opened
    focusTarget: keys
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(body.implicitHeight)

    PanelKeyCatcher {
      id: keys
      anchors.fill: parent
      blocked: keyField.activeFocus
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: body
        width: parent.width
        spacing: Style.space(10)

        // Download and upload now.
        Row {
          width: parent.width
          Column {
            width: parent.width / 2
            Text {
              textFormat: Text.PlainText
              text: "DOWNLOAD"
              color: root.muted
              font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true
            }
            Text {
              textFormat: Text.PlainText
              text: root.health ? Model.formatRate(root.health.rx) : "-"
              color: Color.accent
              font.family: root.fontFamily; font.pixelSize: Style.font.title; font.bold: true
            }
          }
          Column {
            width: parent.width / 2
            Text {
              textFormat: Text.PlainText
              text: "UPLOAD"
              color: root.muted
              font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true
            }
            Text {
              textFormat: Text.PlainText
              text: root.health ? Model.formatRate(root.health.tx) : "-"
              color: root.fg
              font.family: root.fontFamily; font.pixelSize: Style.font.title; font.bold: true
            }
          }
        }

        // Window buttons.
        Row {
          spacing: Style.spacing.xs
          Repeater {
            model: root.windows
            Button {
              required property var modelData
              text: modelData.id
              fontSize: Style.font.caption
              foreground: root.fg
              fontFamily: root.fontFamily
              horizontalPadding: Style.spacing.sm
              verticalPadding: Style.spacing.controlPaddingY
              bordered: true
              active: root.graphWindow === modelData.id
              onClicked: root.graphWindow = modelData.id
            }
          }
        }

        // Graph.
        Item {
          id: graphBox
          width: parent.width
          height: Style.space(120)

          Canvas {
            id: canvas
            anchors.fill: parent
            onPaint: {
              var ctx = getContext("2d")
              ctx.reset()
              var w = width, h = height
              ctx.strokeStyle = Util.alpha(root.fg, 0.15)
              ctx.lineWidth = 1
              ctx.beginPath()
              ctx.moveTo(0, 0.5); ctx.lineTo(w, 0.5)
              ctx.moveTo(0, h / 2); ctx.lineTo(w, h / 2)
              ctx.moveTo(0, h - 0.5); ctx.lineTo(w, h - 0.5)
              ctx.stroke()
              var s = root.series
              if (s.length < 1) return
              var t0 = root.nowMs - root.windowMs
              function x(t) { return (t - t0) / root.windowMs * w }
              function y(v) { return h - Math.min(1, v / root.yAxis.top) * (h - 2) - 1 }
              function line(key, color, fill) {
                ctx.beginPath()
                for (var i = 0; i < s.length; i++) {
                  if (i === 0) ctx.moveTo(x(s[i].t), y(s[i][key]))
                  else ctx.lineTo(x(s[i].t), y(s[i][key]))
                }
                ctx.strokeStyle = color
                ctx.lineWidth = 1.5
                ctx.stroke()
                if (fill) {
                  ctx.lineTo(x(s[s.length - 1].t), h)
                  ctx.lineTo(x(s[0].t), h)
                  ctx.closePath()
                  ctx.fillStyle = fill
                  ctx.fill()
                }
              }
              line("rx", Color.accent, Util.alpha(Color.accent, 0.18))
              line("tx", Util.alpha(root.fg, 0.7), null)
              if (root.hoverX >= 0) {
                ctx.strokeStyle = Util.alpha(root.fg, 0.4)
                ctx.beginPath(); ctx.moveTo(root.hoverX, 0); ctx.lineTo(root.hoverX, h); ctx.stroke()
              }
            }
          }

          Connections {
            target: root
            function onSeriesChanged() { canvas.requestPaint() }
            function onNowMsChanged() { canvas.requestPaint() }
            function onHoverXChanged() { canvas.requestPaint() }
            function onFgChanged() { canvas.requestPaint() }
          }

          Text {
            textFormat: Text.PlainText
            anchors.left: parent.left; anchors.top: parent.top
            text: root.yAxis.topLabel
            color: root.muted
            font.family: root.fontFamily; font.pixelSize: Style.font.caption
          }
          Text {
            textFormat: Text.PlainText
            anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
            text: root.yAxis.halfLabel
            color: root.muted
            font.family: root.fontFamily; font.pixelSize: Style.font.caption
          }

          MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            onPositionChanged: function(m) { root.hoverX = m.x }
            onExited: root.hoverX = -1
          }
        }

        // Hover readout, or the window label when idle.
        Text {
          textFormat: Text.PlainText
          width: parent.width
          elide: Text.ElideRight
          color: root.muted
          font.family: root.fontFamily; font.pixelSize: Style.font.caption
          text: {
            if (root.problem !== "") return root.problem
            var p = root.hoverX >= 0 ? root.nearest(root.hoverX, graphBox.width) : null
            if (p) return root.fmtTime(p.t) + "   ↓ " + Model.formatRate(p.rx) + "   ↑ " + Model.formatRate(p.tx)
            return root.series.length ? "Last " + root.graphWindow : "Waiting for data..."
          }
        }

        PanelSeparator { foreground: root.fg }

        // API key entry: shown while no key is saved, or on request.
        Column {
          width: parent.width
          spacing: Style.space(6)
          visible: root.keyMissing || root.editingKey
          onVisibleChanged: if (visible) keyField.forceActiveFocus()

          Text {
            textFormat: Text.PlainText
            text: "UNIFI API KEY"
            color: root.muted
            font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true
          }
          Row {
            width: parent.width
            spacing: Style.spacing.xs
            TextField {
              id: keyField
              width: parent.width - saveButton.width - parent.spacing
              password: true
              placeholderText: "Paste key, press Enter"
              foreground: root.fg
              font.family: root.fontFamily
              onAccepted: { root.saveKey(text); text = "" }
              Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Escape) { text = ""; root.editingKey = false; event.accepted = true }
              }
            }
            Button {
              id: saveButton
              text: "Save"
              fontSize: Style.font.caption
              foreground: root.fg
              fontFamily: root.fontFamily
              horizontalPadding: Style.spacing.sm
              verticalPadding: Style.spacing.controlPaddingY
              bordered: true
              onClicked: { root.saveKey(keyField.text); keyField.text = "" }
            }
          }
          PanelSeparator { foreground: root.fg }
        }

        Repeater {
          model: [
            { k: "Month", v: root.monthly ? Model.formatBytes(root.monthly.rx + root.monthly.tx) + " used" : "-",
              d: root.monthly ? "↓ " + Model.formatBytes(root.monthly.rx) + "   ↑ " + Model.formatBytes(root.monthly.tx) : "" },
            { k: "Uptime", v: root.device && root.device.wanS !== null ? Model.formatDuration(root.device.wanS) : "-",
              d: root.device && root.device.deviceS !== null ? "gateway up " + Model.formatDuration(root.device.deviceS) : "" },
            { k: "IP", v: root.cgnat ? root.publicIp : (root.health && root.health.wanIp !== "" ? root.health.wanIp : (root.publicIp || "-")),
              d: root.cgnat ? "CGNAT, gateway WAN " + root.health.wanIp : (root.health ? root.health.ispName : "") },
            { k: "Latency", v: root.health ? Model.formatLatency(root.health.latencyMs) : "-", d: root.latencyDetail() }
          ]
          Item {
            required property var modelData
            width: body.width
            implicitHeight: Math.max(key.implicitHeight, detailColumn.height)
            Text {
              id: key
              textFormat: Text.PlainText
              text: modelData.k.toUpperCase()
              color: root.muted
              font.family: root.fontFamily; font.pixelSize: Style.font.caption; font.bold: true
              width: Style.space(72)
            }
            Column {
              id: detailColumn
              anchors.left: key.right; anchors.right: parent.right
              Text {
                id: val
                textFormat: Text.PlainText
                text: modelData.v
                color: root.fg
                font.family: root.fontFamily; font.pixelSize: Style.font.body
              }
              Text {
                id: detail
                textFormat: Text.PlainText
                visible: modelData.d !== ""
                text: modelData.d
                width: parent.width
                wrapMode: Text.WordWrap
                color: root.muted
                font.family: root.fontFamily; font.pixelSize: Style.font.caption
              }
            }
          }
        }

        Button {
          visible: !root.keyMissing && !root.editingKey
          text: "Change API key"
          fontSize: Style.font.caption
          foreground: root.fg
          fontFamily: root.fontFamily
          horizontalPadding: Style.spacing.sm
          verticalPadding: Style.spacing.controlPaddingY
          bordered: true
          onClicked: { root.editingKey = true; keyField.forceActiveFocus() }
        }
      }
    }
  }

  // Per-target latency and 24 h availability, as in the Windows widget's tooltip.
  function latencyDetail() {
    if (!health) return ""
    var parts = health.monitors.map(function(m) { return Model.monitorName(m.target) + " " + Model.formatLatency(m.latencyMs) })
    if (health.availabilityPct !== null) parts.push(health.availabilityPct.toFixed(2) + "% up")
    return parts.join("\n")
  }
}
