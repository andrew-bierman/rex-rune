import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

RowLayout {
    id: row
    required property var controller
    required property string title
    required property string path
    property string query: ""
    property string words: ""
    property string kind: "choice"
    property var choices: []
    property var value
    property int minimum: 0
    property int maximum: 100
    property int step: 1
    property real divisor: 1
    visible: !query || (title + " " + path + " " + words).toLowerCase().includes(query.toLowerCase())
    Layout.fillWidth: true
    Label { text: row.title; textFormat: Text.PlainText; Layout.fillWidth: true; Layout.minimumWidth: 100; wrapMode: Text.Wrap }
    ComboBox {
        visible: row.kind === "choice"
        objectName: visible ? "setting-" + row.path : ""
        Layout.preferredWidth: 195
        Layout.minimumWidth: 120
        model: row.choices
        textRole: "title"
        valueRole: "value"
        currentIndex: row.choices.findIndex(choice => choice.value === row.value)
        onActivated: row.controller.setValue(row.path, currentValue)
    }
    SpinBox {
        visible: row.kind === "number"
        objectName: visible ? "setting-" + row.path : ""
        editable: true
        from: row.minimum; to: row.maximum; stepSize: row.step
        value: Math.round(Number(row.value) * row.divisor)
        onValueModified: row.controller.setValue(row.path, value / row.divisor)
    }
    Switch {
        visible: row.kind === "toggle"
        objectName: visible ? "setting-" + row.path : ""
        checked: Boolean(row.value)
        Accessible.name: row.title
        onToggled: row.controller.setValue(row.path, checked)
    }
}
