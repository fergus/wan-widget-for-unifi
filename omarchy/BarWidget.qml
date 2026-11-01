import QtQuick
import qs.Commons
import qs.Ui

// Bar pill for the UniFi WAN plugin: live download and upload rates, with the
// detail popup (graph, usage, IP, uptime, latency) loaded from Panel.qml.
BarWidget {
  id: root
  moduleName: "io.github.fergus.wan-widget-for-unifi"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false
  readonly property bool popoutSwitchClosing: panelLoader.item ? panelLoader.item.popoutSwitchClosing === true : false

  function injectPanel() {
    var p = panelLoader.item
    if (!p) return
    if ("bar" in p) p.bar = root.bar
    if ("settings" in p) p.settings = root.settings
    if ("anchorItem" in p) p.anchorItem = button
    if ("hostWidget" in p) p.hostWidget = root
  }

  function open() { if (panelLoader.item) panelLoader.item.open() }
  function close() { if (panelLoader.item) panelLoader.item.close() }
  function togglePanel() { if (panelLoader.item) panelLoader.item.toggle() }
  function closeForPopoutSwitch() { if (panelLoader.item) panelLoader.item.closeForPopoutSwitch() }

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("Panel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  // Vertical bars get a glyph only; the rates would not fit the width.
  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: panelLoader.item ? (root.vertical ? panelLoader.item.glyph : panelLoader.item.label) : ""
    tooltipText: panelLoader.item ? panelLoader.item.tooltip : ""
    dimmed: panelLoader.item ? panelLoader.item.stale : false

    onPressed: function(b) {
      if (b === Qt.MiddleButton && panelLoader.item) panelLoader.item.refreshNow()
      else root.togglePanel()
    }
  }
}
