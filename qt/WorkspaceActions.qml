import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: workspace
    required property var frontend
    property real canvasWidth: width
    property real canvasHeight: height
    property bool modalOpen: question.visible || form.visible || menu.visible
    property var pending: frontend.state.workspace || null
    property var menuTarget: ({})
    property var formTarget: ({})
    property string formAction: ""
    property string formKind: ""
    property var destinations: []
    property var directions: []
    property var menuPane: (frontend.state.panes || []).find(pane => pane.id === menuTarget.pane) || ({})
    property bool menuTerminalLive: Boolean(frontend.state.connected && menuPane.terminal && menuPane.terminal.connected)
    signal finished()
    signal terminalAction(string action, var captured)

    function capture(pane) {
        const state = frontend.state
        return {host: state.host, epoch: state.epoch, session: state.session, window: state.window,
                pane: pane || state.pane, width: canvasWidth, height: canvasHeight}
    }
    function send(action, args, captured) { frontend.workspaceAction(action, args || {}, captured || capture()) }
    function run(action, args, pane, captured) {
        const target = captured || capture(pane)
        args = args || {}
        if (action === "session.new" || action === "session.rename" || action === "client.tab.rename" || action === "pane.rename") {
            if (args.label !== undefined) { send(action, args, target); return }
            formAction = action
            formTarget = target
            formKind = "name"
            const state = frontend.state
            let rows = state.panes || []
            let selected = target.pane
            switch (action) {
            case "session.new": form.title = qsTr("New session"); break
            case "session.rename":
                form.title = qsTr("Rename session")
                rows = state.sessions
                selected = target.session
                break
            case "client.tab.rename":
                form.title = qsTr("Rename tab")
                rows = state.windows
                selected = target.window
                break
            default: form.title = qsTr("Rename pane")
            }
            const row = rows.find(row => row.id === selected)
            let name = ""
            if (action !== "session.new" && row)
                name = row.user_label !== undefined ? row.user_label : row.label || ""
            nameField.text = name
            form.open()
            return
        }
        if (action === "pane.move_to_tab" && args.window_id === undefined) {
            formAction = action
            formTarget = target
            formKind = "move"
            destinations = (frontend.state.windows || []).filter(row => row.id !== target.window)
            destination.currentIndex = destinations.length ? 0 : -1
            form.title = qsTr("Move pane to tab")
            form.open()
            return
        }
        if (["pane.split", "pane.focus", "pane.move", "pane.resize", "client.tab.move"].includes(action) && args.direction === undefined) {
            formAction = action
            formTarget = target
            formKind = "direction"
            directions = action === "client.tab.move" ? ["left", "right"] : action === "pane.split" ? ["right", "left", "down", "up", "best_fit"] : ["left", "right", "up", "down"]
            directionPicker.currentIndex = 0
            switch (action) {
            case "pane.split": form.title = qsTr("Split pane"); break
            case "pane.focus": form.title = qsTr("Focus pane"); break
            case "pane.resize": form.title = qsTr("Resize pane"); break
            case "client.tab.move": form.title = qsTr("Move tab"); break
            default: form.title = qsTr("Move pane")
            }
            form.open()
            return
        }
        send(action, args, target)
    }
    function openMenu(anchor, pane) {
        menuTarget = capture(pane)
        menu.parent = anchor
        menu.x = 0
        menu.y = anchor.height
        menu.open()
    }
    function openPaneMenu(x, y, pane) {
        menuTarget = capture(pane)
        menu.parent = workspace
        menu.x = Math.max(0, Math.min(x, workspace.width - menu.width))
        menu.y = Math.max(0, Math.min(y, workspace.height - menu.height))
        menu.open()
    }
    function menuAction(action, args) { run(action, args || {}, "", menuTarget) }
    onPendingChanged: {
        if (pending && (!question.question || question.question.id !== pending.id)) {
            question.question = Object.assign({}, pending, {host: frontend.state.host})
            question.open()
        } else if (!pending && question.visible) question.close()
    }
    ConfirmDialog {
        id: question
        parent: Overlay.overlay
        onConfirmed: captured => workspace.send("confirm.accept", {id: captured.id}, captured)
        onCancelled: captured => workspace.send("confirm.cancel", {id: captured.id}, captured)
        onClosed: workspace.finished()
    }
    Dialog {
        id: form
        objectName: "workspaceForm"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(440, parent.width - 32)
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        contentItem: ColumnLayout {
            Label {
                Layout.fillWidth: true
                text: workspace.formAction === "session.new" ? qsTr("Create a session on this host. Leave the name empty for the server's default.") : workspace.formKind === "move" ? qsTr("The existing program moves to another tab in this session.") : workspace.formKind === "direction" ? qsTr("Choose a direction in the server's current layout.") : qsTr("Leave the name empty to use the default title.")
                wrapMode: Text.Wrap
            }
            TextField {
                id: nameField
                objectName: "workspaceName"
                visible: workspace.formKind === "name"
                Layout.fillWidth: true
                Accessible.name: qsTr("Name")
                onAccepted: form.accept()
            }
            ComboBox {
                id: destination
                objectName: "workspaceDestination"
                visible: workspace.formKind === "move"
                Layout.fillWidth: true
                model: workspace.destinations
                textRole: "label"
                Accessible.name: qsTr("Destination tab")
            }
            ComboBox {
                id: directionPicker
                objectName: "workspaceDirection"
                visible: workspace.formKind === "direction"
                Layout.fillWidth: true
                model: workspace.directions.map(direction => ({value: direction, label: direction === "best_fit" ? qsTr("Best fit") : direction.charAt(0).toUpperCase() + direction.slice(1)}))
                textRole: "label"
                Accessible.name: qsTr("Direction")
            }
            RowLayout {
                visible: workspace.formKind === "direction" && workspace.formAction === "pane.resize"
                Label { text: qsTr("Amount (%)") }
                SpinBox { id: resizeAmount; objectName: "workspaceResizeAmount"; from: 1; to: 100; value: 5; editable: true; Layout.fillWidth: true }
            }
            Label { visible: workspace.formKind === "move" && !workspace.destinations.length; text: qsTr("Open another tab first."); Layout.fillWidth: true; wrapMode: Text.Wrap }
        }
        footer: DialogButtonBox {
            Button { text: qsTr("Cancel"); objectName: "workspaceFormCancel"; DialogButtonBox.buttonRole: DialogButtonBox.RejectRole }
            Button {
                text: workspace.formAction === "session.new" ? qsTr("Create") : workspace.formKind === "move" ? qsTr("Move") : qsTr("Save")
                objectName: "workspaceFormApply"
                enabled: workspace.formKind !== "move" || destination.currentIndex >= 0
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
            }
            Button {
                visible: workspace.formKind === "name" && workspace.formAction !== "session.new"
                objectName: "workspaceUseDefault"
                text: qsTr("Use default title")
                onClicked: { nameField.text = ""; form.accept() }
            }
        }
        onOpened: { if (workspace.formKind === "name") { nameField.forceActiveFocus(); nameField.selectAll() } }
        onAccepted: {
            const args = workspace.formKind === "move" ? {window_id: workspace.destinations[destination.currentIndex].id} : workspace.formKind === "direction" ? {direction: workspace.directions[directionPicker.currentIndex]} : {label: nameField.text}
            if (workspace.formAction === "pane.resize") args.amount = resizeAmount.value
            workspace.send(workspace.formAction, args, workspace.formTarget)
        }
        onClosed: workspace.finished()
    }
    Menu {
        id: menu
        objectName: "workspaceMenu"
        MenuItem { text: qsTr("Copy"); enabled: Boolean(workspace.menuPane.frame && workspace.menuPane.frame.selection_text); onTriggered: workspace.terminalAction("pane.copy", workspace.menuTarget) }
        MenuItem { text: qsTr("Paste"); enabled: workspace.menuTerminalLive; onTriggered: workspace.terminalAction("pane.paste", workspace.menuTarget) }
        MenuItem { text: qsTr("Select all"); enabled: Boolean(workspace.menuPane.frame); onTriggered: workspace.terminalAction("pane.select_all", workspace.menuTarget) }
        MenuSeparator {}
        Menu {
            title: qsTr("Session")
            MenuItem { text: qsTr("New session…"); objectName: "workspaceNewSession"; onTriggered: workspace.menuAction("session.new") }
            MenuItem { text: qsTr("Rename session…"); onTriggered: workspace.menuAction("session.rename") }
            MenuSeparator {}
            MenuItem { text: qsTr("Close session…"); onTriggered: workspace.menuAction("session.close") }
        }
        Menu {
            title: qsTr("Tab")
            MenuItem { text: qsTr("New tab"); objectName: "workspaceNewTab"; onTriggered: workspace.menuAction("client.tab.new") }
            MenuItem { text: qsTr("Rename tab…"); onTriggered: workspace.menuAction("client.tab.rename") }
            MenuItem { text: qsTr("Mute or unmute tab"); onTriggered: workspace.menuAction("client.tab.mute") }
            MenuItem { text: qsTr("Move tab left"); onTriggered: workspace.menuAction("client.tab.move", {direction: "left"}) }
            MenuItem { text: qsTr("Move tab right"); onTriggered: workspace.menuAction("client.tab.move", {direction: "right"}) }
            MenuSeparator {}
            MenuItem { text: qsTr("Close other tabs…"); onTriggered: workspace.menuAction("client.tab.close_others") }
            MenuItem { text: qsTr("Close tab…"); onTriggered: workspace.menuAction("client.tab.close") }
        }
        MenuSeparator {}
        MenuItem { text: qsTr("Split right"); objectName: "workspaceSplitRight"; onTriggered: workspace.menuAction("pane.split", {direction: "right"}) }
        MenuItem { text: qsTr("Split left"); onTriggered: workspace.menuAction("pane.split", {direction: "left"}) }
        MenuItem { text: qsTr("Split down"); objectName: "workspaceSplitDown"; onTriggered: workspace.menuAction("pane.split", {direction: "down"}) }
        MenuItem { text: qsTr("Split up"); onTriggered: workspace.menuAction("pane.split", {direction: "up"}) }
        MenuItem { text: qsTr("Split best fit"); onTriggered: workspace.menuAction("pane.split", {direction: "best_fit"}) }
        MenuItem { text: qsTr("Zoom or restore pane"); onTriggered: workspace.menuAction("pane.zoom") }
        MenuItem { text: qsTr("Balance splits"); onTriggered: workspace.menuAction("pane.balance") }
        MenuItem { text: qsTr("Move pane to tab…"); onTriggered: workspace.menuAction("pane.move_to_tab") }
        MenuItem { text: qsTr("Rename pane…"); onTriggered: workspace.menuAction("pane.rename") }
        MenuItem { text: qsTr("Copy pane ID"); onTriggered: workspace.terminalAction("pane.copy_id", workspace.menuTarget) }
        MenuSeparator {}
        MenuItem { text: qsTr("Clear terminal"); enabled: workspace.menuTerminalLive; onTriggered: workspace.terminalAction("pane.clear", workspace.menuTarget) }
        MenuItem { text: qsTr("Reset terminal"); enabled: workspace.menuTerminalLive; onTriggered: workspace.terminalAction("pane.reset", workspace.menuTarget) }
        MenuItem { text: qsTr("Reconnect terminal"); onTriggered: workspace.terminalAction("pane.reconnect", workspace.menuTarget) }
        MenuSeparator {}
        MenuItem { text: qsTr("Close other panes…"); onTriggered: workspace.menuAction("pane.close_others") }
        MenuItem { text: qsTr("Close pane…"); objectName: "workspaceClosePane"; onTriggered: workspace.menuAction("pane.close") }
        onClosed: workspace.finished()
    }
}
