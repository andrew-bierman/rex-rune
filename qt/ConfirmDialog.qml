import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dialog
    objectName: "workspaceConfirmation"
    property var question: null
    signal confirmed(var captured)
    signal cancelled(var captured)
    anchors.centerIn: parent
    width: Math.min(460, parent.width - 32)
    modal: true
    focus: true
    title: question ? question.title : ""
    closePolicy: Popup.CloseOnEscape
    contentItem: Label {
        text: dialog.question ? dialog.question.description : ""
        wrapMode: Text.Wrap
    }
    footer: DialogButtonBox {
        Button {
            id: keep
            objectName: "workspaceKeepRunning"
            text: qsTr("Keep running")
            DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
            highlighted: true
            Keys.onReturnPressed: dialog.reject()
            Keys.onEnterPressed: dialog.reject()
        }
        Button {
            objectName: "workspaceConfirmClose"
            text: qsTr("Close")
            DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
            palette.buttonText: "#ff8a8a"
        }
    }
    onOpened: keep.forceActiveFocus()
    onAccepted: confirmed(question)
    onRejected: cancelled(question)
}
