#include "frontend.h"
#include "actions.h"
#include "terminal.h"
#ifdef REX_QT_TEST_DRIVER
#include "test_driver.h"
#endif

#include <QCommandLineParser>
#include <QDir>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QTimer>

int main(int argc, char **argv) {
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName("rex-qt");
    QCoreApplication::setApplicationVersion("0.1.0");
    QQuickStyle::setStyle("Basic");
    QCommandLineParser parser;
    parser.setApplicationDescription("Standalone Qt Quick frontend for existing Rex servers");
    parser.addHelpOption();
    parser.addVersionOption();
    parser.addOption({"config", "Separate Qt workspace settings path", "path"});
    parser.addOption({"endpoint", "Existing Rex server endpoint", "endpoint"});
    parser.addOption({"session", "Initially selected session ID or label", "session"});
    parser.addOption({"backend", "Path to rex-qt-backend", "path", QCoreApplication::applicationDirPath() + "/rex-qt-backend"});
#ifdef REX_QT_TEST_DRIVER
    parser.addOption({"script", "Qt event acceptance script", "path"});
    parser.addOption({"report", "Acceptance JSON report", "path"});
#endif
    parser.process(app);
    qmlRegisterType<TerminalItem>("RexQt", 1, 0, "TerminalView");
    qmlRegisterUncreatableType<Frontend>("RexQt", 1, 0, "Frontend", "Created by the application");
    qmlRegisterUncreatableType<Actions>("RexQt", 1, 0, "Actions", "Created by the application");
    Frontend backend;
    Actions workspaceActions;
    QObject::connect(&backend, &Frontend::stateChanged, &workspaceActions, [&] { workspaceActions.setSnapshot(backend.snapshot()); });
    QObject::connect(&backend, &Frontend::actionRequested, &workspaceActions, [&](const QString &id, const QVariantMap &args) { workspaceActions.run(id, args, "api"); });
    QObject::connect(&workspaceActions, &Actions::failed, &backend, &Frontend::reportProblem);
    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty("backend", &backend);
    engine.rootContext()->setContextProperty("workspaceActions", &workspaceActions);
    engine.load(QUrl("qrc:/Main.qml"));
    if (engine.rootObjects().isEmpty()) return 1;
    backend.start(parser.value("backend"), parser.value("endpoint"), parser.value("session"), parser.value("config"));
#ifdef REX_QT_TEST_DRIVER
    const auto window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
    TestDriver driver(window, &backend, parser.value("script"), parser.value("report"));
    QTimer::singleShot(0, &driver, &TestDriver::start);
#endif
    return app.exec();
}
