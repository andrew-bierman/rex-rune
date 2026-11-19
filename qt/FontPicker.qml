import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Popup {
    id: popup
    required property var controller
    property bool fallback: false
    signal selected(string family)
    width: Math.min(440,parent.width-32)
    height: Math.min(420,parent.height-32)
    anchors.centerIn: parent
    modal: true; focus: true
    onOpened: { search.clear(); search.forceActiveFocus() }
    contentItem: ColumnLayout {
        Label { text: popup.fallback ? qsTr("Add fallback font") : qsTr("Terminal font"); font.bold: true; textFormat: Text.PlainText }
        TextField { id: search; Layout.fillWidth: true; placeholderText: qsTr("Find an installed or bundled family") }
        ListView {
            id: list
            Layout.fillWidth: true; Layout.fillHeight: true
            clip: true
            model: popup.controller.families.filter(name => name.toLowerCase().includes(search.text.toLowerCase()))
            ScrollBar.vertical: ScrollBar {}
            delegate: ItemDelegate {
                required property string modelData
                width: list.width
                contentItem: Label { text: modelData; textFormat: Text.PlainText; font.family: modelData; elide: Text.ElideRight }
                onClicked: { popup.selected(modelData); popup.close() }
            }
        }
        Button { text: qsTr("Cancel"); onClicked: popup.close() }
    }
}
