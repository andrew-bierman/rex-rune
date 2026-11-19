#include "../notifications.h"
#include <QTest>
#include <QSignalSpy>
#include <QJsonDocument>
#include <QJsonArray>
#include <QFile>
#include <QFileInfo>
#include <QProcess>
#include <QDBusInterface>
#include <QElapsedTimer>

class NotificationServices : public QObject {
    Q_OBJECT
    QString root = qEnvironmentVariable("REX_NOTIFICATION_FIXTURE");
    QProcess daemon;
    QProcess restartedBus;
    QList<QJsonObject> calls(const QString &method) {
        QFile file(root + "/calls.jsonl"); if (!file.open(QIODevice::ReadOnly)) return {};
        QList<QJsonObject> result;
        for (const auto &line : file.readAll().split('\n')) {
            const auto row = QJsonDocument::fromJson(line).object();
            if (row["method"] == method) result.append(row);
        }
        return result;
    }
    void service(const QString &method, uint id = 0) {
        QDBusInterface control("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "dev.shuv.NotificationTest", QDBusConnection::sessionBus());
        control.setTimeout(1000);
        if (id) control.call(method, id); else control.call(method);
    }
    void startDaemon() {
        daemon.setStandardErrorFile(root + "/fake-service.log", QIODevice::Append);
        daemon.setStandardOutputFile(root + "/fake-service.log", QIODevice::Append);
        daemon.start("/usr/bin/python3", {qEnvironmentVariable("REX_NOTIFICATION_SCRIPT"), "--service", root});
        QVERIFY(daemon.waitForStarted());
        QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(root + "/service-ready"), 5000);
    }
    QJsonObject row(QString kind, bool news = true) {
        return {{"key","captured-tab"},{"host","private-host"},{"epoch",7},{"activityEpoch",4},{"session","owned-session"},{"window","owned-tab"},
                {"label","Owned tab"},{"hostLabel","Fixture"},{"sessionLabel","Test"},{"state",kind},{"newsState",kind},{"news",news},
                {"word",kind == "blocked" ? "Needs approval" : "Error"},{"message","<b>untrusted & text</b>"},{"seen",false},{"muted",false},{"eligible",true}};
    }
    QJsonObject state(const QJsonObject &r, bool sounds = false) {
        return {{"preferences",QJsonObject{{"notifications",QJsonObject{{"play_sounds",sounds}}}}},{"activity",QJsonObject{{"rows",QJsonArray{r}}}}};
    }
private slots:
    void initTestCase() {
        QVERIFY(!root.isEmpty());
        QVERIFY(qEnvironmentVariable("DBUS_SESSION_BUS_ADDRESS").contains(QFileInfo(root).fileName()));
        QVERIFY(qEnvironmentVariable("PATH") == root + "/bin");
        startDaemon();
    }
    void lifecycle() {
        Notifications notes;
        QSignalSpy activation(&notes, &Notifications::activated);
        notes.consume(state(row("blocked")));
        QTRY_COMPARE_WITH_TIMEOUT(calls("Notify").size(), 1, 5000);
        auto first = calls("Notify").last();
        QCOMPARE(first["replaces"].toInt(), 0);
        QCOMPARE(first["urgency"].toInt(), 2);
        QCOMPARE(first["body"].toString(), QString("&lt;b&gt;untrusted &amp; text&lt;/b&gt;\nTest · Fixture"));
        QVERIFY(!QFile::exists(root + "/audio.jsonl"));
        notes.consume(state(row("blocked", false))); QTest::qWait(150);
        QCOMPARE(calls("Notify").size(), 1);
        notes.consume(state(row("error")));
        QTRY_COMPARE(calls("Notify").size(), 2);
        QCOMPARE(calls("Notify").last()["replaces"], first["id"]);
        auto muted = row("error", false); muted["muted"] = true;
        notes.consume(state(muted, true));
        QTRY_COMPARE(calls("CloseNotification").size(), 1);
        notes.consume(state(row("blocked")));
        QTRY_COMPARE(calls("Notify").size(), 3);
        const auto clicked = calls("Notify").last()["id"].toInt();
        service("Click", clicked);
        QTRY_COMPARE(activation.size(), 1);
        const auto target = activation.first()[0].toMap();
        QCOMPARE(target["host"].toString(), QString("private-host"));
        QCOMPARE(target["session"].toString(), QString("owned-session"));
        QCOMPARE(target["window"].toString(), QString("owned-tab"));
        QCOMPARE(target["epoch"].toInt(), 7);
        QCOMPARE(activation.first()[1].toString(), QString("private-activation-token"));
        service("Click", clicked); QTest::qWait(100); QCOMPARE(activation.size(), 1);
        QTRY_COMPARE(calls("CloseNotification").size(), 2);
        notes.consume(state(row("error"))); QTRY_COMPARE(calls("Notify").size(), 4);
        auto seen = row("error", false); seen["seen"] = true;
        notes.consume(state(seen, true)); QTRY_COMPARE(calls("CloseNotification").size(), 3);
        QVERIFY(!QFile::exists(root + "/audio.jsonl"));
        // Program messages use the same captured lifecycle, and are consumed only
        // by publication, never by a QML refresh or clock tick.
        auto program = state(row("idle", false));
        program["programNotices"] = QJsonArray{QJsonObject{{"key","captured-tab"},{"activityEpoch",4},{"title","Owned program"},{"body","<a>plain</a>"}}};
        notes.consume(program); QTRY_COMPARE(calls("Notify").size(), 5);
        QCOMPARE(calls("Notify").last()["summary"].toString(), QString("Owned program"));
        notes.consume(QJsonObject{{"activity",QJsonObject{{"rows",QJsonArray{}}}}}); QTRY_COMPARE(calls("CloseNotification").size(), 4);
        service("RejectNext"); notes.test();
        QTRY_VERIFY(notes.testResult().startsWith("Test failed:"));
        QCOMPARE(notes.service(), QString("Service rejected the request"));
        notes.test(); QTRY_COMPARE(notes.testResult(), QString("Test notification sent"));
        // New daemon reuses numeric ids; old destinations cannot collide.
        notes.consume(state(row("blocked"))); QTRY_COMPARE(calls("Notify").size(), 7);
        daemon.terminate(); QVERIFY(daemon.waitForFinished(3000));
        QFile::remove(root + "/service-ready");
        QTRY_COMPARE(notes.service(), QString("No notification service"));
        notes.test(); QTRY_VERIFY(notes.testResult().startsWith("Test failed:"));
        startDaemon();
        notes.consume(state(row("error"))); QTRY_COMPARE(calls("Notify").size(), 8);
        QCOMPARE(calls("Notify").last()["replaces"].toInt(), 0);
        notes.clear(); QTRY_COMPARE(calls("CloseNotification").size(), 5);
    }
    void busRecovery() {
        Notifications notes;
        const int before = calls("Notify").size();
        notes.consume(state(row("blocked")));
        QTRY_COMPARE(calls("Notify").size(), before + 1);
        daemon.terminate(); QVERIFY(daemon.waitForFinished(3000));
        QFile::remove(root + "/service-ready");
        QVERIFY(QProcess::execute("/bin/kill", {qEnvironmentVariable("REX_NOTIFICATION_BUS_PID")}) == 0);
        QTest::qWait(100);
        notes.test();
        QTRY_VERIFY(notes.testResult().startsWith("Test failed:"));
        restartedBus.setStandardErrorFile(root + "/restarted-bus.log");
        restartedBus.start("/usr/bin/dbus-daemon", {"--config-file=" + root + "/bus.conf", "--nofork", "--nopidfile", "--address=" + qEnvironmentVariable("DBUS_SESSION_BUS_ADDRESS")});
        QVERIFY(restartedBus.waitForStarted()); QTest::qWait(100);
        startDaemon();
        notes.consume(state(row("error")));
        QTRY_COMPARE(calls("Notify").size(), before + 2);
        QCOMPARE(calls("Notify").last()["replaces"].toInt(), 0);
        notes.test(); QTRY_COMPARE(notes.testResult(), QString("Test notification sent"));
        notes.clear();
    }
    void boundedSounds() {
        Sounds sounds;
        QElapsedTimer ui; ui.start();
        sounds.offer(1); sounds.offer(2); sounds.offer(3);
        QVERIFY(ui.elapsed() < 100);
        QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(root + "/audio.jsonl"), 3000);
        QFile audio(root + "/audio.jsonl"); QVERIFY(audio.open(QIODevice::ReadOnly));
        QVERIFY(audio.readAll().contains("dialog-warning"));
        sounds.offer(1); // dropped during playback/cooldown
        QTRY_VERIFY_WITH_TIMEOUT(sounds.health().contains("did not finish"), 5000);
        sounds.check(); QTest::qWait(100);
        QVERIFY(sounds.health().contains("did not finish"));
        QFile mode(root + "/player-mode"); QVERIFY(mode.open(QIODevice::WriteOnly)); mode.write("exit"); mode.close();
        sounds.offer(1, true);
        QTRY_COMPARE_WITH_TIMEOUT(sounds.health(), QString("Sound player available."), 3000);
        Sounds cancelled;
        cancelled.offer(3); cancelled.cancel(); QTest::qWait(200);
        audio.seek(0); QCOMPARE(audio.readAll().count('\n'), 2);
        QFile::remove(root + "/player-mode");
        QElapsedTimer stopped;
        {
            Sounds stopping; stopping.offer(1, true);
            QTRY_VERIFY_WITH_TIMEOUT(([&] { audio.seek(0); return audio.readAll().count('\n') == 3; })(), 3000);
            stopped.start();
        }
        QVERIFY(stopped.elapsed() < 1500);
    }
    void cleanupTestCase() { daemon.terminate(); daemon.waitForFinished(3000); if (restartedBus.state() != QProcess::NotRunning) { restartedBus.terminate(); restartedBus.waitForFinished(3000); } }
};
QTEST_GUILESS_MAIN(NotificationServices)
#include "notification_services.moc"
