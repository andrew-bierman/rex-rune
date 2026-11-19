import QtQuick
import QtQuick.Controls

Label {
    required property var frontend
    readonly property var upload: frontend.state.files ? frontend.state.files.upload : null
    visible: upload !== null
    textFormat: Text.PlainText
    text: upload ? upload.message : ""
    wrapMode: Text.Wrap
    Accessible.name: text
}
