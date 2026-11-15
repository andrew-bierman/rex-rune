import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Popup {
    id: popup
    required property var actions
    required property var frontend
    signal finished()
    width: Math.min(820, parent.width - 32)
    height: Math.min(600, parent.height - 32)
    anchors.centerIn: parent
    modal: true
    focus: true
    padding: 16
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    property string selectedAction: "pane.copy"
    property var presetArgs: ({})
    property var selectedArgs: selectedAction === "client.scroll" ? ({lines: scrollLines.value}) :
                               selectedAction === "client.mode.enter" ? ({name: modePicker.currentText, once: modeOnce.checked}) : presetArgs
    property var rows: {
        const result = []
        for (const row of actions.commands) {
            const variants = row.kind === "server" ? actions.bindings.filter(binding => binding.action === row.id) : []
            const seen = new Set()
            if (row.kind === "client" || (!variants.length && !(row.args && row.args.required && row.args.required.length))) variants.push({args: {}})
            for (const variant of variants) {
                const args = variant.args || {}
                const key = JSON.stringify(args)
                if (seen.has(key)) continue
                seen.add(key)
                const words = (row.title + " " + row.id + " " + (row.hint || "") + " " + actions.bindings.filter(binding => binding.action === row.id).map(binding => binding.keys.join(", ")).join(" ")).toLowerCase()
                if (words.includes(search.text.toLowerCase())) result.push(Object.assign({}, row, {bindingArgs: args}))
            }
        }
        return result
    }
    function argumentWords(args) { return Object.keys(args || {}).map(key => key + " " + (typeof args[key] === "object" ? JSON.stringify(args[key]) : args[key])).join(", ") }
    property string selectedKind: { const row = actions.commands.find(row => row.id === selectedAction); return row ? row.kind : "client" }
    property string selectedTitle: { const row = actions.commands.find(row => row.id === selectedAction); return row ? row.title : selectedAction }
    function sourceLabel(source) {
        if (source === "recorded") return qsTr("Recorded here")
        if (source === "settings") return qsTr("Override file")
        if (!source || source === "default") return qsTr("Default")
        return source
    }
    property var effective: actions.bindings.filter(row => row.action === selectedAction && (selectedKind !== "server" || JSON.stringify(row.args || {}) === JSON.stringify(presetArgs)))
    property var keyRows: effective.map(row => Object.assign({}, row, {removed: false})).concat(
        ((frontend.state.keymap && frontend.state.keymap.recorded) || []).filter(row => row.remove && row.action === selectedAction))
    onOpened: search.forceActiveFocus()
    onClosed: { actions.stopRecording(); finished() }
    contentItem: ColumnLayout {
        spacing: 10
        Label { text: qsTr("Settings · Shortcuts"); font.bold: true }
        TextField {
            id: search
            objectName: "shortcutSearch"
            Layout.fillWidth: true
            placeholderText: qsTr("Find an action or shortcut")
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            ListView {
                id: list
                Layout.preferredWidth: Math.max(190, popup.width * .4)
                Layout.fillHeight: true
                clip: true
                model: popup.rows
                ScrollBar.vertical: ScrollBar {}
                delegate: ItemDelegate {
                    required property var modelData
                    width: list.width
                    highlighted: popup.selectedAction === modelData.id
                    text: modelData.title + (Object.keys(modelData.bindingArgs || {}).length ? " · " + popup.argumentWords(modelData.bindingArgs) : "")
                    onClicked: { popup.selectedAction = modelData.id; popup.presetArgs = modelData.bindingArgs || {} }
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Label { text: popup.selectedTitle; font.bold: true; Layout.fillWidth: true; elide: Text.ElideMiddle }
                ListView {
                    id: keys
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: popup.keyRows
                    ScrollBar.vertical: ScrollBar {}
                    delegate: ItemDelegate {
                        required property var modelData
                        width: keys.width
                        contentItem: ColumnLayout {
                            Label { text: (modelData.hint || modelData.keys.join(", ")) + (modelData.remove ? " · " + qsTr("Removed") : ""); Layout.fillWidth: true; wrapMode: Text.Wrap }
                            Label { visible: !modelData.remove; text: popup.sourceLabel(modelData.source) + (modelData.mode ? " · " + modelData.mode : ""); opacity: .65; Layout.fillWidth: true; elide: Text.ElideMiddle }
                            RowLayout {
                                Button {
                                    visible: !modelData.remove
                                    objectName: "removeShortcut-" + modelData.keys.join(">")
                                    text: qsTr("Remove")
                                    onClicked: popup.frontend.recordShortcut(popup.selectedAction, modelData.keys, modelData.args || {}, modelData.mode || "", "remove")
                                }
                                Button {
                                    visible: modelData.remove || modelData.source === "recorded"
                                    text: qsTr("Restore")
                                    onClicked: popup.frontend.recordShortcut(popup.selectedAction, modelData.keys, modelData.args || {}, modelData.mode || "", "restore")
                                }
                            }
                        }
                    }
                }
                Label { visible: !popup.keyRows.length; text: qsTr("No effective binding"); opacity: .65 }
                RowLayout {
                    visible: popup.selectedAction === "client.scroll"
                    Label { text: qsTr("Lines (negative scrolls up)") }
                    SpinBox { id: scrollLines; from: -65535; to: 65535; value: -10; editable: true }
                }
                RowLayout {
                    visible: popup.selectedAction === "client.mode.enter"
                    Label { text: qsTr("Mode") }
                    ComboBox { id: modePicker; model: ((frontend.state.keymap && frontend.state.keymap.modes) || []).map(mode => mode.name) }
                    CheckBox { id: modeOnce; text: qsTr("One action") }
                }
                Label {
                    text: actions.recording ? qsTr("Press the shortcut keys, then choose Done") : actions.recordedKeys.join(", ")
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
                RowLayout {
                    ComboBox { id: bindingMode; Layout.preferredWidth: 140; Layout.minimumWidth: 100; Layout.maximumWidth: 140; Accessible.name: qsTr("Mode for recorded shortcut"); model: [qsTr("Default mode")].concat(((frontend.state.keymap && frontend.state.keymap.modes) || []).map(mode => mode.name)) }
                    Button { objectName: "recordShortcut"; Layout.preferredWidth: 80; Layout.minimumWidth: 64; text: actions.recording ? qsTr("Done") : qsTr("Record"); onClicked: actions.recording ? actions.stopRecording() : actions.beginRecording() }
                    Button {
                        objectName: "saveShortcut"
                        Layout.preferredWidth: 64
                        Layout.minimumWidth: 52
                        text: qsTr("Bind")
                        enabled: !actions.recording && actions.recordedKeys.length > 0
                        onClicked: popup.frontend.recordShortcut(popup.selectedAction, actions.recordedKeys, popup.selectedArgs, bindingMode.currentIndex > 0 ? bindingMode.currentText : "", "bind")
                    }
                }
            }
        }
        Label {
            text: qsTr("Recorded shortcuts are saved only in Qt's settings. The user override file is read-only.")
            Layout.fillWidth: true
            wrapMode: popup.height >= 500 ? Text.Wrap : Text.NoWrap
            elide: Text.ElideRight
            opacity: .7
        }
        Label {
            text: frontend.state.keymap ? frontend.state.keymap.path : ""
            Layout.fillWidth: true
            elide: Text.ElideMiddle
            opacity: .6
        }
        Label {
            visible: Boolean(frontend.state.keymap && frontend.state.keymap.problem)
            text: frontend.state.keymap ? (frontend.state.keymap.problem || "") : ""
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }
        Repeater {
            model: (frontend.state.keymap && frontend.state.keymap.diagnostics) || []
            delegate: Label {
                required property var modelData
                text: modelData.message
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }
        }
        RowLayout {
            Layout.fillWidth: true
            Button { text: qsTr("Reload override file"); onClicked: frontend.reloadShortcuts() }
            Item { Layout.fillWidth: true }
            Button { text: qsTr("Close"); onClicked: popup.close() }
        }
    }
}
