import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
ColumnLayout {
    id: page
    required property var controller
    required property var notifications
    property string query: ""
    readonly property var config: controller.values.notifications || ({})
    function matches(words) { return !query || ("notifications sounds activity " + words).toLowerCase().includes(query.toLowerCase()) }
    Layout.fillWidth: true
    Label { text: qsTr("Notifications and sounds"); font.bold: true; visible: page.matches("") }
    Repeater {
        model: [
            {title:qsTr("Desktop notifications"), path:"system_notifications", defaultValue:true},
            {title:qsTr("Working"), path:"working", defaultValue:false},
            {title:qsTr("Needs you"), path:"blocked", defaultValue:true},
            {title:qsTr("Error"), path:"error", defaultValue:true},
            {title:qsTr("Done"), path:"done", defaultValue:true},
            {title:qsTr("Program messages"), path:"program_messages", defaultValue:true},
            {title:qsTr("Play activity sounds"), path:"play_sounds", defaultValue:false}
        ]
        PreferenceRow {
            required property var modelData
            controller:page.controller;title:modelData.title;path:"notifications."+modelData.path
            words:"notifications sounds activity";query:page.query;kind:"toggle"
            value:page.config[modelData.path] === undefined ? modelData.defaultValue : page.config[modelData.path]
        }
    }
    Label { visible:page.matches("service health test desktop");text:qsTr("Notification service: %1").arg(page.notifications.service);textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true;objectName:"notificationHealth" }
    Label { visible:page.matches("service health test desktop") && Boolean(page.notifications.testResult);text:page.notifications.testResult;textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true;objectName:"notificationTestResult" }
    RowLayout {
        visible:page.matches("service health test desktop")
        Button { objectName:"notificationCheck";text:qsTr("Check service");onClicked:page.notifications.check() }
        Button { objectName:"notificationTest";text:qsTr("Send test notification");onClicked:page.notifications.test() }
    }
    Label { visible:page.matches("player sound health test");text:page.notifications.soundHealth;textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true;objectName:"soundHealth" }
    RowLayout {
        visible:page.matches("player sound health test")
        Button { objectName:"soundCheck";text:qsTr("Check sound player");onClicked:page.notifications.checkSound() }
        Button { objectName:"soundTest";text:qsTr("Play test sound");onClicked:page.notifications.testSound() }
    }
    Label { visible:page.matches("");text:qsTr("Only unseen, unmuted tabs notify. Sounds use the same event switches and remain off until enabled. Test buttons act immediately.");textFormat:Text.PlainText;wrapMode:Text.Wrap;Layout.fillWidth:true }
}
