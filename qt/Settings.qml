import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import RexQt 1.0
Popup {
    id: popup
    required property var frontend
    property double pendingApply:0
    signal serverRequested()
    signal shortcutsRequested()
    signal fullscreenRequested()
    signal finished()
    width:Math.min(760,parent.width-24)
    height:Math.min(640,parent.height-24)
    anchors.centerIn:parent
    modal:true;focus:true
    closePolicy:Popup.CloseOnEscape
    SettingsController { id:controller;objectName:"preferencesController" }
    onOpened: { pendingApply=0;frontend.dismissPreferencesProblem();controller.begin(frontend.preferences);search.forceActiveFocus() }
    onClosed: { pendingApply=0;frontend.dismissPreferencesProblem();controller.cancel();finished() }
    Connections {
        target:popup.frontend
        function onPreferencesSaved(requestId) { if(popup.pendingApply===requestId) { popup.pendingApply=0;popup.close() } }
        function onPreferencesRejected(requestId) { if(popup.pendingApply===requestId) popup.pendingApply=0 }
    }
    contentItem:ColumnLayout {
        spacing:8
        RowLayout {
            Layout.fillWidth:true
            Button { text:qsTr("Back");enabled:controller.canBack;onClicked:controller.back() }
            Label { text:qsTr("Settings");font.bold:true;Layout.fillWidth:true }
            Button { objectName:"serverSettingsButton"; text:qsTr("Server…"); onClicked:popup.serverRequested() }
            Button { objectName:"shortcutsSettingsButton";text:qsTr("Shortcuts…");onClicked:popup.shortcutsRequested() }
        }
        TextField { id:search;objectName:"settingsSearch";Layout.fillWidth:true;placeholderText:qsTr("Search all settings");onTextChanged: { if(scroll.contentItem) scroll.contentItem.contentY=0 } }
        RowLayout {
            Button { text:qsTr("Terminal");checkable:true;checked:controller.page==="Terminal";onClicked:controller.page="Terminal" }
            Button { text:qsTr("Interface");checkable:true;checked:controller.page==="Interface";onClicked:controller.page="Interface" }
            Button { text:qsTr("Notifications");checkable:true;checked:controller.page==="Notifications";onClicked: { controller.page="Notifications";frontend.notifications.check();frontend.notifications.checkSound() } }
            Button { text:qsTr("Fullscreen");visible:!search.text || "fullscreen window".includes(search.text.toLowerCase());onClicked:popup.fullscreenRequested() }
        }
        ScrollView {
            id:scroll
            Layout.fillWidth:true;Layout.fillHeight:true
            clip:true
            contentWidth:availableWidth
            ColumnLayout {
                width:scroll.availableWidth
                NotificationSettings { controller:controller;notifications:popup.frontend.notifications;query:search.text;visible:search.text.length>0 || controller.page==="Notifications";Layout.fillWidth:true }
                TerminalSettings { controller:controller;query:search.text;visible:search.text.length>0 || controller.page==="Terminal";Layout.fillWidth:true }
                InterfaceSettings { controller:controller;query:search.text;visible:search.text.length>0 || controller.page==="Interface";Layout.fillWidth:true }
            }
        }
        Label { visible:Boolean(popup.frontend.preferencesProblem);text:popup.frontend.preferencesProblem;textFormat:Text.PlainText;Layout.fillWidth:true;wrapMode:Text.Wrap }
        RowLayout {
            Layout.fillWidth:true
            Button { objectName:"resetPreferences";text:qsTr("Reset…");onClicked:resetQuestion.open() }
            Item { Layout.fillWidth:true }
            Button { objectName:"cancelPreferences";text:qsTr("Cancel");onClicked:popup.close() }
            Button { objectName:"applyPreferences";text:qsTr("Apply");enabled:!popup.pendingApply;onClicked: { popup.pendingApply=frontend.savePreferences(controller.patch()) } }
        }
    }
    Dialog {
        id:resetQuestion
        title:qsTr("Reset terminal and interface preferences?")
        anchors.centerIn:parent
        width:Math.min(440,popup.width-24)
        height:200
        modal:true
        contentItem:Label { text:qsTr("Restore the defaults in this draft. Choose Apply to save, or Cancel to keep your current preferences.");textFormat:Text.PlainText;wrapMode:Text.Wrap }
        footer:DialogButtonBox {
            Button { id:keepPreferences;objectName:"keepPreferences";text:qsTr("Keep preferences");DialogButtonBox.buttonRole:DialogButtonBox.RejectRole }
            Button { objectName:"confirmResetPreferences";text:qsTr("Reset draft");DialogButtonBox.buttonRole:DialogButtonBox.AcceptRole }
            onAccepted: { controller.reset();resetQuestion.close() }
            onRejected:resetQuestion.close()
        }
        onOpened:keepPreferences.forceActiveFocus()
    }
}
