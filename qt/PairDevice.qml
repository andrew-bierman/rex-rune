import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Dialog {
    id: root
    required property var pair
    signal copyRequested(string text)
    signal pairClosed()
    modal: true
    title: qsTr("Pair device")
    width: Math.min(460, parent.width - 24)
    anchors.centerIn: parent
    standardButtons: Dialog.Close
    onClosed: pairClosed()
    onPairChanged: { if (!pair || !pair.address) close(); qr.requestPaint() }
    onOpened: qr.requestPaint()
    contentItem: ColumnLayout {
        Label { textFormat: Text.PlainText; text: qsTr("Scan the server’s reported HTTPS address on your other device."); wrapMode: Text.Wrap; Layout.fillWidth: true }
        Canvas {
            id: qr
            objectName: "pairQr"
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 280; Layout.preferredHeight: 280
            onPaint: {
                const ctx = getContext("2d")
                ctx.fillStyle = "white"; ctx.fillRect(0, 0, width, height)
                const matrix = root.pair ? root.pair.matrix : null
                if (!matrix || !matrix.length) return
                const scale = Math.max(1, Math.floor(Math.min(width,height)/(matrix.length+8)))
                const offset = Math.floor((width - matrix.length*scale)/2)
                ctx.fillStyle = "black"
                for (let y=0;y<matrix.length;y++) for(let x=0;x<matrix.length;x++) if(matrix[y][x]) ctx.fillRect(offset+x*scale,offset+y*scale,scale,scale)
            }
        }
        Label { textFormat: Text.PlainText; text: root.pair ? root.pair.address || "" : ""; wrapMode: Text.Wrap; Layout.fillWidth: true }
        Button { objectName: "pairCopyAddress"; text: qsTr("Copy address"); enabled: !!root.pair && !!root.pair.address; onClicked: root.copyRequested(root.pair.address) }
    }
}
