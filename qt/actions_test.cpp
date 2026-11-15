#include "actions.h"
#include <QJsonArray>
#include <QSignalSpy>
#include <QTest>

class ActionTests : public QObject {
    Q_OBJECT
private:
    QJsonObject snapshot(const QJsonArray &bindings) {
        return {{"epoch",1},{"session","s"},{"pane","p"},{"keymap",QJsonObject{
            {"generation",1},{"revision",1},{"bindings",bindings},
            {"actions",QJsonArray{QJsonObject{{"id","pane.copy"},{"enabled",true},{"kind","client"},{"hint","Ctrl Shift C"}},
                                   QJsonObject{{"id","pane.paste"},{"enabled",true},{"kind","client"}}}}}}};
    }
    QJsonObject chord(QString key, bool ctrl = false, bool shift = false) {
        return {{"kind","character"},{"key",key},{"ctrl",ctrl},{"alt",false},{"shift",shift},{"super",false}};
    }
    QJsonObject binding(QJsonArray chords, QString action) {
        return {{"action",action},{"chords",chords},{"keys",QJsonArray{"ctrl+b","c"}}};
    }
private slots:
    void removableEffectiveBindingsConsumeBothEdges() {
        Actions actions;
        actions.setSnapshot(snapshot({binding({chord("c",true,true)},"pane.copy")}));
        QSignalSpy requests(&actions, &Actions::requested);
        QKeyEvent press(QEvent::KeyPress,Qt::Key_C,Qt::ControlModifier|Qt::ShiftModifier);
        QKeyEvent release(QEvent::KeyRelease,Qt::Key_C,Qt::ControlModifier|Qt::ShiftModifier);
        QVERIFY(actions.handleKey(&press));
        QCOMPARE(requests.count(),1);
        QCOMPARE(requests.first()[0].toString(),QString("pane.copy"));
        QVERIFY(actions.handleKey(&release));
        actions.setSnapshot(snapshot({}));
        QVERIFY(!actions.handleKey(&press));
        QVERIFY(!actions.handleKey(&release));
    }
    void sequencesKeepTheirPrefixAndEscapeLocal() {
        Actions actions;
        actions.setSnapshot(snapshot({binding({chord("b",true),chord("c")},"pane.copy")}));
        QSignalSpy requests(&actions, &Actions::requested);
        QKeyEvent prefix(QEvent::KeyPress,Qt::Key_B,Qt::ControlModifier);
        QKeyEvent release(QEvent::KeyRelease,Qt::Key_B,Qt::ControlModifier);
        QKeyEvent escape(QEvent::KeyPress,Qt::Key_Escape,Qt::NoModifier);
        QKeyEvent suffix(QEvent::KeyPress,Qt::Key_C,Qt::NoModifier,"c");
        QVERIFY(actions.handleKey(&prefix));
        QVERIFY(actions.handleKey(&release));
        QVERIFY(!actions.pending().isEmpty());
        QVERIFY(actions.handleKey(&escape));
        QVERIFY(actions.pending().isEmpty());
        QVERIFY(!actions.handleKey(&suffix));
        QVERIFY(actions.handleKey(&prefix));
        QVERIFY(actions.handleKey(&suffix));
        QCOMPARE(requests.count(),1);
    }
    void modalEditingAndFocusChangesCannotLeakConsumedReleases() {
        Actions actions;
        actions.setSnapshot(snapshot({binding({chord("b",true),chord("c")},"pane.copy")}));
        QKeyEvent prefix(QEvent::KeyPress,Qt::Key_B,Qt::ControlModifier);
        QKeyEvent release(QEvent::KeyRelease,Qt::Key_B,Qt::ControlModifier);
        QKeyEvent suffix(QEvent::KeyPress,Qt::Key_C,Qt::NoModifier,"c");
        QVERIFY(actions.handleKey(&prefix));
        actions.setModalOpen(true);
        QVERIFY(actions.pending().isEmpty());
        QVERIFY(actions.handleKey(&release,true));
        QVERIFY(!actions.handleKey(&suffix,true));
    }
    void exclusiveAndOneShotModesStayInTheClient() {
        Actions actions;
        auto state = snapshot({});
        auto map = state["keymap"].toObject();
        auto commands = map["actions"].toArray();
        commands.append(QJsonObject{{"id","client.mode.enter"},{"enabled",true},{"kind","client"}});
        commands.append(QJsonObject{{"id","client.mode.exit_all"},{"enabled",true},{"kind","client"}});
        auto local = binding({chord("x")},"pane.copy");
        local["mode"] = "resize";
        map["actions"] = commands;
        map["bindings"] = QJsonArray{local, binding({chord("c",true,true)},"pane.copy")};
        map["modes"] = QJsonArray{QJsonObject{{"name","resize"},{"exclusive",true}}};
        state["keymap"] = map;
        actions.setSnapshot(state);
        actions.run("client.mode.enter",{{"name","resize"},{"once",true}});
        QCOMPARE(actions.activeModes(),QStringList{"resize"});
        QKeyEvent unused(QEvent::KeyPress,Qt::Key_Z,Qt::NoModifier,"z");
        QVERIFY(actions.handleKey(&unused));
        QSignalSpy requests(&actions, &Actions::requested);
        QKeyEvent blocked(QEvent::KeyPress,Qt::Key_C,Qt::ControlModifier|Qt::ShiftModifier);
        QVERIFY(actions.handleKey(&blocked));
        QCOMPARE(requests.count(),0);
        QKeyEvent run(QEvent::KeyPress,Qt::Key_X,Qt::NoModifier,"x");
        QVERIFY(actions.handleKey(&run));
        QCOMPARE(requests.count(),1);
        QVERIFY(actions.activeModes().isEmpty());
        QVERIFY(!actions.handleKey(&unused));
        actions.run("client.mode.enter",{{"name","resize"}});
        state["session"] = "other-session";
        actions.setSnapshot(state);
        QVERIFY(actions.activeModes().isEmpty());
    }

};
QTEST_GUILESS_MAIN(ActionTests)
#include "actions_test.moc"
