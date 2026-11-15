import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: heading
    required property var pane
    required property var workspace
    property bool iconVisible: true
    property bool iconColored: false
    property bool focused: false
    implicitHeight: 28
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        spacing: 0
        ProgramIcon { visible: heading.iconVisible && !!heading.pane.activity; icon: heading.pane.activity ? heading.pane.activity.icon : null; colored: heading.iconColored }
        Label {
            textFormat: Text.PlainText
            text: heading.pane.label || qsTr("Terminal")
            Layout.fillWidth: true
            elide: Text.ElideRight
            color: heading.focused ? "#e8eef6" : "#93a3b8"
            ToolTip.text: text
            ToolTip.visible: hover.hovered
            HoverHandler { id: hover }
        }
        ToolButton {
            objectName: "paneSplitRight-" + heading.pane.id
            Layout.preferredWidth: 28
            Layout.preferredHeight: 26
            visible: heading.width >= 200
            text: "↔"
            Accessible.name: qsTr("Split right")
            ToolTip.text: Accessible.name
            ToolTip.visible: hovered
            onClicked: heading.workspace.run("pane.split", {direction: "right"}, heading.pane.id)
        }
        ToolButton {
            objectName: "paneSplitDown-" + heading.pane.id
            Layout.preferredWidth: 28
            Layout.preferredHeight: 26
            visible: heading.width >= 200
            text: "↕"
            Accessible.name: qsTr("Split down")
            ToolTip.text: Accessible.name
            ToolTip.visible: hovered
            onClicked: heading.workspace.run("pane.split", {direction: "down"}, heading.pane.id)
        }
        ToolButton {
            objectName: "paneZoom-" + heading.pane.id
            Layout.preferredWidth: 28
            Layout.preferredHeight: 26
            visible: heading.width >= 140
            text: "⛶"
            Accessible.name: qsTr("Zoom or restore pane")
            ToolTip.text: Accessible.name
            ToolTip.visible: hovered
            onClicked: heading.workspace.run("pane.zoom", {}, heading.pane.id)
        }
        ToolButton {
            objectName: "paneClose-" + heading.pane.id
            Layout.preferredWidth: 28
            Layout.preferredHeight: 26
            visible: heading.width >= 110
            text: "×"
            Accessible.name: qsTr("Close pane")
            ToolTip.text: Accessible.name
            ToolTip.visible: hovered
            onClicked: heading.workspace.run("pane.close", {}, heading.pane.id)
        }
        ToolButton {
            objectName: "paneMenu-" + heading.pane.id
            Layout.preferredWidth: 28
            Layout.preferredHeight: 26
            text: "⋯"
            Accessible.name: qsTr("Pane actions")
            onClicked: heading.workspace.openMenu(this, heading.pane.id)
        }
    }
}
