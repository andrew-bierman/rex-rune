import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import RexQt 1.0

ApplicationWindow {
    id: root
    objectName: "rexWindow"
    width: 1024
    height: 700
    minimumWidth: 640
    minimumHeight: 480
    visible: true
    title: "Rex Qt Quick"
    color: "#001528"
    palette.window: "#001528"
    palette.windowText: "#e8eef6"
    palette.base: "#0d1524"
    palette.text: "#e8eef6"
    palette.placeholderText: "#93a3b8"
    palette.button: "#142337"
    palette.buttonText: "#e8eef6"
    palette.highlight: "#7cc4ff"
    palette.highlightedText: "#001528"
    onActiveChanged: backend.setVisible(active && visible)
    onVisibleChanged: backend.setVisible(active && visible)
    Component.onCompleted: backend.setVisible(active && visible)
    property var state: backend.state
    property var dividerIds: []
    onStateChanged: {
        const ids = (state.dividers || []).map(split => split.before + "/" + split.after)
        if (JSON.stringify(ids) !== JSON.stringify(dividerIds)) dividerIds = ids
    }
    function selectedIndex(items, id) {
        for (let i = 0; i < items.length; i++) if (items[i].id === id) return i
        return -1
    }
    function selectedTerminal(pane) {
        const i = backend.paneIds.indexOf(pane || root.state.pane)
        const card = paneRepeater.itemAt(i)
        return card ? card.terminal : null
    }
    function find() { findBar.terminal = selectedTerminal(); findBar.open() }
    FindBar { id: findBar; anchors.top: parent.top; anchors.right: parent.right; z: 100; onClosed: root.restoreTerminal() }
    function restoreTerminal() {
        if (commandPopup.visible || shortcutPopup.visible || findBar.visible || workspaceUI.modalOpen) return
        const term = selectedTerminal(); if (term) term.forceActiveFocus()
    }
    function action(id, args, origin) {
        const term = selectedTerminal()
        switch (id) {
        case "client.palette.toggle": commandPopup.visible ? commandPopup.close() : commandPopup.open(); break
        case "client.settings": shortcutPopup.open(); break
        case "session.refresh": backend.refresh(); break
        case "session.switch": sessionPicker.popup.open(); break
        case "pane.copy": if (term) term.copy(); break
        case "pane.paste": if (term) term.paste(); break
        case "pane.select_all": if (term) term.selectAll(); break
        case "pane.copy_id": if (term) term.copyId(); break
        case "pane.clear": if (term) term.clear(); break
        case "pane.reset": if (term) term.reset(); break
        case "client.search": find(); break
        case "client.find.next": if (term) term.searchStep(false); break
        case "client.find.previous": if (term) term.searchStep(true); break
        case "client.prompt.previous": if (term) term.promptStep(true); break
        case "client.prompt.next": if (term) term.promptStep(false); break
        case "client.scroll": if (term) term.scroll(args.lines); break
        case "pane.reconnect": if (term) term.reconnect(); break
        default:
            const spec = ((root.state.keymap || {}).actions || []).find(row => row.id === id)
            if (spec && spec.handler === "workspace") workspaceUI.run(id, args)
            else backend.performAction(id, args, origin)
        }
    }
    Commands { id: commandPopup; objectName: "commandsPopup"; parent: Overlay.overlay; actions: workspaceActions; onFinished: root.restoreTerminal() }
    ShortcutSettings { id: shortcutPopup; objectName: "shortcutPopup"; parent: Overlay.overlay; actions: workspaceActions; frontend: backend; onFinished: root.restoreTerminal() }
    WorkspaceActions {
        id: workspaceUI
        objectName: "workspaceUI"
        anchors.fill: parent
        z: 200
        frontend: backend
        canvasWidth: canvas.width
        canvasHeight: canvas.height
        onFinished: Qt.callLater(root.restoreTerminal)
        onTerminalAction: (id, captured) => {
            if (captured.host !== root.state.host || captured.epoch !== root.state.epoch || captured.session !== root.state.session || captured.window !== root.state.window) return
            const term = selectedTerminal(captured.pane)
            if (!term) return
            switch (id) {
            case "pane.copy": term.copy(); break
            case "pane.paste": term.paste(); break
            case "pane.select_all": term.selectAll(); break
            case "pane.copy_id": term.copyId(); break
            case "pane.clear": term.clear(); break
            case "pane.reset": term.reset(); break
            case "pane.reconnect": term.reconnect(); break
            }
        }
    }
    Binding { target: workspaceActions; property: "searching"; value: findBar.visible }
    Binding { target: workspaceActions; property: "modalOpen"; value: commandPopup.visible || shortcutPopup.visible || findBar.visible || workspaceUI.modalOpen }
    Connections { target: workspaceActions; function onRequested(id, args, origin) { root.action(id, args, origin) } }
    property string selectedPane: state.pane || ""
    onSelectedPaneChanged: { if (findBar.visible) findBar.close(); Qt.callLater(restoreTerminal) }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        ToolBar {
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            RowLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                ComboBox {
                    id: sessionPicker
                    objectName: "sessions"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 180
                    Layout.maximumWidth: 240
                    Layout.minimumWidth: 110
                    model: root.state.sessions || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.session)
                    Accessible.name: "Session"
                    ToolTip.text: "Session"
                    ToolTip.visible: hovered
                    onActivated: { backend.selectSession(model[index].id); root.restoreTerminal() }
                }
                ComboBox {
                    objectName: "tabs"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 200
                    Layout.minimumWidth: 110
                    model: root.state.windows || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.window)
                    Accessible.name: "Tab"
                    ToolTip.text: "Tab"
                    ToolTip.visible: hovered
                    onActivated: { backend.selectWindow(model[index].id); root.restoreTerminal() }
                }
                ComboBox {
                    objectName: "panes"
                    visible: root.width >= 900
                    Layout.fillWidth: true
                    Layout.preferredWidth: 150
                    Layout.maximumWidth: 200
                    Layout.minimumWidth: 90
                    model: root.state.panes || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.pane)
                    Accessible.name: "Pane"
                    ToolTip.text: "Pane"
                    ToolTip.visible: hovered
                    onActivated: { backend.selectPane(model[index].id); root.restoreTerminal() }
                }
                ToolButton {
                    objectName: "copyButton"
                    text: "Copy"
                    Accessible.name: "Copy selection"
                    ToolTip.text: "Copy" + (workspaceActions.hint("pane.copy") ? " · " + workspaceActions.hint("pane.copy") : "")
                    ToolTip.visible: hovered
                    onClicked: { workspaceActions.run("pane.copy"); root.restoreTerminal() }
                }
                ToolButton {
                    objectName: "pasteButton"
                    text: "Paste"
                    onClicked: workspaceActions.run("pane.paste")
                }
                Label {
                    visible: workspaceActions.pending.length > 0 || workspaceActions.activeModes.length > 0
                    text: workspaceActions.pending || workspaceActions.activeModes.join(" · ")
                    Layout.maximumWidth: 150
                    elide: Text.ElideRight
                }
                ToolButton {
                    objectName: "commandsButton"
                    text: "⌘"
                    Accessible.name: "Commands"
                    ToolTip.text: "Commands" + (workspaceActions.hint("client.palette.toggle") ? " · " + workspaceActions.hint("client.palette.toggle") : "")
                    ToolTip.visible: hovered
                    onClicked: workspaceActions.run("client.palette.toggle")
                }
                ToolButton {
                    objectName: "settingsButton"
                    text: "⚙"
                    Accessible.name: "Settings"
                    onClicked: workspaceActions.run("client.settings")
                }
                ToolButton {
                    objectName: "workspaceButton"
                    text: "+"
                    Accessible.name: qsTr("Session, tab and pane actions")
                    onClicked: workspaceUI.openMenu(this)
                }
                ToolButton {
                    id: more
                    objectName: "moreButton"
                    text: "⋯"
                    Accessible.name: "Terminal actions"
                    onClicked: actions.open()
                    Menu {
                        id: actions
                        objectName: "actionsMenu"
                        y: more.height
                        MenuItem {
                            objectName: "selectAllButton"
                            text: "Select all terminal text"
                            onTriggered: { workspaceActions.run("pane.select_all"); root.restoreTerminal() }
                        }
                        MenuItem { objectName: "previousPromptButton"; text: "Previous prompt"; onTriggered: { workspaceActions.run("client.prompt.previous"); root.restoreTerminal() } }
                        MenuItem { objectName: "nextPromptButton"; text: "Next prompt"; onTriggered: { workspaceActions.run("client.prompt.next"); root.restoreTerminal() } }
                        MenuItem { text: "Refresh sessions"; onTriggered: workspaceActions.run("session.refresh") }
                    }
                }
            }
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: warning.visible ? warning.implicitHeight + 16 : 0
            color: "#332b1d"
            visible: implicitHeight > 0
            RowLayout {
                id: warning
                anchors.fill: parent
                anchors.margins: 8
                visible: backend.problem.length > 0 || !root.state.connected || !(root.state.terminal && root.state.terminal.connected)
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: "#f0c987"
                    text: backend.problem || (!root.state.connected ? (root.state.status || "Connecting…") : (root.state.pane ? "Terminal input paused" : "Choose a terminal pane"))
                }
                ToolButton { text: backend.problem ? "Dismiss" : "Refresh"; onClicked: backend.problem ? backend.dismissProblem() : backend.refresh() }
            }
        }
        Item {
            id: canvas
            objectName: "terminalCanvas"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 8
            clip: true
            Label {
                anchors.centerIn: parent
                visible: backend.paneIds.length === 0
                text: root.state.connected ? "Choose a terminal session" : "Connecting…"
                color: "#a2b0c4"
            }
            Repeater {
                model: root.dividerIds
                delegate: Item {
                    id: dividerHandle
                    required property string modelData
                    property var split: (root.state.dividers || []).find(row => row.before + "/" + row.after === modelData)
                    property var rect: split ? split.rect : [0, 0, 0, 0]
                    x: Math.round((rect[0] + (split && split.horizontal ? rect[2] * split.ratio : 0)) * canvas.width) - (split && split.horizontal ? 4 : 0)
                    y: Math.round((rect[1] + (split && !split.horizontal ? rect[3] * split.ratio : 0)) * canvas.height) - (split && !split.horizontal ? 4 : 0)
                    width: split && split.horizontal ? 8 : rect[2] * canvas.width
                    height: split && !split.horizontal ? 8 : rect[3] * canvas.height
                    z: 1000
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: dividerHandle.split && dividerHandle.split.horizontal ? Qt.SplitHCursor : Qt.SplitVCursor
                        property var captured: null
                        property var initial: null
                        onPressed: { captured = workspaceUI.capture(); initial = dividerHandle.split }
                        onPositionChanged: mouse => {
                            if (!pressed || !initial) return
                            const point = mapToItem(canvas, mouse.x, mouse.y)
                            const r = initial.rect
                            const ratio = initial.horizontal ? (point.x / canvas.width - r[0]) / r[2] : (point.y / canvas.height - r[1]) / r[3]
                            workspaceUI.send("pane.resize_divider", {before: initial.before, after: initial.after, ratio: ratio}, captured)
                        }
                        onReleased: { captured = null; initial = null }
                        onCanceled: { captured = null; initial = null }
                    }
                }
            }
            Repeater {
                id: paneRepeater
                model: backend.paneIds
                delegate: Rectangle {
                    id: card
                    required property string modelData
                    required property int index
                    property alias terminal: terminalView
                    property var pane: terminalView.paneState
                    property var placement: pane.rect || [0, 0, 1, 1]
                    x: Math.round(placement[0] * canvas.width) + 2
                    y: Math.round(placement[1] * canvas.height) + 2
                    width: Math.max(0, Math.round(placement[2] * canvas.width) - 4)
                    height: Math.max(0, Math.round(placement[3] * canvas.height) - 4)
                    z: index + 1
                    color: "#001528"
                    border.width: backend.paneIds.length > 1 ? 1 : 0
                    border.color: root.selectedPane === modelData ? "#7cc4ff" : "#26374b"
                    clip: true
                    PaneHeader {
                        id: heading
                        width: parent.width
                        height: backend.paneIds.length > 1 || root.state.zoomed ? implicitHeight : 0
                        visible: height > 0
                        pane: card.pane
                        workspace: workspaceUI
                        focused: root.selectedPane === card.modelData
                    }
                    TerminalView {
                        id: terminalView
                        objectName: "terminal"
                        x: 4
                        y: heading.height
                        width: Math.max(0, parent.width - 8)
                        height: Math.max(0, parent.height - y)
                        frontend: backend
                        actions: workspaceActions
                        paneId: card.modelData
                        onContextMenuRequested: (x, y) => {
                            const point = terminalView.mapToItem(workspaceUI, x, y)
                            workspaceUI.openPaneMenu(point.x, point.y, card.modelData)
                        }
                        Accessible.name: "Terminal " + (card.pane.label || "")
                        Component.onCompleted: if (root.selectedPane === card.modelData) root.restoreTerminal()
                    }
                }
            }
        }
    }
}
