import QtQuick
import QtQuick.Controls

Item {
    id: badge
    required property var icon
    property bool colored: false
    property color ink: glyph.palette.text
    implicitWidth: 18
    implicitHeight: 18
    Accessible.name: icon ? icon.name : qsTr("Shell")
    Label {
        id: glyph
        anchors.fill: parent
        text: badge.icon ? badge.icon.glyph || badge.icon.badge || ">_" : ">_"
        textFormat: Text.PlainText
        color: badge.colored && badge.icon ? badge.icon.tint : badge.ink
        font.family: "Symbols Nerd Font"
        font.pixelSize: 15
        font.bold: true
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
    ToolTip.text: Accessible.name
    ToolTip.visible: hover.hovered
    HoverHandler { id: hover }
}
