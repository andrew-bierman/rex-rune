import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
ColumnLayout {
    id: page
    required property var controller
    property string query: ""
    property var values:controller.values
    property var interfaceValues:values.interface || ({})
    function choices(names) { return names.map(name => ({value:name[0],title:name[1]})) }
    spacing:10
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Chrome palette"); path:"chrome_theme"; value:page.values.chrome_theme; choices:page.choices([["midnight",qsTr("Midnight")],["rose_pine",qsTr("Rosé Pine")],["rose_pine_moon",qsTr("Rosé Pine Moon")],["gruvbox_dark",qsTr("Gruvbox Dark")]]) }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Interface zoom (%)"); path:"ui_scale"; kind:"number"; divisor:100; minimum:70; maximum:150; step:10; value:page.values.ui_scale }
    Label { text:qsTr("Terminal font size is independent of interface zoom. Zoom is limited by the window's 640 × 480 layout floor."); Layout.fillWidth:true; wrapMode:Text.Wrap; opacity:.7 }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Pane density"); path:"pane_density"; value:page.values.pane_density; choices:page.choices([["comfortable",qsTr("Comfortable")],["compact",qsTr("Compact")]]) }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Pane headers"); path:"pane_header"; value:page.values.pane_header; choices:page.choices([["auto",qsTr("Follow density")],["always",qsTr("Always")],["on_hover",qsTr("On hover")],["hidden",qsTr("Hidden")]]) }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Horizontal pane padding"); path:"pane_padding"; kind:"number"; maximum:32; value:page.values.pane_padding }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Vertical pane padding"); path:"interface.pane_padding_vertical"; kind:"number"; maximum:32; value:page.interfaceValues.pane_padding_vertical === undefined ? page.values.pane_padding : page.interfaceValues.pane_padding_vertical }
    PreferenceRow { controller:page.controller; query:page.query; title:qsTr("Active pane outline"); path:"interface.highlight_active_pane"; value:page.interfaceValues.highlight_active_pane === undefined ? null : page.interfaceValues.highlight_active_pane; choices:page.choices([[null,qsTr("Follow density")],[true,qsTr("On")],[false,qsTr("Off")]]) }
    Repeater {
        model:[{key:"fade_inactive_pane_headers",title:qsTr("Fade inactive pane headers")},{key:"fade_pane_headers_in_inactive_window",title:qsTr("Fade headers in inactive window")},{key:"tabs_fill_available_space",title:qsTr("Tabs fill available space")}]
        delegate:PreferenceRow { required property var modelData;controller:page.controller;query:page.query;title:modelData.title;path:"interface."+modelData.key;kind:"toggle";value:page.interfaceValues[modelData.key] }
    }
    PreferenceRow { controller:page.controller;query:page.query;title:qsTr("Program icons per tab");path:"interface.tab_icon_count";kind:"number";maximum:5;value:page.interfaceValues.tab_icon_count }
    PreferenceRow { controller:page.controller;query:page.query;title:qsTr("Program icon colour");path:"interface.tab_icon_color";value:page.interfaceValues.tab_icon_color;choices:page.choices([["never",qsTr("Never")],["always",qsTr("Always")],["active_pane",qsTr("Active pane")],["active_tab",qsTr("Active tab")]]) }
    PreferenceRow { controller:page.controller;query:page.query;title:qsTr("Toolbar visibility");path:"interface.toolbar";value:page.interfaceValues.toolbar;choices:page.choices([["always",qsTr("Always")],["on_hover",qsTr("On hover")],["hidden",qsTr("Hidden")]]) }
    PreferenceRow { controller:page.controller;query:page.query;title:qsTr("Generated tab and pane titles");path:"generated_titles";kind:"toggle";value:page.values.generated_titles }
    PreferenceRow { controller:page.controller;query:page.query;title:qsTr("Tabs in sidebar");path:"vertical_tabs";kind:"toggle";value:page.values.vertical_tabs }
}
