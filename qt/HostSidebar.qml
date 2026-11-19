import "IconPolicy.js" as IconPolicy
pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Pane {
    id: sidebar
    required property var frontend
    signal editHost(string host)
    signal removeHost(string host)
    signal addHost()
    signal navigated()
    property var inventory: sidebar.frontend.hosts || ({rows: []})
    property var menuTarget: ({})
    readonly property bool menuOpen: hostMenu.visible
    property var rows: []
    readonly property var iconPrefs: (frontend.preferences || {}).interface || ({})
    readonly property int iconCount: iconPrefs.tab_icon_count === undefined ? 1 : iconPrefs.tab_icon_count
    function colored(presentation, index) {
        const mode = iconPrefs.tab_icon_color || "never"
        return IconPolicy.colored(mode, presentation.inFront, index)
    }
    function key(row) { return row ? [row.kind, row.host, row.session || "", row.window || ""].join("/") : "" }
    function rebuild(reset) {
        if (!list) return
        const selected = key(rows[list.currentIndex])
        const result = []
        const needle = search.text.toLowerCase()
        const labelCounts = new Map()
        for (const host of inventory.rows || []) labelCounts.set(host.label, (labelCounts.get(host.label) || 0) + 1)
        for (const host of inventory.rows || []) {
            const collision = labelCounts.get(host.label) > 1
            const hostName = host.label + (collision ? " · " + host.endpoint : "")
            const hostMatch = host.label.toLowerCase().includes(needle)
            const children = []
            for (const session of host.sessions) {
                const sessionMatch = hostMatch || session.label.toLowerCase().includes(needle)
                const tabs = session.tabs.filter(tab => sessionMatch || tab.label.toLowerCase().includes(needle))
                if (sessionMatch || tabs.length) {
                    children.push({kind: "session", host: host.id, epoch: host.epoch, session: session.id,
                        label: session.label, subtitle: hostName, activity: session.activity || {}, selected: host.active && session.selected})
                    if (inventory.vertical_tabs || needle.length) for (const tab of tabs)
                        children.push({kind: "tab", host: host.id, epoch: host.epoch, session: session.id, window: tab.id,
                            label: tab.label, subtitle: session.label + " · " + hostName, activity: tab.activity || {}, selected: host.active && session.selected && tab.selected})
                }
            }
            if (hostMatch || children.length) {
                result.push({kind: "host", host: host.id, epoch: host.epoch, label: host.label, subtitle: [collision ? host.endpoint : "", host.connected ? "" : host.status].filter(Boolean).join(" · "),
                    selected: host.active, collapsed: host.collapsed, enabled: host.enabled})
                if (!host.collapsed || needle.length) result.push(...children)
            }
        }
        rows = result
        list.currentIndex = reset ? (rows.length ? 0 : -1) : rows.findIndex(row => key(row) === selected)
    }
    onInventoryChanged: rebuild(false)
    Component.onCompleted: rebuild(false)
    function selectedRow() { return rows[list.currentIndex] }
    function captured(row) { return {host: row.host, epoch: row.epoch} }
    function choose(row) {
        if (row.kind === "host") sidebar.frontend.hostAction("activate", {}, captured(row))
        else sidebar.frontend.hostAction("navigate", {session: row.session, window: row.window || ""}, captured(row))
        navigated()
    }
    padding: 8
    contentItem: ColumnLayout {
        spacing: 6
        TextField {
            id: search
            objectName: "sidebarSearch"
            Layout.fillWidth: true
            placeholderText: qsTr("Find sessions and tabs")
            onTextChanged: sidebar.rebuild(true)
            Keys.onDownPressed: { list.forceActiveFocus(); if (list.currentIndex < 0) list.currentIndex = 0 }
            onAccepted: if (sidebar.selectedRow()) sidebar.choose(sidebar.selectedRow())
        }
        ListView {
            id: list
            objectName: "hostInventory"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: sidebar.rows
            currentIndex: -1
            keyNavigationWraps: false
            ScrollBar.vertical: ScrollBar {}
            Keys.onReturnPressed: if (sidebar.selectedRow()) sidebar.choose(sidebar.selectedRow())
            Keys.onEnterPressed: if (sidebar.selectedRow()) sidebar.choose(sidebar.selectedRow())
            Keys.onLeftPressed: if (sidebar.selectedRow() && sidebar.selectedRow().kind === "host") sidebar.frontend.hostAction("collapse", {collapsed: true}, sidebar.captured(sidebar.selectedRow()))
            Keys.onRightPressed: if (sidebar.selectedRow() && sidebar.selectedRow().kind === "host") sidebar.frontend.hostAction("collapse", {collapsed: false}, sidebar.captured(sidebar.selectedRow()))
            delegate: ItemDelegate {
                id: row
                required property var modelData
                required property int index
                width: list.width
                highlighted: modelData.selected
                objectName: "sidebar-" + modelData.kind + "-" + (modelData.window || modelData.session || modelData.host)
                leftPadding: modelData.kind === "tab" ? 28 : modelData.kind === "session" ? 18 : 6
                background: Rectangle {
                    radius: 3
                    color: row.highlighted ? Qt.alpha(sidebar.palette.highlight, .18) : row.down ? Qt.rgba(1, 1, 1, .11) : row.hovered ? Qt.rgba(1, 1, 1, .07) : "transparent"
                    border.color: sidebar.palette.highlight
                    border.width: row.highlighted || row.activeFocus || (list.activeFocus && list.currentIndex === row.index) ? 1 : 0
                }
                contentItem: ColumnLayout {
                    spacing: 1
                    RowLayout {
                        property var presentation: row.modelData.activity || ({})
                        Repeater {
                            model: row.modelData.kind === "tab" ? (parent.presentation.icons || []).slice(0, sidebar.iconCount) : []
                            ProgramIcon { required property var modelData; required property int index; icon: modelData; colored: sidebar.colored(row.modelData.activity || {}, index) }
                        }
                        ProgramIcon { visible: row.modelData.kind === "session" && sidebar.iconCount > 0 && !!parent.presentation.icon; icon: parent.presentation.icon || null }
                        Label { Layout.fillWidth: true; color: sidebar.palette.windowText; text: (row.modelData.kind === "host" ? (row.modelData.collapsed ? "▸ " : "▾ ") : "") + row.modelData.label; textFormat: Text.PlainText; elide: Text.ElideRight; font.bold: row.modelData.kind === "host" }
                        Label { text: parent.presentation.muted ? "" : parent.presentation.word || ""; textFormat: Text.PlainText; color: sidebar.palette.windowText }
                    }
                    ProgressBar {
                        property var progress: (row.modelData.activity || {}).progress
                        visible: !!progress && !(row.modelData.activity || {}).muted
                        Layout.fillWidth: true
                        from: 0; to: 100
                        value: progress && progress.percent !== null ? progress.percent : 0
                        indeterminate: !!progress && progress.percent === null
                    }
                    Label { Layout.fillWidth: true; color: sidebar.palette.windowText; text: row.modelData.subtitle; textFormat: Text.PlainText; visible: text.length > 0; elide: Text.ElideMiddle; font.pixelSize: 11; opacity: .7 }
                }
                onClicked: { list.currentIndex = index; sidebar.choose(modelData) }
                TapHandler { acceptedButtons: Qt.RightButton; onTapped: { sidebar.menuTarget = Object.assign({}, row.modelData); hostMenu.popup() } }
                ToolTip.text: modelData.label + (modelData.subtitle ? " · " + modelData.subtitle : "")
                ToolTip.visible: hovered
            }
        }
        Button { objectName: "addHostButton"; text: qsTr("Add host…"); Layout.fillWidth: true; onClicked: sidebar.addHost() }
    }
    Menu {
        id: hostMenu
        MenuItem { text: qsTr("Edit host…"); onTriggered: sidebar.editHost(sidebar.menuTarget.host) }
        MenuItem { text: sidebar.menuTarget.enabled ? qsTr("Disconnect client") : qsTr("Connect client"); onTriggered: sidebar.frontend.hostAction(sidebar.menuTarget.enabled ? "disconnect" : "connect", {}, sidebar.captured(sidebar.menuTarget)) }
        MenuItem { text: sidebar.menuTarget.collapsed ? qsTr("Expand host") : qsTr("Collapse host"); onTriggered: sidebar.frontend.hostAction("collapse", {collapsed: !sidebar.menuTarget.collapsed}, sidebar.captured(sidebar.menuTarget)) }
        MenuSeparator {}
        MenuItem { text: qsTr("Remove saved host…"); enabled: (sidebar.inventory.rows || []).length > 1; onTriggered: sidebar.removeHost(sidebar.menuTarget.host) }
    }
}
