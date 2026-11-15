#include "frontend.h"
#include <QClipboard>
#include <QGuiApplication>

#include <QJsonDocument>
#include <QJsonArray>

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

void Frontend::start(const QString &program, const QString &endpoint, const QString &session, const QString &config) {
    QStringList args;
    if (!endpoint.isEmpty()) args << "--endpoint" << endpoint;
    if (!session.isEmpty()) args << "--session" << session;
    if (!config.isEmpty()) args << "--config" << config;
    m_process.start(program, args);
}

bool Frontend::send(QJsonObject message) {
    if (m_process.state() != QProcess::Running) return false;
    message["version"] = 2;
    message["epoch"] = m_state["epoch"];
    const auto data = QJsonDocument(message).toJson(QJsonDocument::Compact) + '\n';
    return m_process.write(data) == data.size();
}

QJsonObject Frontend::paneSnapshot(const QString &id) const {
    for (const auto &value : m_state["panes"].toArray()) {
        const auto pane = value.toObject();
        if (pane["id"].toString() == id) {
            auto result = pane;
            result["connected"] = m_state["connected"];
            result["epoch"] = m_state["epoch"];
            if (!m_state["connected"].toBool()) {
                auto terminal = result["terminal"].toObject(); terminal["connected"] = false;
                result["terminal"] = terminal;
            }
            return result;
        }
    }
    return {};
}

bool Frontend::sendTarget(const QString &op, QJsonObject fields) {
    return sendPane(m_state["pane"].toString(), op, fields);
}

bool Frontend::sendPane(const QString &id, const QString &op, QJsonObject fields) {
    const auto pane = paneSnapshot(id);
    const bool local = op == "scroll" || op == "search" || op == "search_step" || op == "prompt_step" || op == "selection" || op == "reconnect";
    if (!m_hello || (op != "reconnect" && (local ? !pane["frame"].isObject() : !pane["terminal"].toObject()["connected"].toBool()))) return false;
    for (const auto *name : {"session", "window", "pane"}) {
        const auto value = pane[name].toString();
        if (value.isEmpty()) return false;
        fields[name] = value;
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
void Frontend::reloadShortcuts() { send({{"op","keymap_reload"}}); }
void Frontend::recordShortcut(const QString &action, const QStringList &keys, const QVariantMap &args, const QString &mode, const QString &operation) {
    send({{"op","shortcut"},{"action",action},{"keys",QJsonArray::fromStringList(keys)},
          {"args",QJsonObject::fromVariantMap(args)},{"mode",mode},{"operation",operation}});
}
void Frontend::performAction(const QString &action, const QVariantMap &args, const QString &origin) {
    send({{"op","action"},{"action",action},{"args",QJsonObject::fromVariantMap(args)},{"origin",origin},
          {"generation",m_state["keymap"].toObject()["generation"]},{"revision",m_state["keymap"].toObject()["revision"]},{"session",m_state["session"]},
          {"window",m_state["window"]},{"pane",m_state["pane"]}});
}
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
        if (object["version"].toInt() != 2) {
            fail("Terminal backend version mismatch; input paused.");
            m_process.kill();
            return;
        }
        const auto type = object["type"].toString();
        if (type == "hello") { m_hello = true; setVisible(m_visible); }
        else if (type == "state" && m_hello) {
            const bool newEpoch = m_state["epoch"] != object["epoch"];
            m_state = object;
            if (newEpoch) setVisible(m_visible);
            QStringList ids;
            for (const auto &pane : object["panes"].toArray()) ids << pane.toObject()["id"].toString();
            if (ids != m_paneIds) { m_paneIds = ids; emit paneIdsChanged(); }
            emit stateChanged();
        }
        else if (type == "clipboard" && m_visible && object["epoch"] == m_state["epoch"]
                 && object["session"] == m_state["session"] && object["pane"] == m_state["pane"]
                 && m_state["connected"].toBool()) QGuiApplication::clipboard()->setText(object["text"].toString());
        else if (type == "action" && m_visible && object["epoch"] == m_state["epoch"] &&
                 object["generation"] == m_state["keymap"].toObject()["generation"] &&
                 object["session"] == m_state["session"] && object["pane"] == m_state["pane"])
            emit actionRequested(object["action"].toString(), object["args"].toObject().toVariantMap());
        else if (type == "error") { m_problem = object["message"].toString(); emit problemChanged(); }
    }
}

void Frontend::setVisible(bool visible) { m_visible = visible; if (m_hello && m_state["epoch"].isDouble()) send({{"op", "visibility"}, {"visible", visible}}); }

void Frontend::reportProblem(const QString &message) { m_problem = message; emit problemChanged(); }
