import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Popup {
    id: popup
    required property var frontend
    property var facts: frontend.state.maintenance || ({})
    width: Math.min(700,parent.width-24); height: Math.min(650,parent.height-24)
    anchors.centerIn:parent; modal:true; focus:true
    onOpened: frontend.maintenanceAction("status")
    onClosed: frontend.maintenanceAction("cancel")
    function summary(agent) {
        const rows=(facts.status || {}).harnesses || []
        const h=rows.find(r=>r.id===agent)
        if (!h) return qsTr("Not detected")
        const items=(h.items || []).filter(i=>i.state!=="unsupported")
        if(items.some(i=>i.state==="foreign")) return qsTr("Conflict — existing files belong to someone else")
        if(items.some(i=>i.state==="unreadable")) return qsTr("Unreadable")
        if(items.length && items.every(i=>i.state==="current")) return qsTr("Installed")
        if(items.every(i=>i.state==="missing")) return qsTr("Missing")
        return qsTr("Update needed")
    }
    contentItem: ColumnLayout {
        Label { text:qsTr("Local agent integrations");font.bold:true }
        Label { Layout.fillWidth:true;wrapMode:Text.Wrap;text:qsTr("Uses the local rex skill command, with server autostart disabled. Every change is previewed first. OMP shares Pi’s skill files.");textFormat:Text.PlainText }
        CheckBox { id:naming;objectName:"integrationNaming";text:qsTr("Enable agent session naming");checked:true }
        CheckBox { id:force;objectName:"integrationForce";text:qsTr("Replace conflicting files (explicit force)") }
        ScrollView {
            Layout.fillWidth:true;Layout.fillHeight:true;contentWidth:availableWidth;clip:true
            ColumnLayout {
                width:parent.width
                Repeater {
                    model:popup.facts.agents || []
                    delegate: Frame {
                        required property var modelData
                        Layout.fillWidth:true
                        ColumnLayout {
                            anchors.fill:parent
                            Label { text:modelData.name;font.bold:true }
                            Label { text:popup.summary(modelData.id);wrapMode:Text.Wrap;Layout.fillWidth:true;textFormat:Text.PlainText }
                            Label {
                                Layout.fillWidth:true;wrapMode:Text.Wrap;textFormat:Text.PlainText
                                text: {
                                    const h=((popup.facts.status || {}).harnesses || []).find(r=>r.id===modelData.id)
                                    return h ? (h.detected ? qsTr("Agent detected") : qsTr("Agent not detected"))+"\n"+(h.items || []).map(i=>i.kind+": "+i.state+(i.path ? " — "+i.path : "")).join("\n") : ""
                                }
                            }
                            RowLayout {
                                Button { objectName:"integrationSetup_"+modelData.id;text:qsTr("Set up…");enabled:!popup.facts.busy;onClicked:frontend.maintenanceAction("preview",{agent:modelData.id,naming:naming.checked,force:force.checked}) }
                                Button { objectName:"integrationRemove_"+modelData.id;text:qsTr("Remove…");enabled:!popup.facts.busy;onClicked:frontend.maintenanceAction("preview",{agent:modelData.id,remove:true}) }
                            }
                        }
                    }
                }
                Label { visible:Boolean(popup.facts.error);text:popup.facts.error || "";textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true }
                Label { visible:Boolean(popup.facts.message);text:popup.facts.message || "";textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true }
            }
        }
        RowLayout {
            Button { objectName:"integrationRefresh";text:qsTr("Refresh");enabled:!popup.facts.busy;onClicked:frontend.maintenanceAction("status") }
            BusyIndicator { running:popup.facts.busy || false;implicitWidth:28;implicitHeight:28 }
            Item { Layout.fillWidth:true }
            Button { objectName:"integrationClose";text:qsTr("Close");onClicked:popup.close() }
        }
    }
    Dialog {
        id:preview;objectName:"integrationPreview";title:qsTr("Review local file changes")
        parent:Overlay.overlay;anchors.centerIn:parent;width:Math.min(620,parent.width-40);height:Math.min(470,parent.height-40);modal:true
        visible:Boolean(popup.facts.preview)
        contentItem:ScrollView { TextArea { objectName:"integrationPreviewText";readOnly:true;wrapMode:TextEdit.Wrap;text:JSON.stringify(popup.facts.preview || {},null,2);textFormat:TextEdit.PlainText } }
        footer:DialogButtonBox {
            Button { objectName:"integrationCancel";text:qsTr("Cancel");DialogButtonBox.buttonRole:DialogButtonBox.RejectRole }
            Button { objectName:"integrationConfirm";text:qsTr("Apply changes");DialogButtonBox.buttonRole:DialogButtonBox.AcceptRole }
            onRejected:frontend.maintenanceAction("cancel")
            onAccepted:frontend.maintenanceAction("confirm")
        }
        onRejected:frontend.maintenanceAction("cancel")
    }
}
