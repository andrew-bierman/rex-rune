import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
ColumnLayout {
    required property var frontend
    property string query: ""
    visible: !query || "remote hosts control navigation consent".includes(query.toLowerCase())
    Label { text: qsTr("Remote control"); font.bold: true }
    Label {
        Layout.fillWidth: true; wrapMode: Text.Wrap; textFormat: Text.PlainText
        text: qsTr("Allow other clients on a saved host to navigate this window. Input, clipboard, creating or closing terminals, files and dialogs stay local. Control pauses while you type or a panel is open.")
    }
    Label { text: qsTr("Changes are saved immediately for each host."); wrapMode: Text.Wrap; Layout.fillWidth: true }
    Repeater {
        model: frontend.hosts.rows || []
        delegate: Switch {
            required property var modelData
            objectName: "remoteConsent-" + modelData.id
            text: modelData.label
            checked: Boolean(modelData.remote_control)
            onToggled: frontend.hostAction("remote_control", {enabled: checked}, {host: modelData.id, epoch: modelData.epoch})
        }
    }
}
