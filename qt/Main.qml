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
    palette.button: "#142337"
    palette.buttonText: "#e8eef6"
    palette.highlight: "#7cc4ff"
    palette.highlightedText: "#001528"
    property var state: backend.state
    function selectedIndex(items, id) {
        for (let i = 0; i < items.length; i++) if (items[i].id === id) return i
        return -1
    }
    function restoreTerminal() { terminal.forceActiveFocus() }

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
                    onClicked: { terminal.copy(); root.restoreTerminal() }
                }
                ToolButton {
                    objectName: "pasteButton"
                    text: "Paste"
                    onClicked: terminal.paste()
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
                            text: "Select visible text"
                            onTriggered: { terminal.selectAll(); root.restoreTerminal() }
                        }
                        MenuItem { text: "Refresh sessions"; onTriggered: backend.refresh() }
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
        TerminalView {
            id: terminal
            objectName: "terminal"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 8
            frontend: backend
            focus: true
            Accessible.name: "Terminal"
            Component.onCompleted: forceActiveFocus()
        }
    }
}
