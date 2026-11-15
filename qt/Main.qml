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
    function selectedIndex(items, id) {
        for (let i = 0; i < items.length; i++) if (items[i].id === id) return i
        return -1
    }
    function selectedTerminal() {
        const i = backend.paneIds.indexOf(root.state.pane)
        const card = paneRepeater.itemAt(i)
        return card ? card.terminal : null
    }
    function find() { findBar.terminal = selectedTerminal(); findBar.open() }
    FindBar { id: findBar; anchors.top: parent.top; anchors.right: parent.right; z: 100; onClosed: root.restoreTerminal() }
    function restoreTerminal() {
        if (commandPopup.visible || shortcutPopup.visible || findBar.visible) return
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
        case "client.search": find(); break
        case "client.find.next": if (term) term.searchStep(false); break
        case "client.find.previous": if (term) term.searchStep(true); break
        case "client.prompt.previous": if (term) term.promptStep(true); break
        case "client.prompt.next": if (term) term.promptStep(false); break
        case "client.scroll": if (term) term.scroll(args.lines); break
        case "pane.reconnect": if (term) term.reconnect(); break
        default: backend.performAction(id, args, origin)
        }
    }
    Commands { id: commandPopup; objectName: "commandsPopup"; parent: Overlay.overlay; actions: workspaceActions; onFinished: root.restoreTerminal() }
    ShortcutSettings { id: shortcutPopup; objectName: "shortcutPopup"; parent: Overlay.overlay; actions: workspaceActions; frontend: backend; onFinished: root.restoreTerminal() }
    Binding { target: workspaceActions; property: "searching"; value: findBar.visible }
    Binding { target: workspaceActions; property: "modalOpen"; value: commandPopup.visible || shortcutPopup.visible || findBar.visible }
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
                    Label {
                        id: heading
                        x: 8
                        y: 3
                        width: parent.width - 16
                        height: backend.paneIds.length > 1 ? 22 : 0
                        visible: height > 0
                        text: card.pane.label || "Terminal"
                        elide: Text.ElideRight
                        color: root.selectedPane === card.modelData ? "#e8eef6" : "#a2b0c4"
                    }
                    TerminalView {
                        id: terminalView
                        objectName: "terminal"
                        x: 4
                        y: heading.visible ? 26 : 0
                        width: Math.max(0, parent.width - 8)
                        height: Math.max(0, parent.height - y)
                        frontend: backend
                        actions: workspaceActions
                        paneId: card.modelData
                        Accessible.name: "Terminal " + (card.pane.label || "")
                        Component.onCompleted: if (root.selectedPane === card.modelData) root.restoreTerminal()
                    }
                }
            }
        }
    }
}
