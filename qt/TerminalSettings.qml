import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import RexQt 1.0
ColumnLayout {
    id: page
    required property var controller
    property string query: ""
    property var values: controller.values
    property var terminal: values.terminal || ({})
    property var typography: terminal.font || ({})
    function choices(names) { return names.map(name => ({value:name[0],title:name[1]})) }
    function detailsVisible(words) { return !query || words.some(word => query.toLowerCase().includes(word)) }
    readonly property bool fontDetailsVisible: detailsVisible(["font", "typography", "fallback", "spacing", "line height", "bold", "italic"])
    readonly property bool cursorDetailsVisible: detailsVisible(["cursor"])
    readonly property bool selectionDetailsVisible: detailsVisible(["selection"])
    spacing: 10
    Label { visible:page.fontDetailsVisible; text: qsTr("Typography"); font.bold: true }
    TerminalPreview { visible:page.fontDetailsVisible; Layout.fillWidth: true; implicitHeight: Math.max(48, page.controller.metrics.height+12); preferences: page.values }
    RowLayout {
        visible: !page.query || ("font family fallback typography "+page.values.font_family).toLowerCase().includes(page.query.toLowerCase())
        Layout.fillWidth: true
        Label { text: qsTr("Font family"); Layout.fillWidth: true }
        Button { objectName: "chooseFont"; text: page.values.font_family || ""; onClicked: { picker.fallback=false; picker.open() } }
    }
    Repeater {
        model: page.values.font_fallbacks || []
        delegate: RowLayout {
            required property string modelData
            required property int index
            visible:page.fontDetailsVisible
            Layout.fillWidth: true
            Label { text: modelData; textFormat: Text.PlainText; Layout.fillWidth: true; elide: Text.ElideMiddle }
            Button { text: qsTr("Up"); enabled: index>0; onClicked: { let list=page.values.font_fallbacks.slice();let old=list[index-1];list[index-1]=list[index];list[index]=old;page.controller.setValue("font_fallbacks",list) } }
            Button { text: qsTr("Remove"); onClicked: { let list=page.values.font_fallbacks.slice();list.splice(index,1);page.controller.setValue("font_fallbacks",list) } }
        }
    }
    Button { visible:page.fontDetailsVisible; text: qsTr("Add fallback font…"); onClicked: { picker.fallback=true;picker.open() } }
    FontPicker { id: picker; controller: page.controller; onSelected: family => { if (fallback) { let list=(page.values.font_fallbacks || []).slice();if(!list.includes(family)) list.push(family);page.controller.setValue("font_fallbacks",list) } else page.controller.setValue("font_family",family) } }
    Label { visible:page.fontDetailsVisible; text: qsTr("Bundled IBM Plex Mono and Symbols Nerd Font follow your ordered fallbacks. Cell size: %1 × %2; resolved font: %3").arg(page.controller.metrics.width).arg(page.controller.metrics.height).arg(page.controller.metrics.family); textFormat: Text.PlainText; Layout.fillWidth: true; wrapMode: Text.Wrap; opacity: .7 }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Font size (points)"); path: "font_size"; kind: "number"; minimum:8; maximum:48; value:page.values.font_size }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Font style"); path: "terminal.font.style"; value:page.typography.style; choices:page.choices([["regular",qsTr("Regular")],["italic",qsTr("Italic")],["bold",qsTr("Bold")],["bold_italic",qsTr("Bold italic")]]) }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Line height adjustment (%)"); path: "terminal.font.line_height"; kind:"number"; minimum:-20; maximum:100; step:5; value:page.typography.line_height }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Character spacing"); path: "terminal.font.spacing"; kind:"number"; minimum:page.typography.spacing_unit==="percent"?-30:-3; maximum:page.typography.spacing_unit==="percent"?100:12; value:page.typography.spacing }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Spacing unit"); path:"terminal.font.spacing_unit"; value:page.typography.spacing_unit; choices:page.choices([["px",qsTr("Points")],["percent",qsTr("Percent")]]) }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Program bold and italic"); path:"terminal.font.bold_italic"; value:page.typography.bold_italic; choices:page.choices([["both",qsTr("Both")],["bold_only",qsTr("Bold only")],["italic_only",qsTr("Italic only")],["neither",qsTr("Neither")]]) }
    PreferenceRow { controller: page.controller; query: page.query; title: qsTr("Terminal theme"); path:"terminal_theme"; value:page.values.terminal_theme; choices:page.values._themes || [] }
    Label { visible:page.detailsVisible(["theme", "follow server"]); text: qsTr("Themes are declared to Rex for panes this client owns or nobody owns. Follow server clears a theme previously declared by this client."); Layout.fillWidth:true; wrapMode:Text.Wrap; opacity:.7 }
    Label { visible:page.cursorDetailsVisible; text: qsTr("Cursor"); font.bold:true }
    TerminalPreview { visible:page.cursorDetailsVisible; kind:"cursor"; preferences:page.values; Layout.fillWidth:true; implicitHeight:Math.max(48,page.controller.metrics.height+12) }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Cursor style"); path:"terminal.cursor_style"; value:page.terminal.cursor_style; choices:page.choices([["block",qsTr("Block")],["bar",qsTr("Bar")],["underline",qsTr("Underline")]]) }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Force cursor style"); path:"terminal.cursor_force"; kind:"toggle"; value:page.terminal.cursor_force }
    Label { visible:page.selectionDetailsVisible; text: qsTr("Selection"); font.bold:true }
    TerminalPreview { visible:page.selectionDetailsVisible; kind:"selection"; preferences:page.values; Layout.fillWidth:true; implicitHeight:Math.max(48,page.controller.metrics.height+12) }
    Repeater {
        model: [{key:"selection_foreground",title:qsTr("Selection text")},{key:"selection_background",title:qsTr("Selection background")}]
        delegate: PreferenceRow {
            required property var modelData
            controller:page.controller; query:page.query; title:modelData.title; path:"terminal."+modelData.key; value:page.terminal[modelData.key]
            choices:page.choices([["theme",qsTr("Theme")],["cell_foreground",qsTr("Cell foreground")],["cell_background",qsTr("Cell background")]])
        }
    }
    Repeater {
        model:[{key:"selection_rounded",title:qsTr("Rounded selection")},{key:"copy_on_selection",title:qsTr("Copy on selection")},{key:"primary_selection",title:qsTr("PRIMARY selection and middle click paste")},{key:"key_sequence_hints",title:qsTr("Show key sequence hints")},{key:"show_recent_paths",title:qsTr("Show recently used paths")}]
        delegate: PreferenceRow { required property var modelData; controller:page.controller; query:page.query; title:modelData.title; path:"terminal."+modelData.key; kind:"toggle"; value:page.terminal[modelData.key] }
    }
}
