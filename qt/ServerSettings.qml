import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
ColumnLayout {
    id: root
    required property var serverState
    signal readRequested()
    signal prepareRequested(string kind)
    signal connectRequested()
    signal editEndpointRequested(string endpoint)
    signal pairRequested()
    signal copyRequested(string text)
    spacing: 12
    Label { textFormat: Text.PlainText; text: qsTr("Server"); font.bold: true }
    Label { textFormat: Text.PlainText; text: root.serverState.status || qsTr("Waiting for report"); wrapMode: Text.Wrap; Layout.fillWidth: true }
    RowLayout {
        Button { text: qsTr("Read server facts"); onClicked: root.readRequested() }
        Button { objectName: "pairDeviceButton"; text: qsTr("Pair device…"); enabled: !!root.serverState.can_pair; onClicked: root.pairRequested() }
    }
    Button { objectName: "stopServerButton"; text: qsTr("Stop server…"); enabled: !!root.serverState.can_stop; onClicked: root.prepareRequested("stop") }
    Button { objectName: "setupServerButton"; text: qsTr("Start personal server…"); visible: !!root.serverState.can_setup; onClicked: root.prepareRequested("setup") }
    Repeater {
        model: [ [qsTr("Version"),root.serverState.version], [qsTr("Platform"),root.serverState.platform], [qsTr("API revisions"),root.serverState.api ? root.serverState.api.join("–") : null], [qsTr("Process ID"),root.serverState.pid], [qsTr("Instance"),root.serverState.instance_id], [qsTr("Host"),root.serverState.host_id], [qsTr("Tailscale"),root.serverState.tailscale_state], [qsTr("Started"),root.serverState.started_at], [qsTr("Sessions"),root.serverState.sessions], [qsTr("Commit"),root.serverState.commit], [qsTr("Built"),root.serverState.built_at] ]
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            Label { textFormat: Text.PlainText; text: modelData[0]; Layout.preferredWidth: 120 }
            Label { textFormat: Text.PlainText; text: modelData[1] === null || modelData[1] === undefined ? qsTr("Not reported") : String(modelData[1]); wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
    }
    Label { textFormat: Text.PlainText;
        text: root.serverState.uptime_seconds === null || root.serverState.uptime_seconds === undefined ? qsTr("Running duration unavailable") : qsTr("Running duration: %1 minutes").arg(Math.floor(root.serverState.uptime_seconds / 60))
    }
    Repeater {
        model: root.serverState.listeners || []
        delegate: RowLayout {
            required property var modelData
            Layout.fillWidth: true
            Label { textFormat: Text.PlainText; text: modelData.transport + ": " + (modelData.address || qsTr("Not reported")); wrapMode: Text.Wrap; Layout.fillWidth: true }
            Button { text: qsTr("Copy address"); enabled: !!modelData.address; onClicked: root.copyRequested(modelData.address) }
        }
    }
    Label { textFormat: Text.PlainText; text: root.serverState.setup_running ? qsTr("Starting once…") : (root.serverState.setup_result ? root.serverState.setup_result.message : ""); visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
    Button { text: qsTr("Connect"); visible: !!root.serverState.setup_result; enabled: !root.serverState.connected && !root.serverState.setup_running; onClicked: root.connectRequested() }
    Button { text: qsTr("Review reported endpoint…"); visible: !!root.serverState.setup_result && !!root.serverState.setup_result.endpoint; onClicked: root.editEndpointRequested(root.serverState.setup_result.endpoint) }
    Label { textFormat: Text.PlainText; text: root.serverState.stop_result || ""; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
}
