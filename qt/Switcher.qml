pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Popup {
    id: popup
    required property var frontend
    property string scope: "sessions"
    signal finished()
    width: Math.min(560, parent.width - 32)
    height: Math.min(540, parent.height - 32)
    anchors.centerIn: parent
    modal: true
    focus: true
    padding: 16
    property var rows: []
    function key(row) { return row ? [row.host, row.session || "", row.window || ""].join("/") : "" }
    function rebuild(reset) {
        if (!list) return
        const selected = key(rows[list.currentIndex])
        const result = []
        const needle = query.text.toLowerCase()
        const hosts = (popup.frontend.hosts || {}).rows || []
        const labelCounts = new Map()
        for (const host of hosts) labelCounts.set(host.label, (labelCounts.get(host.label) || 0) + 1)
        for (const host of hosts) {
            const collision = labelCounts.get(host.label) > 1
            const hostName = host.label + (collision ? " · " + host.endpoint : "")
            if (scope === "hosts") result.push({host: host.id, epoch: host.epoch, label: host.label, detail: [collision ? host.endpoint : "", host.connected ? "" : host.status].filter(Boolean).join(" · "), active: host.active})
            else for (const session of host.sessions) {
                if (scope === "sessions") result.push({host: host.id, epoch: host.epoch, session: session.id, label: session.label, activity: session.activity || {}, detail: hostName, active: host.active && session.selected})
                else for (const tab of session.tabs) result.push({host: host.id, epoch: host.epoch, session: session.id, window: tab.id, label: tab.label, activity: tab.activity || {}, detail: session.label + " · " + hostName, active: host.active && session.selected && tab.selected})
            }
        }
        rows = result.filter(row => (row.label + " " + row.detail).toLowerCase().includes(needle))
        list.currentIndex = reset ? (rows.length ? 0 : -1) : rows.findIndex(row => key(row) === selected)
    }
    Connections { target: popup.frontend; function onHostsChanged() { popup.rebuild(false) } }
    onScopeChanged: rebuild(true)
    function choose(index) {
        const row = rows[index]
        if (!row) return
        popup.frontend.hostAction(row.session ? "navigate" : "activate", {session: row.session || "", window: row.window || ""}, {host: row.host, epoch: row.epoch})
        close()
    }
    onOpened: { query.text = ""; rebuild(true); query.forceActiveFocus(); list.currentIndex = rows.findIndex(row => row.active); if (list.currentIndex < 0 && rows.length) list.currentIndex = 0 }
    onClosed: finished()
    contentItem: ColumnLayout {
        Label { text: popup.scope === "hosts" ? qsTr("Switch host") : popup.scope === "tabs" ? qsTr("Switch tab") : qsTr("Switch session"); font.bold: true }
        TextField {
            id: query
            objectName: "switcherSearch"
            Layout.fillWidth: true
            placeholderText: qsTr("Search by name or host")
            onTextChanged: popup.rebuild(true)
            onAccepted: popup.choose(list.currentIndex)
            Keys.onDownPressed: { if (list.currentIndex < list.count - 1) ++list.currentIndex }
            Keys.onUpPressed: { if (list.currentIndex > 0) --list.currentIndex }
        }
        ListView {
            id: list
            objectName: "switcherResults"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: popup.rows
            ScrollBar.vertical: ScrollBar {}
            delegate: ItemDelegate {
                id: resultRow
                required property var modelData
                required property int index
                width: list.width
                highlighted: ListView.isCurrentItem
                background: Rectangle {
                    radius: 3
                    color: resultRow.highlighted ? Qt.alpha(popup.palette.highlight, .18) : resultRow.down ? Qt.rgba(1, 1, 1, .11) : resultRow.hovered ? Qt.rgba(1, 1, 1, .07) : "transparent"
                    border.color: popup.palette.highlight
                    border.width: resultRow.highlighted ? 1 : 0
                }
                contentItem: ColumnLayout {
                    Label { Layout.fillWidth: true; color: popup.palette.windowText; text: resultRow.modelData.label + (((resultRow.modelData.activity || {}).word && !(resultRow.modelData.activity || {}).muted) ? " · " + resultRow.modelData.activity.word : ""); textFormat: Text.PlainText; elide: Text.ElideRight }
                    Label { Layout.fillWidth: true; color: popup.palette.windowText; text: resultRow.modelData.detail; textFormat: Text.PlainText; elide: Text.ElideMiddle; opacity: .7 }
                }
                ToolTip.text: modelData.label + (modelData.detail ? " · " + modelData.detail : "")
                ToolTip.visible: hovered
                onClicked: popup.choose(index)
            }
        }
        Label { visible: !popup.rows.length; text: qsTr("No matches"); opacity: .7 }
    }
}
