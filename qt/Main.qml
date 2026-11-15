import "IconPolicy.js" as IconPolicy
import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Layouts
import RexQt 1.0

ApplicationWindow {
    id: root
    objectName: "rexWindow"
    width: 1024
    height: 700
    minimumWidth: 640
    minimumHeight: 480
    visible: true
    title: {
        const host = (backend.hosts.rows || []).find(row => row.id === state.host)
        const session = (state.sessions || []).find(row => row.id === state.session)
        const tab = (state.windows || []).find(row => row.id === state.window)
        return [tab ? tab.label : "", session ? session.label : "", host ? host.label : ""].filter(Boolean).join(" · ") || "Rex Qt Quick"
    }
    property var preferences: backend.preferences
    property var interfacePrefs: preferences.interface || ({})
    property real uiZoom: Math.min(preferences.ui_scale || 1, Math.max(.7, root.width / 640), Math.max(.7, root.height / 480))
    property var chrome: ({rose_pine: ["#191724", "#1f1d2e", "#e0def4", "#908caa", "#c4a7e7"], rose_pine_moon: ["#232136", "#2a273f", "#e0def4", "#908caa", "#c4a7e7"], gruvbox_dark: ["#282828", "#3c3836", "#ebdbb2", "#a89984", "#83a598"]})[preferences.chrome_theme] || ["#001528", "#142337", "#e8eef6", "#93a3b8", "#7cc4ff"]
    font.pixelSize: 14 * uiZoom
    color: chrome[0]
    palette.window: chrome[0]
    palette.windowText: chrome[2]
    palette.base: chrome[1]
    palette.text: chrome[2]
    palette.placeholderText: chrome[3]
    palette.button: chrome[1]
    palette.buttonText: chrome[2]
    palette.highlight: chrome[4]
    palette.highlightedText: chrome[0]
    onActiveChanged: backend.setVisible(active && visible)
    onVisibleChanged: backend.setVisible(active && visible)
    Component.onCompleted: backend.setVisible(active && visible)
    property string activeHost: state.host || ""
    onActiveHostChanged: {
        if (findBar.visible) findBar.close()
        workspaceUI.cancelCaptured()
        hostDialogs.cancelCaptured()
        hostSwitcher.close()
        Qt.callLater(restoreTerminal)
    }
    property var state: backend.state
    property var hostInfo: (backend.hosts.rows || []).find(row => row.id === state.host) || ({})
    property var dividerIds: []
    onStateChanged: {
        const ids = (state.dividers || []).map(split => split.before + "/" + split.after)
        if (JSON.stringify(ids) !== JSON.stringify(dividerIds)) dividerIds = ids
    }
    readonly property int iconCount: interfacePrefs.tab_icon_count === undefined ? 1 : interfacePrefs.tab_icon_count
    function coloredIcon(presentation, index) {
        const mode = interfacePrefs.tab_icon_color || "never"
        return IconPolicy.colored(mode, presentation.inFront, index)
    }
    function selectedIndex(items, id) {
        for (let i = 0; i < items.length; i++) if (items[i].id === id) return i
        return -1
    }
    function selectedTerminal(pane) {
        const i = backend.paneIds.indexOf(pane || root.state.pane)
        const card = paneRepeater.itemAt(i)
        return card ? card.terminal : null
    }
    function find() { findBar.terminal = selectedTerminal(); findBar.open() }
    FindBar { id: findBar; anchors.top: parent.top; anchors.right: parent.right; z: 100; onClosed: root.restoreTerminal() }
    readonly property bool panelOpen: serverPopup.visible || serverConfirm.visible || pairDialog.visible || directoryPopup.visible || activityPopup.visible || settingsPopup.visible || commandPopup.visible || shortcutPopup.visible || findBar.visible || workspaceUI.modalOpen || hostDialogs.modalOpen || hostSwitcher.visible || (root.width < 960 && hostDrawer.visible)
    function restoreTerminal() {
        if (root.panelOpen) return
        const term = selectedTerminal(); if (term) term.forceActiveFocus()
    }
    function terminalAction(term, id) {
        switch (id) {
        case "pane.copy": if (term) term.copy(); break
        case "pane.paste": if (term) term.paste(); break
        case "pane.select_all": if (term) term.selectAll(); break
        case "pane.copy_id": if (term) term.copyId(); break
        case "pane.clear": if (term) term.clear(); break
        case "pane.reset": if (term) term.reset(); break
        case "pane.reconnect": if (term) term.reconnect(); break
        default: return false
        }
        return true
    }
    function action(id, args, origin) {
        const term = selectedTerminal()
        if (terminalAction(term, id)) return
        switch (id) {
        case "client.palette.toggle": commandPopup.visible ? commandPopup.close() : commandPopup.open(); break
        case "client.activity":
            if (args.view) backend.activityAction("view", {view: args.view})
            activityPopup.visible ? activityPopup.close() : activityPopup.open()
            break
        case "client.server.settings": serverPopup.open(); break;
        case "client.pair_device": backend.serverAction("pair"); break;
        case "client.server.stop": backend.serverAction("prepare_stop", ++root.serverRequest); break;
        case "client.server.setup": backend.serverAction("prepare_setup", ++root.serverRequest); break;
        case "client.settings": settingsPopup.open(); break
        case "client.full_screen.toggle": root.visibility === Window.FullScreen ? root.showNormal() : root.showFullScreen(); break
        case "client.font.increase": backend.savePreferences({font_size: Math.min(48, (preferences.font_size || 15) + 1)}); break
        case "client.font.decrease": backend.savePreferences({font_size: Math.max(8, (preferences.font_size || 15) - 1)}); break
        case "client.font.reset": backend.savePreferences({font_size: 15}); break
        case "client.zoom.in": backend.savePreferences({ui_scale: Math.min(1.5, Math.round((uiZoom + .1) * 10) / 10)}); break
        case "client.zoom.out": backend.savePreferences({ui_scale: Math.max(.7, Math.round((uiZoom - .1) * 10) / 10)}); break
        case "client.zoom.reset": backend.savePreferences({ui_scale: 1}); break
        case "session.refresh": backend.refresh(); break
        case "session.switch": hostSwitcher.scope = "sessions"; hostSwitcher.open(); break
        case "client.host.switch": hostSwitcher.scope = "hosts"; hostSwitcher.open(); break
        case "client.tab.switch": hostSwitcher.scope = "tabs"; hostSwitcher.open(); break
        case "client.host.add": hostDialogs.openAdd(); break
        case "client.host.edit": hostDialogs.openEdit(state.host); break
        case "client.host.remove": hostDialogs.openRemove(state.host); break
        case "client.sidebar.toggle": backend.hostAction("sidebar", {open: !backend.hosts.sidebar_open}); break
        case "client.history.back": backend.hostAction("history", {direction: -1}); break
        case "client.history.forward": backend.hostAction("history", {direction: 1}); break
        case "client.tab.next_global": backend.hostAction("cycle_tab", {direction: 1}); break
        case "client.tab.previous_global": backend.hostAction("cycle_tab", {direction: -1}); break
        case "session.next": backend.hostAction("cycle_session", {direction: 1}); break
        case "session.previous": backend.hostAction("cycle_session", {direction: -1}); break
        case "client.tab.next": backend.hostAction("tab_step", {direction: 1}); break
        case "client.tab.previous": backend.hostAction("tab_step", {direction: -1}); break
        case "client.tab.goto": backend.hostAction("tab_goto", {index: args.index}); break
        case "session.select": backend.hostAction("navigate", {session: args.session_id, window: args.window_id || ""}); break
        case "pane.actions": workspaceUI.openPaneMenu(root.width / 2, 52, state.pane); break
        case "pane.insert_path":
        case "pane.go_to_directory": backend.fileAction("directory.open", {go: id === "pane.go_to_directory"}, {host: state.host, epoch: state.epoch, session: state.session, window: state.window, pane: state.pane}); break
        case "client.search": find(); break
        case "client.find.next": if (term) term.searchStep(false); break
        case "client.find.previous": if (term) term.searchStep(true); break
        case "client.prompt.previous": if (term) term.promptStep(true); break
        case "client.prompt.next": if (term) term.promptStep(false); break
        case "client.scroll": if (term) term.scroll(args.lines); break
        default:
            const spec = ((root.state.keymap || {}).actions || []).find(row => row.id === id)
            if (spec && spec.handler === "workspace") workspaceUI.run(id, args)
            else backend.performAction(id, args, origin)
        }
    }
    ActivityModel {
        id: activityModel
        objectName: "activityModel"
        inventory: backend.activityState
        onActivate: target => {
            backend.hostAction("navigate", {session: target.session, window: target.window}, target)
            activityPopup.close()
        }
    }
    Activity {
        id: activityPopup
        objectName: "activityDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        activity: activityModel
        iconCount: root.iconCount
        colorMode: root.interfacePrefs.tab_icon_color || "never"
        onOpened: backend.activityAction("refresh")
        onClosed: root.restoreTerminal()
        onViewRequested: sessionView => backend.activityAction("view", {view: sessionView ? "session" : "priority"})
    }
    Timer { interval: 60000; repeat: true; running: activityPopup.visible; onTriggered: backend.activityAction("refresh") }
    Directory { id: directoryPopup; objectName: "directoryPopup"; parent: Overlay.overlay; frontend: backend; onClosed: Qt.callLater(root.restoreTerminal) }
    Commands { id: commandPopup; objectName: "commandsPopup"; parent: Overlay.overlay; actions: workspaceActions; onFinished: root.restoreTerminal() }
    Settings {
        id: settingsPopup
        objectName: "settingsPopup"
        parent: Overlay.overlay
        frontend: backend
        onServerRequested: { close(); serverPopup.open() }
        onShortcutsRequested: { close(); shortcutPopup.open() }
        onFullscreenRequested: root.visibility === Window.FullScreen ? root.showNormal() : root.showFullScreen()
        onFinished: root.restoreTerminal()
    }
    Popup {
        id: serverPopup; objectName: "serverPopup"; parent: Overlay.overlay
        width: Math.min(620, root.width-24); height: Math.min(650, root.height-24)
        anchors.centerIn: parent; modal: true; focus: true
        onOpened: backend.serverAction("page_open")
        onClosed: { backend.serverAction("page_close"); root.restoreTerminal() }
        contentItem: ScrollView {
            id: serverScroll
            clip: true; contentWidth: availableWidth; contentHeight: serverPage.implicitHeight
            ServerSettings {
                id: serverPage
                height: implicitHeight
                width: serverScroll.availableWidth
                serverState: backend.state.server || ({})
                onReadRequested: backend.serverAction("read")
                onPrepareRequested: kind => backend.serverAction("prepare_"+kind, ++root.serverRequest)
                onConnectRequested: backend.hostAction("connect")
                onEditEndpointRequested: endpoint => { serverPopup.close(); hostDialogs.openEdit(backend.state.host, endpoint) }
                onPairRequested: backend.serverAction("pair")
                onCopyRequested: text => backend.serverAction("copy", 0, text)
            }
        }
    }
    property double serverRequest: 0
    SSHSetup {
        id: serverConfirm; parent: Overlay.overlay
        confirmation: backend.state.server ? backend.state.server.confirmation || ({}) : ({})
        onConfirmRequested: requestId => backend.serverAction("confirm", requestId)
        onCancelRequested: backend.serverAction("cancel")
    }
    PairDevice {
        id: pairDialog; parent: Overlay.overlay
        pair: backend.state.server ? backend.state.server.pair : null
        onCopyRequested: text => backend.serverAction("copy", 0, text)
        onPairClosed: backend.serverAction("pair_close")
    }
    Connections {
        target: backend
        function onStateChanged() {
            const facts = backend.state.server || ({})
            if (facts.confirmation && facts.confirmation.request_id) { if (!serverConfirm.visible) serverConfirm.open() }
            else if (serverConfirm.visible) serverConfirm.close()
            if (facts.pair && facts.pair.address) { if (!pairDialog.visible) pairDialog.open() }
            else if (pairDialog.visible) pairDialog.close()
        }
    }
    ShortcutSettings { id: shortcutPopup; objectName: "shortcutPopup"; parent: Overlay.overlay; actions: workspaceActions; frontend: backend; onFinished: root.restoreTerminal() }
    WorkspaceActions {
        id: workspaceUI
        objectName: "workspaceUI"
        anchors.fill: parent
        z: 200
        frontend: backend
        canvasWidth: canvas.width
        canvasHeight: canvas.height
        onFinished: Qt.callLater(root.restoreTerminal)
        onTerminalAction: (id, captured) => {
            if (captured.host !== root.state.host || captured.epoch !== root.state.epoch || captured.session !== root.state.session || captured.window !== root.state.window) return
            const term = selectedTerminal(captured.pane)
            if (!term) return
            root.terminalAction(term, id)
        }
    }
    Binding { target: workspaceActions; property: "searching"; value: findBar.visible }
    Binding { target: workspaceActions; property: "modalOpen"; value: root.panelOpen }
    Connections { target: workspaceActions; function onRequested(id, args, origin) { root.action(id, args, origin) } }
    HostDialogs { id: hostDialogs; frontend: backend; onFinished: Qt.callLater(root.restoreTerminal) }
    Switcher { id: hostSwitcher; objectName: "hostSwitcher"; parent: Overlay.overlay; frontend: backend; onFinished: Qt.callLater(root.restoreTerminal) }
    Drawer {
        id: hostDrawer
        objectName: "hostDrawer"
        width: Math.min(280, root.width * .7)
        height: root.height - y
        y: toolbar.height
        edge: Qt.LeftEdge
        modal: root.width < 960
        dim: false
        interactive: root.width < 960
        closePolicy: root.width < 960 ? Popup.CloseOnEscape | Popup.CloseOnPressOutside : Popup.NoAutoClose
        Binding { target: hostDrawer; property: "visible"; value: Boolean(backend.hosts.sidebar_open) }
        onClosed: { if (backend.hosts.sidebar_open) backend.hostAction("sidebar", {open: false}); Qt.callLater(root.restoreTerminal) }
        HostSidebar {
            anchors.fill: parent
            frontend: backend
            onAddHost: hostDialogs.openAdd()
            onEditHost: id => hostDialogs.openEdit(id)
            onRemoveHost: id => hostDialogs.openRemove(id)
            onNavigated: if (root.width < 960) backend.hostAction("sidebar", {open: false})
        }
    }
    property string selectedPane: state.pane || ""
    onSelectedPaneChanged: { if (findBar.visible) findBar.close(); Qt.callLater(restoreTerminal) }

    Item {
        z: 500
        width: parent.width
        height: 5
        HoverHandler { id: toolbarEdge }
    }
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        ToolBar {
            id: toolbar
            property bool revealed: root.interfacePrefs.toolbar !== "hidden" && (root.interfacePrefs.toolbar !== "on_hover" || toolbarEdge.hovered || toolbarHover.hovered)
            visible: revealed
            HoverHandler { id: toolbarHover }
            Layout.fillWidth: true
            Layout.preferredHeight: revealed ? 52 * root.uiZoom : 0
            RowLayout {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                ToolButton {
                    objectName: "sidebarButton"
                    text: "☰"
                    Accessible.name: qsTr("Toggle hosts and sessions")
                    onClicked: workspaceActions.run("client.sidebar.toggle")
                }
                ComboBox {
                    id: sessionPicker
                    visible: root.width >= 800
                    objectName: "sessions"
                    Layout.fillWidth: true
                    Layout.preferredWidth: 180
                    Layout.maximumWidth: 240
                    Layout.minimumWidth: 110
                    model: root.state.sessions || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.session)
                    displayText: currentIndex >= 0 && model[currentIndex] ? model[currentIndex].label + ((model[currentIndex].activity || {}).word ? " · " + model[currentIndex].activity.word : "") : ""
                    Accessible.name: "Session"
                    ToolTip.text: "Session"
                    ToolTip.visible: hovered
                    onActivated: { backend.selectSession(model[index].id); root.restoreTerminal() }
                }
                ComboBox {
                    id: tabsPicker
                    objectName: "tabs"
                    visible: !root.preferences.vertical_tabs
                    Layout.fillWidth: Boolean(root.interfacePrefs.tabs_fill_available_space)
                    Layout.preferredWidth: 200
                    Layout.minimumWidth: 110
                    model: root.state.windows || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.window)
                    Accessible.name: "Tab"
                    ToolTip.text: "Tab"
                    ToolTip.visible: hovered
                    property var currentPresentation: currentIndex >= 0 && model[currentIndex] ? model[currentIndex].activity || ({}) : ({})
                    ProgressBar {
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom; anchors.margins: 2
                        height: 3
                        property var progress: tabsPicker.currentPresentation.progress
                        visible: !!progress && !tabsPicker.currentPresentation.muted
                        from: 0; to: 100
                        value: progress && progress.percent !== null ? progress.percent : 0
                        indeterminate: !!progress && progress.percent === null
                        palette.highlight: progress && progress.kind === "error" ? "#ff8a8a" : progress && progress.kind === "paused" ? "#f5d48a" : root.palette.highlight
                    }
                    contentItem: RowLayout {
                        spacing: 4
                        Repeater {
                            model: (tabsPicker.currentPresentation.icons || []).slice(0, root.iconCount)
                            ProgramIcon { required property var modelData; required property int index; icon: modelData; colored: root.coloredIcon(tabsPicker.currentPresentation, index) }
                        }
                        Label { Layout.fillWidth: true; text: tabsPicker.displayText; textFormat: Text.PlainText; elide: Text.ElideRight; verticalAlignment: Text.AlignVCenter }
                        Label { text: tabsPicker.currentPresentation.muted ? "" : tabsPicker.currentPresentation.word || ""; textFormat: Text.PlainText }
                    }
                    delegate: ItemDelegate {
                        required property var modelData
                        required property int index
                        property var presentation: modelData.activity || ({})
                        width: tabsPicker.width
                        highlighted: tabsPicker.highlightedIndex === index
                        contentItem: RowLayout {
                            Repeater {
                                model: (presentation.icons || []).slice(0, root.iconCount)
                                ProgramIcon { required property var modelData; required property int index; icon: modelData; colored: root.coloredIcon(presentation, index) }
                            }
                            Label { Layout.fillWidth: true; text: modelData.label; textFormat: Text.PlainText; elide: Text.ElideRight }
                            Label { text: presentation.muted ? "" : presentation.word || ""; textFormat: Text.PlainText }
                        }
                    }
                    onActivated: { backend.selectWindow(model[index].id); root.restoreTerminal() }
                }
                ComboBox {
                    objectName: "panes"
                    visible: root.width >= 900
                    Layout.fillWidth: true
                    Layout.preferredWidth: 150
                    Layout.maximumWidth: 200
                    Layout.minimumWidth: 90
                    model: root.state.panes || []
                    textRole: "label"
                    currentIndex: root.selectedIndex(model, root.state.pane)
                    Accessible.name: "Pane"
                    ToolTip.text: "Pane"
                    ToolTip.visible: hovered
                    onActivated: { backend.selectPane(model[index].id); root.restoreTerminal() }
                }
                ToolButton {
                    visible: root.width >= 1000
                    objectName: "copyButton"
                    text: "Copy"
                    Accessible.name: "Copy selection"
                    ToolTip.text: "Copy" + (workspaceActions.hint("pane.copy") ? " · " + workspaceActions.hint("pane.copy") : "")
                    ToolTip.visible: hovered
                    onClicked: { workspaceActions.run("pane.copy"); root.restoreTerminal() }
                }
                ToolButton {
                    visible: root.width >= 1000
                    objectName: "pasteButton"
                    text: "Paste"
                    onClicked: workspaceActions.run("pane.paste")
                }
                Label {
                    visible: ((root.preferences.terminal || {}).key_sequence_hints !== false && workspaceActions.pending.length > 0) || workspaceActions.activeModes.length > 0
                    text: ((root.preferences.terminal || {}).key_sequence_hints !== false ? workspaceActions.pending : "") || workspaceActions.activeModes.join(" · ")
                    Layout.maximumWidth: 150
                    elide: Text.ElideRight
                }
                ToolButton {
                    objectName: "activityButton"
                    text: activityModel.chipCount ? qsTr("Activity · %1").arg(activityModel.chipCount) : qsTr("Activity")
                    Accessible.name: qsTr("Activity")
                    ToolTip.text: qsTr("Activity") + (workspaceActions.hint("client.activity") ? " · " + workspaceActions.hint("client.activity") : "")
                    ToolTip.visible: hovered
                    onClicked: workspaceActions.run("client.activity")
                }
                ToolButton {
                    objectName: "commandsButton"
                    text: "⌘"
                    Accessible.name: "Commands"
                    ToolTip.text: "Commands" + (workspaceActions.hint("client.palette.toggle") ? " · " + workspaceActions.hint("client.palette.toggle") : "")
                    ToolTip.visible: hovered
                    onClicked: workspaceActions.run("client.palette.toggle")
                }
                ToolButton {
                    objectName: "settingsButton"
                    text: "⚙"
                    Accessible.name: "Settings"
                    onClicked: workspaceActions.run("client.settings")
                }
                ToolButton {
                    objectName: "workspaceButton"
                    text: "+"
                    Accessible.name: qsTr("Session, tab and pane actions")
                    onClicked: workspaceUI.openMenu(this)
                }
                ToolButton {
                    id: more
                    objectName: "moreButton"
                    text: "⋯"
                    Accessible.name: "Terminal actions"
                    onClicked: actions.open()
                    Menu {
                        id: actions
                        objectName: "actionsMenu"
                        y: more.height
                        MenuItem {
                            objectName: "selectAllButton"
                            text: "Select all terminal text"
                            onTriggered: { workspaceActions.run("pane.select_all"); root.restoreTerminal() }
                        }
                        MenuItem { objectName: "previousPromptButton"; text: "Previous prompt"; onTriggered: { workspaceActions.run("client.prompt.previous"); root.restoreTerminal() } }
                        MenuItem { objectName: "nextPromptButton"; text: "Next prompt"; onTriggered: { workspaceActions.run("client.prompt.next"); root.restoreTerminal() } }
                        MenuItem { text: "Refresh sessions"; onTriggered: workspaceActions.run("session.refresh") }
                    }
                }
            }
        }
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: warning.visible ? warning.implicitHeight + 16 : 0
            color: "#332b1d"
            visible: implicitHeight > 0
            RowLayout {
                id: warning
                anchors.fill: parent
                anchors.margins: 8
                visible: backend.problem.length > 0 || !root.state.connected || !(root.state.terminal && root.state.terminal.connected)
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: "#f0c987"
                    text: backend.problem || (!root.state.connected ? (root.state.status || "Connecting…") : (root.state.pane ? "Terminal input paused" : "Choose a terminal pane"))
                }
                ToolButton {
                    text: backend.problem ? qsTr("Dismiss") : !root.state.host ? qsTr("Add host") :
                          !root.state.connected ? (root.hostInfo.enabled ? qsTr("Retry") : qsTr("Connect")) : qsTr("Refresh")
                    onClicked: {
                        if (backend.problem) backend.dismissProblem()
                        else if (!root.state.host) hostDialogs.openAdd()
                        else if (!root.state.connected) backend.hostAction("connect", {})
                        else backend.refresh()
                    }
                }
            }
        }
        UploadStatus { frontend: backend; Layout.fillWidth: true; Layout.leftMargin: 8; Layout.rightMargin: 8 }
        Item {
            id: canvas
            objectName: "terminalCanvas"
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: root.preferences.pane_density === "compact" ? 0 : 8
            Layout.leftMargin: root.width >= 960 && backend.hosts.sidebar_open ? hostDrawer.width + (root.preferences.pane_density === "compact" ? 0 : 8) : (root.preferences.pane_density === "compact" ? 0 : 8)
            clip: true
            Label {
                anchors.centerIn: parent
                visible: backend.paneIds.length === 0
                text: root.state.connected ? "Choose a terminal session" : "Connecting…"
                color: "#a2b0c4"
            }
            Repeater {
                model: root.dividerIds
                delegate: Item {
                    id: dividerHandle
                    required property string modelData
                    property var split: (root.state.dividers || []).find(row => row.before + "/" + row.after === modelData)
                    property var rect: split ? split.rect : [0, 0, 0, 0]
                    x: Math.round((rect[0] + (split && split.horizontal ? rect[2] * split.ratio : 0)) * canvas.width) - (split && split.horizontal ? 4 : 0)
                    y: Math.round((rect[1] + (split && !split.horizontal ? rect[3] * split.ratio : 0)) * canvas.height) - (split && !split.horizontal ? 4 : 0)
                    width: split && split.horizontal ? 8 : rect[2] * canvas.width
                    height: split && !split.horizontal ? 8 : rect[3] * canvas.height
                    z: 1000
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: dividerHandle.split && dividerHandle.split.horizontal ? Qt.SplitHCursor : Qt.SplitVCursor
                        property var captured: null
                        property var initial: null
                        onPressed: { captured = workspaceUI.capture(); initial = dividerHandle.split }
                        onPositionChanged: mouse => {
                            if (!pressed || !initial) return
                            const point = mapToItem(canvas, mouse.x, mouse.y)
                            const r = initial.rect
                            const ratio = initial.horizontal ? (point.x / canvas.width - r[0]) / r[2] : (point.y / canvas.height - r[1]) / r[3]
                            workspaceUI.send("pane.resize_divider", {before: initial.before, after: initial.after, ratio: ratio}, captured)
                        }
                        onReleased: { captured = null; initial = null }
                        onCanceled: { captured = null; initial = null }
                    }
                }
            }
            Repeater {
                id: paneRepeater
                model: backend.paneIds
                delegate: Rectangle {
                    id: card
                    required property string modelData
                    required property int index
                    property alias terminal: terminalView
                    property var pane: terminalView.paneState
                    property var placement: pane.rect || [0, 0, 1, 1]
                    x: Math.round(placement[0] * canvas.width) + 2
                    y: Math.round(placement[1] * canvas.height) + 2
                    width: Math.max(0, Math.round(placement[2] * canvas.width) - 4)
                    height: Math.max(0, Math.round(placement[3] * canvas.height) - 4)
                    z: index + 1
                    color: root.chrome[0]
                    border.width: (root.interfacePrefs.highlight_active_pane ?? root.preferences.pane_density !== "compact") && backend.paneIds.length > 1 ? 1 : 0
                    border.color: root.selectedPane === modelData ? root.chrome[4] : root.chrome[1]
                    clip: true
                    HoverHandler { id: paneHover }
                    PaneHeader {
                        id: heading
                        width: parent.width
                        property bool floating: root.preferences.pane_header === "on_hover"
                        height: root.preferences.pane_header === "hidden" ? 0 : root.preferences.pane_header === "always" || floating || (root.preferences.pane_density !== "compact" && (backend.paneIds.length > 1 || root.state.zoomed)) ? implicitHeight : 0
                        z: 2
                        opacity: floating && !paneHover.hovered ? 0 : ((root.interfacePrefs.fade_inactive_pane_headers && !focused) || (root.interfacePrefs.fade_pane_headers_in_inactive_window && !root.active)) ? .55 : 1
                        enabled: opacity > 0
                        visible: height > 0
                        iconVisible: root.iconCount > 0
                        iconColored: root.interfacePrefs.tab_icon_color === "always" || root.interfacePrefs.tab_icon_color === "active_tab" || (root.interfacePrefs.tab_icon_color === "active_pane" && root.selectedPane === card.modelData)
                        pane: card.pane
                        workspace: workspaceUI
                        focused: root.selectedPane === card.modelData
                    }
                    TerminalView {
                        id: terminalView
                        objectName: "terminal"
                        x: 4
                        y: heading.floating ? 0 : heading.height
                        width: Math.max(0, parent.width - 8)
                        height: Math.max(0, parent.height - y)
                        preferences: root.preferences
                        frontend: backend
                        actions: workspaceActions
                        paneId: card.modelData
                        onContextMenuRequested: (x, y) => {
                            const point = terminalView.mapToItem(workspaceUI, x, y)
                            workspaceUI.openPaneMenu(point.x, point.y, card.modelData)
                        }
                        Accessible.name: "Terminal " + (card.pane.label || "")
                        Component.onCompleted: if (root.selectedPane === card.modelData) root.restoreTerminal()
                    }
                }
            }
        }
    }
}
