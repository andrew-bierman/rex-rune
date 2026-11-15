import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Popup {
    id: popup
    required property var actions
    signal finished()
    width: Math.min(600, parent.width - 32)
    height: Math.min(520, parent.height - 32)
    anchors.centerIn: parent
    modal: true
    focus: true
    padding: 16
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    property var rows: actions.commands.filter(row => {
        const terms = (row.title + " " + row.category + " " + row.id + " " + (row.hint || "")).toLowerCase()
        return terms.includes(search.text.toLowerCase()) && !(row.args && row.args.required && row.args.required.length)
    })
    onOpened: { search.clear(); search.forceActiveFocus(); list.currentIndex = 0 }
    onClosed: finished()
    function choose(index) {
        if (index < 0 || index >= rows.length || !rows[index].enabled) return
        const row = rows[index]
        close()
        actions.run(row.id)
    }
    contentItem: ColumnLayout {
        spacing: 10
        Label { text: qsTr("Commands"); font.bold: true }
        TextField {
            id: search
            objectName: "commandSearch"
            Layout.fillWidth: true
            placeholderText: qsTr("Find a command")
            onTextChanged: list.currentIndex = 0
            Keys.onDownPressed: list.incrementCurrentIndex()
            Keys.onUpPressed: list.decrementCurrentIndex()
            onAccepted: popup.choose(list.currentIndex)
        }
        ListView {
            id: list
            objectName: "commandList"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: popup.rows
            currentIndex: 0
            ScrollBar.vertical: ScrollBar {}
            delegate: ItemDelegate {
                required property var modelData
                required property int index
                width: list.width
                highlighted: list.currentIndex === index
                enabled: modelData.enabled
                onClicked: popup.choose(index)
                contentItem: RowLayout {
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 1
                        Label { text: modelData.title; elide: Text.ElideRight; Layout.fillWidth: true }
                        Label { text: modelData.category; opacity: .65; font.pixelSize: 11 }
                    }
                    Label { text: modelData.hint || ""; opacity: .75 }
                }
            }
        }
        Label { visible: !popup.rows.length; text: qsTr("No commands match"); opacity: .65 }
    }
}
