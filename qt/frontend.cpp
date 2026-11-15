#include "frontend.h"

#include <QJsonDocument>

Frontend::Frontend(QObject *parent) : QObject(parent) {
    connect(&m_process, &QProcess::readyReadStandardOutput, this, &Frontend::read);
    connect(&m_process, &QProcess::readyReadStandardError, this, [this] {
        // Diagnostics never enter the protocol stream or the terminal canvas.
        const auto bytes = m_process.readAllStandardError();
        fprintf(stderr, "%s", bytes.constData());
    });
    connect(&m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
        if (!m_stopping) fail("Terminal backend unavailable: " + m_process.errorString());
    });
    connect(&m_process, &QProcess::finished, this, [this](int, QProcess::ExitStatus) {
        if (!m_stopping) fail("Terminal backend stopped; input paused.");
    });
}

Frontend::~Frontend() {
    m_stopping = true;
    send({{"op", "shutdown"}});
    m_process.closeWriteChannel();
    if (!m_process.waitForFinished(3500)) {
        m_process.terminate();
        if (!m_process.waitForFinished(500)) {
            m_process.kill();
            m_process.waitForFinished(500);
        }
    }
}

void Frontend::start(const QString &program, const QString &endpoint, const QString &session) {
    QStringList args;
    if (!endpoint.isEmpty()) args << "--endpoint" << endpoint;
    if (!session.isEmpty()) args << "--session" << session;
    m_process.start(program, args);
}

bool Frontend::send(QJsonObject message) {
    if (m_process.state() != QProcess::Running) return false;
    message["version"] = 1;
    const auto data = QJsonDocument(message).toJson(QJsonDocument::Compact) + '\n';
    return m_process.write(data) == data.size();
}

bool Frontend::sendTarget(const QString &op, QJsonObject fields) {
    if (!m_hello || !m_state["terminal"].toObject()["connected"].toBool()) return false;
    for (const auto *name : {"session", "window", "pane"}) {
        const auto id = m_state[name].toString();
        if (id.isEmpty()) return false;
        fields[name] = id;
    }
    fields["op"] = op;
    return send(fields);
}

void Frontend::selectSession(const QString &id) { send({{"op", "select_session"}, {"session", id}}); }
void Frontend::selectWindow(const QString &id) { send({{"op", "select_window"}, {"session", m_state["session"]}, {"window", id}}); }
void Frontend::selectPane(const QString &id) {
    send({{"op", "select_pane"}, {"session", m_state["session"]}, {"window", m_state["window"]}, {"pane", id}});
}
void Frontend::refresh() { send({{"op", "refresh"}}); }
void Frontend::dismissProblem() { m_problem.clear(); emit problemChanged(); }

void Frontend::fail(const QString &message) {
    m_problem = message;
    m_state["connected"] = false;
    auto terminal = m_state["terminal"].toObject();
    terminal["connected"] = false;
    m_state["terminal"] = terminal;
    emit stateChanged();
    emit problemChanged();
}

void Frontend::read() {
    m_buffer += m_process.readAllStandardOutput();
    while (true) {
        const auto end = m_buffer.indexOf('\n');
        if (end < 0) break;
        QJsonParseError error;
        const auto doc = QJsonDocument::fromJson(m_buffer.left(end), &error);
        m_buffer.remove(0, end + 1);
        if (error.error != QJsonParseError::NoError || !doc.isObject()) {
            fail("Invalid terminal backend response; input paused.");
            m_process.kill();
            return;
        }
        const auto object = doc.object();
        if (object["version"].toInt() != 1) {
            fail("Terminal backend version mismatch; input paused.");
            m_process.kill();
            return;
        }
        const auto type = object["type"].toString();
        if (type == "hello") m_hello = true;
        else if (type == "state" && m_hello) { m_state = object; emit stateChanged(); }
        else if (type == "error") { m_problem = object["message"].toString(); emit problemChanged(); }
    }
}
