#include "activity.h"
#include <QSignalSpy>
#include <QTest>

class ActivityTests : public QObject {
    Q_OBJECT
    QVariantMap row(QString host, QString session, QString window, int rank) {
        return {{"key",host+":"+session+":"+window},{"host",host},{"epoch",7},{"session",session},{"window",window},{"pane","p"},
            {"hostLabel","Same host"},{"sessionLabel","Same session"},{"label",window},{"rank",rank},{"groupName",rank==0?"Needs you":rank==1?"Error":"Working"},
            {"state",rank==0?"blocked":rank==1?"error":"working"},{"listed",true},{"eligible",rank<2},{"inFront",false}};
    }
private slots:
    void liveWakesAndViewsPreserveCapturedSelection() {
        ActivityModel model;
        const auto a=row("h","s","a",3), b=row("h","s","b",1);
        model.setInventory({{"rows",QVariantList{a,b}}});
        QCOMPARE(model.rows().first().toMap().value("window").toString(),QString("b"));
        model.setSelectedKey(a.value("key").toString());
        auto updated=a;updated["rank"]=0;updated["state"]="blocked";
        model.setInventory({{"rows",QVariantList{updated,b}}});
        QCOMPARE(model.selectedKey(),a.value("key").toString());
        model.setInventory({{"view","session"},{"rows",QVariantList{updated,b}}});
        QVERIFY(model.sessionView());
        QCOMPARE(model.selectedKey(),a.value("key").toString());
        QSignalSpy activated(&model,&ActivityModel::activate);
        model.activateSelected();
        QCOMPARE(activated.count(),1);
        const auto target=activated.first()[0].toMap();
        QCOMPARE(target.value("host").toString(),QString("h"));
        QCOMPARE(target.value("epoch").toInt(),7);
        QCOMPARE(target.value("window").toString(),QString("a"));
    }
    void filteringKeepsMatchingIdentityAndRemovedSelectionCannotActivate() {
        ActivityModel model;
        const auto a=row("h","s","alpha",0),b=row("h","s","beta",1);
        model.setInventory({{"rows",QVariantList{a,b}}});
        model.setSelectedKey(b.value("key").toString());
        model.setFilter("Same");
        QCOMPARE(model.selectedKey(),b.value("key").toString());
        model.setFilter("alpha");
        QCOMPARE(model.selectedKey(),a.value("key").toString());
        model.setFilter("absent");
        QVERIFY(model.rows().isEmpty());
        QSignalSpy activated(&model,&ActivityModel::activate);
        model.activateSelected();QCOMPARE(activated.count(),0);
        model.setFilter("");model.step(1);
        QCOMPARE(model.selectedKey(),b.value("key").toString());
        model.setInventory({{"rows",QVariantList{a}}});
        QCOMPARE(model.selectedKey(),a.value("key").toString());
    }
    void duplicateNamesHaveDistinctSessionGroupsAndPriority() {
        ActivityModel model;
        const auto a=row("one","s","a",3),b=row("two","s","b",0),c=row("one","s","c",1);
        model.setInventory({{"view","session"},{"rows",QVariantList{a,b,c}}});
        const auto rows=model.rows();
        QCOMPARE(rows[0].toMap().value("window").toString(),QString("b"));
        QCOMPARE(rows[1].toMap().value("window").toString(),QString("c"));
        QCOMPARE(rows[2].toMap().value("window").toString(),QString("a"));
        QVERIFY(rows[0].toMap().value("showGroup").toBool());
        QVERIFY(rows[1].toMap().value("showGroup").toBool());
        QVERIFY(!rows[2].toMap().value("showGroup").toBool());
    }
    void inventoryPolicyDoesNotCountDisplayedOrMutedRows() {
        ActivityModel model;
        auto a=row("h","s","a",0),b=row("h","s","b",1),c=row("h","s","c",0);
        a["inFront"]=true;b["listed"]=false;b["eligible"]=false;
        model.setInventory({{"rows",QVariantList{a,b,c}}});
        QCOMPARE(model.rows().size(),2);QCOMPARE(model.chipCount(),1);
    }
};
QTEST_GUILESS_MAIN(ActivityTests)
#include "activity_test.moc"
