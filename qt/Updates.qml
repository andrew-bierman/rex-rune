import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Popup {
    id:popup
    required property var frontend
    property var facts:frontend.state.maintenance || ({})
    width:Math.min(580,parent.width-24);height:Math.min(440,parent.height-24)
    anchors.centerIn:parent;modal:true;focus:true
    contentItem:ColumnLayout {
        Label { text:qsTr("Updates — Qt source build");font.bold:true }
        Label { Layout.fillWidth:true;wrapMode:Text.Wrap;text:qsTr("This separate Qt application is built from source. Check signed upstream Rune release metadata to compare the reused core version. To update Qt, update its checkout and rebuild both the Qt frontend and private backend. Rune installers and packages do not update this application.");textFormat:Text.PlainText }
        Label { objectName:"updateResult";Layout.fillWidth:true;wrapMode:Text.Wrap;text:popup.facts.update ? popup.facts.update.message+"\n"+qsTr("Signed Rune release: ")+popup.facts.update.version : "";textFormat:Text.PlainText }
        Label { visible:Boolean(popup.facts.error);Layout.fillWidth:true;wrapMode:Text.Wrap;text:popup.facts.error || "";textFormat:Text.PlainText }
        Item { Layout.fillHeight:true }
        RowLayout {
            Button { objectName:"checkSignedUpdates";text:qsTr("Check signed releases");enabled:!popup.facts.busy;onClicked:frontend.maintenanceAction("check") }
            BusyIndicator { running:popup.facts.busy || false;implicitWidth:28;implicitHeight:28 }
            Button { objectName:"updatesClose";text:qsTr("Close");onClicked:popup.close() }
        }
    }
}
