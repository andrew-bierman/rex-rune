import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Dialog {
    id: root
    required property var frontend
    title: qsTr("Downloads")
    modal: true
    anchors.centerIn: parent
    width: Math.min(580, parent.width - 32)
    height: Math.min(460, parent.height - 32)
    standardButtons: Dialog.Close
    contentItem: ListView {
        clip: true
        model: frontend.state.downloads || []
        delegate: ColumnLayout {
            required property var modelData
            required property int index
            width: ListView.view.width
            Label { text: modelData.name + " · " + modelData.label; textFormat: Text.PlainText; elide: Text.ElideMiddle; Layout.fillWidth: true }
            Label { text: modelData.status; textFormat: Text.PlainText; wrapMode: Text.Wrap; Layout.fillWidth: true }
            ProgressBar { visible: modelData.running; indeterminate: modelData.progress === undefined; value: modelData.progress || 0; Layout.fillWidth: true }
            Button { objectName: "downloadRowAction" + index; visible: modelData.running || !!modelData.saved; text: modelData.saved ? qsTr("Show in folder") : qsTr("Cancel"); onClicked: modelData.saved ? frontend.showDownload(modelData.host, modelData.id) : frontend.fileAction("download.cancel", {id:modelData.id}, {host:modelData.host,epoch:modelData.epoch}) }
        }
    }
}
