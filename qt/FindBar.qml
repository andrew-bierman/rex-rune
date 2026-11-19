import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: bar
    property var terminal: null
    property string capturedTarget: ""
    function targetKey() {
        if (!terminal) return ""
        const s = terminal.paneState
        return [s.host, s.epoch, s.session, s.window, s.pane, s.terminal ? s.terminal.generation : ""].join("/")
    }
    Connections {
        target: bar.terminal
        function onTextChanged() { if (bar.visible && bar.capturedTarget !== bar.targetKey()) bar.close() }
        function onPaneIdChanged() { bar.close() }
    }
    visible: false
    color: "#0d1524"
    border.color: "#7cc4ff"
    width: Math.min(parent.width, 460)
    height: 52
    Keys.onEscapePressed: close()
    signal closed()
    function open() { capturedTarget = targetKey(); visible = true; if (terminal) terminal.search(query.text); query.forceActiveFocus(); query.selectAll() }
    function close() { if (terminal && capturedTarget === targetKey()) terminal.search(""); visible = false; closed() }
    RowLayout {
        anchors.fill: parent
        anchors.margins: 6
        TextField {
            id: query
            objectName: "terminalSearch"
            Layout.fillWidth: true
            placeholderText: "Find in terminal"
            Accessible.name: "Find in terminal"
            onTextEdited: if (bar.terminal) bar.terminal.search(text)
            onAccepted: if (bar.terminal) bar.terminal.searchStep(false)
            Keys.onEscapePressed: bar.close()
        }
        Label {
            text: {
                const s = bar.terminal ? bar.terminal.paneState.frame : null
                const found = s ? s.search : null
                return found ? ((found.index === null ? 0 : found.index + 1) + "/" + found.total) : ""
            }
            color: "#93a3b8"
        }
        Button { Layout.preferredWidth: 32; objectName: "findPrevious"; text: "↑"; Accessible.name: "Previous match"; onClicked: if (bar.terminal) bar.terminal.searchStep(true) }
        Button { Layout.preferredWidth: 32; objectName: "findNext"; text: "↓"; Accessible.name: "Next match"; onClicked: if (bar.terminal) bar.terminal.searchStep(false) }
        Button { Layout.preferredWidth: 32; text: "×"; Accessible.name: "Close find"; onClicked: bar.close() }
    }
}
