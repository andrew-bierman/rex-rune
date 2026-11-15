import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: popup
    required property var activity
    signal viewRequested(bool sessionView)
    property int iconCount: 1
    property string colorMode: "never"
    objectName: "activityDialog"
    title: qsTr("Activity")
    modal: true
    width: Math.min(640, parent ? parent.width - 32 : 640)
    height: Math.min(560, parent ? parent.height - 32 : 560)
    standardButtons: Dialog.Close
    onOpened: search.forceActiveFocus()
    contentItem: ColumnLayout {
        RowLayout {
            TextField {
                id: search
                objectName: "activityFilter"
                placeholderText: qsTr("Filter activity")
                Layout.fillWidth: true
                text: popup.activity.filter
                onTextEdited: popup.activity.filter = text
                Keys.onDownPressed: popup.activity.step(1)
                Keys.onUpPressed: popup.activity.step(-1)
                Keys.onReturnPressed: popup.activity.activateSelected()
                Keys.onTabPressed: popup.viewRequested(!popup.activity.sessionView)
                Keys.onBacktabPressed: popup.viewRequested(!popup.activity.sessionView)
            }
            ComboBox {
                objectName: "activityView"
                model: [qsTr("Priority"), qsTr("Session")]
                currentIndex: popup.activity.sessionView ? 1 : 0
                onActivated: popup.viewRequested(currentIndex === 1)
            }
        }
        Label { visible: (popup.activity.inventory.offlineCount || 0) > 0; text: qsTr("%1 hosts are not connected.").arg(popup.activity.inventory.offlineCount || 0) }
        Label { visible: !!popup.activity.inventory.reading; text: qsTr("Reading activity…") }
        Label { visible: !!popup.activity.inventory.incomplete; text: qsTr("Some activity could not be read.") }
        ListView {
            id: list
            objectName: "activityRows"
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: popup.activity.rows
            currentIndex: {
                for (let i = 0; i < model.length; ++i) if (model[i].key === popup.activity.selectedKey) return i
                return -1
            }
            onCurrentIndexChanged: if (currentIndex >= 0) positionViewAtIndex(currentIndex, ListView.Contain)
            ScrollBar.vertical: ScrollBar {}
            delegate: Column {
                required property var modelData
                property var rowData: modelData
                width: list.width
                Label {
                    visible: modelData.showGroup
                    text: modelData.group
                    textFormat: Text.PlainText
                    font.bold: true
                    padding: 8
                }
                ItemDelegate {
                    id: entry
                    width: list.width
                    highlighted: popup.activity.selectedKey === modelData.key
                    onClicked: { popup.activity.selectedKey = modelData.key; popup.activity.activateSelected() }
                    background: Rectangle {
                        radius: 3
                        color: entry.highlighted ? Qt.alpha(popup.palette.highlight, .18) : entry.down ? Qt.rgba(1, 1, 1, .11) : entry.hovered ? Qt.rgba(1, 1, 1, .07) : "transparent"
                        border.color: popup.palette.highlight
                        border.width: entry.highlighted || entry.activeFocus ? 1 : 0
                    }
                    contentItem: ColumnLayout {
                        RowLayout {
                            Repeater {
                                model: modelData.icons.slice(0, popup.iconCount)
                                ProgramIcon {
                                    required property var modelData
                                    required property int index
                                    icon: modelData
                                    colored: popup.colorMode === "always" || (popup.colorMode === "active_tab" && rowData.inFront) || (popup.colorMode === "active_pane" && rowData.inFront && index === 0)
                                }
                            }
                            Label { color: popup.palette.windowText; text: modelData.label; textFormat: Text.PlainText; elide: Text.ElideRight; Layout.fillWidth: true }
                            Label { color: popup.palette.windowText; text: modelData.listWord; textFormat: Text.PlainText }
                            Label { color: popup.palette.windowText; visible: !!modelData.age; text: modelData.age ? modelData.age.text : ""; Accessible.description: modelData.age ? modelData.age.description : "" }
                            Label { visible: modelData.muted; text: qsTr("Muted") }
                        }
                        Label { color: popup.palette.windowText; text: modelData.hostLabel + " · " + modelData.sessionLabel; textFormat: Text.PlainText; elide: Text.ElideRight; Layout.fillWidth: true }
                        Label { color: popup.palette.windowText; visible: !!modelData.message; text: modelData.message || ""; textFormat: Text.PlainText; elide: Text.ElideRight; Layout.fillWidth: true }
                        ProgressBar { visible: !!modelData.progress; Layout.fillWidth: true; from: 0; to: 100; value: modelData.progress && modelData.progress.percent !== null ? modelData.progress.percent : 0; indeterminate: !!modelData.progress && modelData.progress.percent === null }
                    }
                }
            }
        }
        Label { visible: popup.activity.rows.length === 0; text: qsTr("No matching activity") }
    }
}
