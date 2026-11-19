import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Dialog {
    id: root
    required property var frontend
    readonly property var download: frontend.state.files ? frontend.state.files.download : null
    property var captured: ({})
    readonly property var transfer: download && download.id ? (frontend.state.downloads || []).find(r => r.host === captured.host && r.id === download.id) : null
    function submit() {
        if (!submitButton.enabled) return
        frontend.fileAction(root.transfer && root.transfer.confirmation ? "download.confirm" : "download.start", {path: path.text, directory: folder.text}, root.captured)
    }
    title: qsTr("Download file")
    modal: true
    anchors.centerIn: parent
    width: Math.min(540, parent.width - 32)
    closePolicy: Popup.CloseOnEscape
    onDownloadChanged: {
        if (download) {
            captured = download.target
            if (!visible) { path.text = ""; folder.text = download.directory; open(); path.forceActiveFocus() }
        } else close()
    }
    onTransferChanged: if (transfer && transfer.confirmation) cancel.forceActiveFocus()
    onClosed: if (download) frontend.fileAction("download.close", {}, captured)
    contentItem: ColumnLayout {
        Label { text: qsTr("File on the host"); Layout.fillWidth: true }
        TextField { id: path; objectName: "downloadPath"; Layout.fillWidth: true; enabled: !root.download || !root.download.id; placeholderText: qsTr("/path/to/file or ~/file"); Keys.onReturnPressed: root.submit(); Keys.onEnterPressed: root.submit() }
        Label { text: qsTr("Save in (also remembers the download folder)"); Layout.fillWidth: true }
        TextField { id: folder; objectName: "downloadFolder"; Layout.fillWidth: true; enabled: path.enabled; Keys.onReturnPressed: root.submit(); Keys.onEnterPressed: root.submit() }
        Label { textFormat: Text.PlainText; text: root.download && root.download.unsupported ? qsTr("Update Rex on this host to download files (API 13 required).") : root.transfer ? root.transfer.status : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { text: qsTr("Downloaded files are saved only. They are never opened or run."); wrapMode: Text.Wrap; Layout.fillWidth: true }
    }
    footer: DialogButtonBox {
        Button { id: cancel; objectName: "downloadCancel"; text: qsTr("Cancel"); Keys.onReturnPressed: root.reject(); Keys.onEnterPressed: root.reject(); DialogButtonBox.buttonRole: DialogButtonBox.RejectRole }
        Button {
            id: submitButton
            objectName: "downloadStart"
            text: root.transfer && root.transfer.confirmation ? qsTr("Download anyway") : qsTr("Download")
            enabled: root.download && !root.download.unsupported && (root.transfer ? root.transfer.confirmation === true : !root.download.id) && path.text.trim().length > 0
            onClicked: root.submit()
            Keys.onReturnPressed: root.submit()
            Keys.onEnterPressed: root.submit()
        }
        onRejected: root.reject()
    }
}
