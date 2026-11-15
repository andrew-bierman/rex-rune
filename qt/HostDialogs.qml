pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: dialogs
    required property var frontend
    property bool modalOpen: form.visible || removal.visible
    property var target: ({})
    property bool adding: false
    property var selected: ({})
    signal finished()
    function cancelCaptured() { form.close(); removal.close() }
    function openAdd() {
        dialogs.frontend.dismissHostFormProblem()
        adding = true
        target = {host: dialogs.frontend.state.host, epoch: dialogs.frontend.state.epoch}
        name.text = ""; endpoint.text = ""; shell.text = ""
        form.open()
    }
    function openEdit(id) {
        dialogs.frontend.dismissHostFormProblem()
        const row = (((dialogs.frontend.hosts || {}).rows) || []).find(host => host.id === id)
        if (!row) return
        adding = false
        target = {host: row.id, epoch: row.epoch}
        name.text = row.label; endpoint.text = row.endpoint; shell.text = row.shell || ""
        form.open()
    }
    function openRemove(id) {
        const row = (((dialogs.frontend.hosts || {}).rows) || []).find(host => host.id === id)
        if (!row) return
        selected = row
        target = {host: row.id, epoch: row.epoch}
        removal.open()
    }
    Dialog {
        id: form
        objectName: "hostForm"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(600, parent.width - 32)
        height: Math.min(680, parent.height - 32)
        modal: true
        focus: true
        title: dialogs.adding ? qsTr("Add host") : qsTr("Edit host")
        closePolicy: Popup.CloseOnEscape
        onOpened: name.forceActiveFocus()
        onClosed: { dialogs.frontend.hostAction("discover_cancel", {}, dialogs.target); dialogs.frontend.dismissHostFormProblem(); dialogs.finished() }
        contentItem: ScrollView {
            id: fields
            clip: true
            ColumnLayout {
            width: fields.availableWidth
            spacing: 8
            Label { Layout.fillWidth: true; text: qsTr("Attach to an existing Rex server. Its sessions and programs stay on that host."); wrapMode: Text.Wrap }
            Label { text: qsTr("Name") }
            TextField { id: name; objectName: "hostName"; Layout.fillWidth: true; Accessible.name: qsTr("Host name"); placeholderText: qsTr("A name for this host") }
            Label { text: qsTr("Endpoint") }
            TextField { id: endpoint; objectName: "hostEndpoint"; Layout.fillWidth: true; Accessible.name: qsTr("Rex endpoint"); placeholderText: "unix:///…  https://…  ssh://…"; selectByMouse: true }
            Label { text: qsTr("Shell for new terminals (optional)") }
            TextField { id: shell; objectName: "hostShell"; Layout.fillWidth: true; Accessible.name: qsTr("Shell for new terminals"); placeholderText: qsTr("Use the server's login shell") }
            RowLayout {
                Button { objectName: "discoverHosts"; text: qsTr("Find on tailnet"); enabled: !((dialogs.frontend.hosts || {}).discovery || {}).reading; onClicked: dialogs.frontend.hostAction("discover", {}, dialogs.target) }
                BusyIndicator { running: Boolean(((dialogs.frontend.hosts || {}).discovery || {}).reading); visible: running; Layout.preferredWidth: 24; Layout.preferredHeight: 24 }
            }
            Label { Layout.fillWidth: true; visible: text.length > 0; wrapMode: Text.Wrap; text: ((dialogs.frontend.hosts || {}).discovery || {}).problem || "" }
            ListView {
                id: discovered
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(180, contentHeight)
                visible: count > 0
                clip: true
                model: (((dialogs.frontend.hosts || {}).discovery || {}).rows) || []
                ScrollBar.vertical: ScrollBar {}
                delegate: ItemDelegate {
                    id: machine
                    required property var modelData
                    width: discovered.width
                    contentItem: ColumnLayout {
                        Label { Layout.fillWidth: true; text: machine.modelData.label + " · " + machine.modelData.os; elide: Text.ElideRight }
                        Label { Layout.fillWidth: true; text: machine.modelData.endpoint || machine.modelData.problem || ""; elide: Text.ElideMiddle; opacity: .7 }
                        RowLayout {
                            Button { text: qsTr("Use direct"); visible: Boolean(machine.modelData.endpoint); onClicked: { name.text = machine.modelData.label; endpoint.text = machine.modelData.endpoint } }
                            Button { text: qsTr("Use SSH"); enabled: machine.modelData.online; onClicked: { name.text = machine.modelData.label; endpoint.text = machine.modelData.ssh_endpoint } }
                        }
                    }
                    onClicked: if (modelData.endpoint) { name.text = modelData.label; endpoint.text = modelData.endpoint }
                }
            }
            Label { Layout.fillWidth: true; visible: dialogs.frontend.hostFormProblem.length > 0; text: dialogs.frontend.hostFormProblem; wrapMode: Text.Wrap }
            }
        }
        footer: DialogButtonBox {
            Button { objectName: "cancelHost"; text: qsTr("Cancel"); DialogButtonBox.buttonRole: DialogButtonBox.RejectRole; onClicked: form.close() }
            Button {
                objectName: "saveHost"
                text: qsTr("Save")
                enabled: name.text.trim().length > 0 && endpoint.text.trim().length > 0
                DialogButtonBox.buttonRole: DialogButtonBox.ActionRole
                onClicked: {
                    dialogs.frontend.hostAction(dialogs.adding ? "add" : "edit", {label: name.text, endpoint: endpoint.text, shell: shell.text}, dialogs.target)
                }
            }
        }
        Connections {
            target: dialogs.frontend
            function onHostSaved() { if (form.visible) form.close() }
        }
    }
    Dialog {
        id: removal
        objectName: "removeHostConfirmation"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(440, parent.width - 32)
        modal: true
        focus: true
        title: qsTr("Remove saved host?")
        closePolicy: Popup.CloseOnEscape
        onOpened: keep.forceActiveFocus()
        onClosed: dialogs.finished()
        contentItem: Label { text: qsTr("Remove “%1” from this client's saved hosts? Its sessions and programs keep running. No server is stopped.").arg(dialogs.selected.label || ""); wrapMode: Text.Wrap }
        footer: DialogButtonBox {
            Button { id: keep; objectName: "keepHost"; text: qsTr("Keep host"); DialogButtonBox.buttonRole: DialogButtonBox.RejectRole; onClicked: removal.close() }
            Button { objectName: "removeHost"; text: qsTr("Remove saved host"); DialogButtonBox.buttonRole: DialogButtonBox.DestructiveRole; onClicked: { dialogs.frontend.hostAction("remove", {}, dialogs.target); removal.close() } }
        }
    }
}
