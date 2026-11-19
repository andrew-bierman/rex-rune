import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root
    required property var frontend
    readonly property var browser: frontend.state.files ? frontend.state.files.directory : null
    property var captured: ({})
    title: browser && browser.go ? qsTr("Go to directory") : qsTr("Insert path")
    modal: true
    width: Math.min(580, parent.width - 32)
    height: Math.min(480, parent.height - 32)
    anchors.centerIn: parent
    standardButtons: Dialog.Cancel
    onClosed: editDelay.stop()
    onRejected: frontend.fileAction("directory.close", {}, captured)
    onBrowserChanged: {
        if (browser) {
            captured = browser.target
            if (!visible) { path.text = browser.text; open(); path.forceActiveFocus() }
        } else close()
    }
    Timer {
        id: editDelay
        interval: 120
        onTriggered: if (root.browser) frontend.fileAction("directory.edit", {text: path.text}, root.captured)
    }
    contentItem: ColumnLayout {
        TextField {
            id: path
            objectName: "directoryPath"
            Layout.fillWidth: true
            placeholderText: qsTr("Working directory, ~, or a path")
            onTextEdited: editDelay.restart()
        }
        Label { textFormat: Text.PlainText; text: root.browser ? root.browser.problem : ""; visible: text.length > 0; wrapMode: Text.Wrap; Layout.fillWidth: true }
        BusyIndicator { running: root.browser ? root.browser.pending : false; visible: running; Layout.preferredHeight: 28 }
        ListView {
            id: list
            objectName: "directoryRows"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.browser ? root.browser.rows : []
            delegate: ItemDelegate {
                id: entry
                required property int index
                objectName: "directoryChoice" + index
                required property var modelData
                width: ListView.view.width
                text: (modelData.directory ? "▸ " : "") + modelData.label
                contentItem: Label { text: entry.text; textFormat: Text.PlainText; elide: Text.ElideRight; verticalAlignment: Text.AlignVCenter }
                onClicked: frontend.fileAction("directory.choose", {path: modelData.path}, root.captured)
            }
        }
        Label { visible: root.browser ? root.browser.truncated : false; text: qsTr("More paths match. Edit the path to narrow the list."); wrapMode: Text.Wrap; Layout.fillWidth: true }
        Label { text: qsTr("Inserts text only. Press Enter in the terminal when ready."); wrapMode: Text.Wrap; Layout.fillWidth: true }
    }
}
