import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Dialog {
    id: root
    required property var confirmation
    signal confirmRequested(double requestId)
    signal cancelRequested()
    modal: true
    focus: true
    title: confirmation.kind === "setup" ? qsTr("Start personal server?") : qsTr("Stop server?")
    width: Math.min(580, parent.width - 32)
    anchors.centerIn: parent
    closePolicy: Popup.CloseOnEscape
    onOpened: keep.forceActiveFocus()
    onRejected: cancelRequested()
    onClosed: cancelRequested()
    contentItem: ColumnLayout {
        Label { textFormat: Text.PlainText; text: root.confirmation.target || ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { textFormat: Text.PlainText; text: root.confirmation.kind === "setup" ? qsTr("Run this command once. This does not create a session. A failed attempt is never retried automatically.") : qsTr("Stop this server and disconnect its clients. This attempt is never retried automatically."); wrapMode: Text.Wrap; Layout.fillWidth: true }
        TextArea { text: root.confirmation.command || ""; readOnly: true; wrapMode: TextEdit.Wrap; Layout.fillWidth: true }
        RowLayout {
            Item { Layout.fillWidth: true }
            Button { id: keep; Keys.onReturnPressed: root.reject(); Keys.onEnterPressed: root.reject(); objectName: "keepServerButton"; text: qsTr("Keep"); focus: true; onClicked: root.reject() }
            Button { objectName: "confirmServerButton"; text: root.confirmation.kind === "setup" ? qsTr("Start once") : qsTr("Stop server"); onClicked: { root.confirmRequested(root.confirmation.request_id); root.close() } }
        }
    }
}
