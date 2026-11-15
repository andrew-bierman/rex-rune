#pragma once
#include <QObject>
#include <QJsonArray>
#include <QElapsedTimer>
#include <QTimer>
class Frontend;
class TerminalItem;
class QQuickWindow;
class TestDriver : public QObject {
    Q_OBJECT
public:
    TestDriver(QQuickWindow *window, Frontend *backend, const QString &script, const QString &report);
    void start();
private:
    void step();
    void finish(bool success, const QString &message = {});
    void clickControl(const QString &name);
    QQuickWindow *m_window;
    Frontend *m_backend;
    TerminalItem *m_terminal;
    QString m_script;
    QString m_report;
    QJsonArray m_steps;
    int m_index = 0;
    QTimer m_timer;
    QElapsedTimer m_wait;
    bool m_waiting = false;
    bool m_menuActivated = false;
    QJsonArray m_evidence;
};
