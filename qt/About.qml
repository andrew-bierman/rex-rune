import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Popup {
    id:popup
    required property var frontend
    width:Math.min(650,parent.width-24);height:Math.min(570,parent.height-24)
    anchors.centerIn:parent;modal:true;focus:true
    contentItem:ColumnLayout {
        Label { text:qsTr("About Rex Qt Quick");font.bold:true }
        ScrollView { Layout.fillWidth:true;Layout.fillHeight:true;contentWidth:availableWidth
            TextArea { objectName:"aboutReport";readOnly:true;selectByMouse:true;wrapMode:TextEdit.Wrap;textFormat:TextEdit.PlainText;text:(popup.frontend.state.maintenance ? popup.frontend.maintenanceReport() : qsTr("Waiting for backend identity…"))+"\n\n"+popup.frontend.maintenanceLicenses() }
        }
        RowLayout {
            Button { objectName:"copyMaintenanceReport";text:qsTr("Copy safe report");enabled:Boolean(popup.frontend.state.maintenance);onClicked:frontend.copyMaintenanceReport() }
            Item { Layout.fillWidth:true }
            Button { objectName:"aboutClose";text:qsTr("Close");onClicked:popup.close() }
        }
    }
}
