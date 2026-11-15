#include "frontend.h"
#include <QClipboard>
#include <QFile>
#include <QGuiApplication>
#include <QDesktopServices>
#include <QFileInfo>
#include <QUrl>

#include <QJsonDocument>
#include <QJsonArray>

// QML uses frame metadata; the C++ terminal keeps the full cells and images
// in the raw snapshot. Avoid converting that pixel/grid payload for bindings.
QJsonObject terminalMetadata(QJsonObject state) {
    auto frame = state["frame"].toObject();
    if (!frame.isEmpty()) {
        frame.remove("cells");
        frame.remove("images");
        state["frame"] = frame;
    }
    auto panes = state["panes"].toArray();
    for (qsizetype i = 0; i < panes.size(); ++i) panes[i] = terminalMetadata(panes[i].toObject());
    if (state.contains("panes")) state["panes"] = panes;
    return state;
}

Frontend::Frontend(QObject *parent) : QObject(parent), m_notifications(this) {
    connect(&m_notifications, &Notifications::activated, this, [this](const QVariantMap &target, const QString &token) {
        hostAction("navigate", {{"session", target.value("session")}, {"window", target.value("window")}}, target);
        emit notificationActivated(token);
    });
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
    message["version"] = 3;
    if (!message.contains("epoch")) message["epoch"] = m_state["epoch"];
    if (!message.contains("host")) message["host"] = m_state["host"];
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
            result["host"] = m_state["host"];
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
    if (m_state["hosts"].toObject()["pending"].toBool()) return false;
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
void Frontend::hostAction(const QString &action, const QVariantMap &args, const QVariantMap &captured) {
    auto message = QJsonObject::fromVariantMap(captured);
    message["op"] = "host";
    message["action"] = action;
    message["args"] = QJsonObject::fromVariantMap(args);
    send(message);
}
void Frontend::fileAction(const QString &op, const QVariantMap &fields, const QVariantMap &captured) {
    if (op != "download.open" && op != "download.start" && op != "download.confirm" && op != "download.close" && op != "download.cancel" && op != "directory.open" && op != "directory.edit" && op != "directory.choose" && op != "directory.close") {
        reportProblem("Unknown file action.");
        return;
    }
    auto message = QJsonObject::fromVariantMap(fields);
    for (auto it = captured.begin(); it != captured.end(); ++it) message[it.key()] = QJsonValue::fromVariant(it.value());
    message["op"] = op;
    send(message);
}
void Frontend::workspaceAction(const QString &action, const QVariantMap &args, const QVariantMap &captured) {
    auto message = QJsonObject::fromVariantMap(captured);
    message["op"] = "workspace";
    message["action"] = action;
    message["args"] = QJsonObject::fromVariantMap(args);
    send(message);
}
quint64 Frontend::savePreferences(const QVariantMap &patch) {
    const auto id = ++m_preferenceRequest;
    dismissPreferencesProblem();
    if (!send({{"op","preferences"},{"request_id",qint64(id)},{"patch",QJsonObject::fromVariantMap(patch)}})) {
        m_preferencesProblem = "Settings backend unavailable"; emit preferencesProblemChanged();
        return 0;
    }
    return id;
}
void Frontend::activityAction(const QString &action, const QVariantMap &args) { send({{"op","activity"},{"action",action},{"args",QJsonObject::fromVariantMap(args)}}); }
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
    m_notifications.clear();
    m_problem = message;
    m_state["connected"] = false;
    // Captured requests belong to the child that just stopped. Keep the last
    // terminal frame and facts, but release dialogs and controls waiting on it.
    m_state["workspace"] = QJsonValue();
    auto files = m_state["files"].toObject();
    for (const auto *name : {"directory", "download", "upload"}) files[name] = QJsonValue();
    m_state["files"] = files;
    auto hosts = m_state["hosts"].toObject();
    hosts["pending"] = false;
    auto rows = hosts["rows"].toArray();
    for (qsizetype i = 0; i < rows.size(); ++i) {
        auto row = rows[i].toObject();
        row["connected"] = false;
        row["status"] = "Backend unavailable · last reported";
        rows[i] = row;
    }
    hosts["rows"] = rows;
    m_state["hosts"] = hosts;
    auto maintenance = m_state["maintenance"].toObject();
    maintenance["busy"] = false;
    maintenance["preview"] = QJsonValue();
    m_state["maintenance"] = maintenance;
    auto downloads = m_state["downloads"].toArray();
    for (qsizetype i = 0; i < downloads.size(); ++i) {
        auto row = downloads[i].toObject();
        if (!row["running"].toBool()) continue;
        row["running"] = false;
        row["confirmation"] = false;
        row["status"] = "Backend unavailable; transfer outcome unknown";
        downloads[i] = row;
    }
    m_state["downloads"] = downloads;
    auto server = m_state["server"].toObject();
    server["connected"] = false; server["status"] = "Disconnected · last reported";
    server["pair"] = QJsonValue(); server["confirmation"] = QJsonValue();
    server["can_pair"] = false; server["can_stop"] = false; server["can_setup"] = false;
    server["setup_running"] = false; server["uptime_seconds"] = QJsonValue();
    m_state["server"] = server;
    auto terminal = m_state["terminal"].toObject();
    terminal["connected"] = false;
    m_state["terminal"] = terminal;
    m_variantState = terminalMetadata(m_state).toVariantMap();
    emit hostsChanged();
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
        if (object["version"].toInt() != 3) {
            fail("Terminal backend version mismatch; input paused.");
            m_process.kill();
            return;
        }
        const auto type = object["type"].toString();
        if (type == "hello") { m_hello = true; setVisible(m_visible); }
        else if (type == "state" && m_hello) {
            const bool newEpoch = m_state["epoch"] != object["epoch"] || m_state["host"] != object["host"];
            const bool newActivity = m_state["activity"] != object["activity"];
            const bool newPreferences = m_state["preferences"] != object["preferences"];
            const bool newHosts = m_state["hosts"] != object["hosts"];
            m_state = object;
            m_variantState = terminalMetadata(object).toVariantMap();
            m_notifications.consume(object);
            if (newEpoch) setVisible(m_visible);
            QStringList ids;
            for (const auto &pane : object["panes"].toArray()) ids << pane.toObject()["id"].toString();
            if (ids != m_paneIds) { m_paneIds = ids; emit paneIdsChanged(); }
            if (newActivity) emit activityChanged();
            if (newPreferences) emit preferencesChanged();
            if (newHosts) emit hostsChanged();
            emit stateChanged();
        }
        else if (type == "remote_check") emit remoteCheck(object.toVariantMap());
        else if (type == "preferences_saved") emit preferencesSaved(quint64(object["request_id"].toInteger()));
        else if (type == "error" && object["scope"] == "preferences") {
            m_preferencesProblem = object["message"].toString(); emit preferencesProblemChanged();
            emit preferencesRejected(quint64(object["request_id"].toInteger()));
        }
        else if (type == "host_saved") emit hostSaved();
        else if (type == "clipboard" && m_visible && object["host"] == m_state["host"] && object["epoch"] == m_state["epoch"]
                 && object["session"] == m_state["session"] && object["pane"] == m_state["pane"]
                 && m_state["connected"].toBool()) QGuiApplication::clipboard()->setText(object["text"].toString());
        else if (type == "action" && m_visible && object["host"] == m_state["host"] && object["epoch"] == m_state["epoch"] &&
                 object["generation"] == m_state["keymap"].toObject()["generation"] &&
                 object["session"] == m_state["session"] && object["pane"] == m_state["pane"])
            emit actionRequested(object["action"].toString(), object["args"].toObject().toVariantMap());
        else if (type == "error" && object["scope"] == "host_form") {
            m_hostFormProblem = object["message"].toString(); emit hostFormProblemChanged();
        }
        else if (type == "notice" && object["host"] == m_state["host"] && object["epoch"] == m_state["epoch"]) { m_problem = object["message"].toString(); emit problemChanged(); }
        else if (type == "error") { m_problem = object["message"].toString(); emit problemChanged(); }
    }
}

void Frontend::authorizeRemote(const QVariantMap &request, bool accepted) {
    auto message=QJsonObject::fromVariantMap(request);
    message["op"]="remote_authorize";
    message["accepted"]=accepted;
    send(message);
}

void Frontend::viewAction(const QString &action, const QVariantMap &args) {
    send({{"op","view_action"},{"action",action},{"args",QJsonObject::fromVariantMap(args)},
          {"session",m_state["session"]},{"window",m_state["window"]},{"pane",m_state["pane"]}});
}

void Frontend::remoteContext(bool modal, bool field, bool sequence, bool keypress) {
    if (m_hello) send({{"op","remote_context"},{"modal",modal},{"field",field},{"sequence",sequence},{"keypress",keypress}});
}

void Frontend::setVisible(bool visible) { m_visible = visible; if (m_hello && m_state["epoch"].isDouble()) send({{"op", "visibility"}, {"visible", visible}}); }

void Frontend::reportProblem(const QString &message) { m_problem = message; emit problemChanged(); }
void Frontend::dismissHostFormProblem() { m_hostFormProblem.clear(); emit hostFormProblemChanged(); }

void Frontend::dismissPreferencesProblem() { m_preferencesProblem.clear(); emit preferencesProblemChanged(); }

void Frontend::serverAction(const QString &action, quint64 requestId, const QString &text) {
    static const QStringList allowed = {"page_open","page_close","read","prepare_stop","prepare_setup","cancel","confirm","pair","pair_close","copy"};
    if (!allowed.contains(action)) return;
    send({{"op","server_" + action},{"request_id",qint64(requestId)},{"text",text}});
}
void Frontend::showDownload(const QString &host, quint64 id) {
    for (const auto &value : m_state["downloads"].toArray()) {
        const auto row = value.toObject();
        if (row["host"].toString() == host && row["id"].toInteger() == qint64(id) && row["saved"].isString()) {
            if (!QDesktopServices::openUrl(QUrl::fromLocalFile(QFileInfo(row["saved"].toString()).absolutePath()))) reportProblem("Could not show the download folder.");
            return;
        }
    }
    reportProblem("Saved download is no longer available.");
}

void Frontend::maintenanceAction(const QString &action, const QVariantMap &args) { send({{"op","maintenance"},{"action",action},{"args",QJsonObject::fromVariantMap(args)}}); }

QString Frontend::maintenanceReport() const {
    return m_state["maintenance"].toObject()["report"].toString() + "\nQt runtime " + QString::fromLatin1(qVersion()) + " (LGPLv3 / GPLv3 / commercial, per Qt module).";
}
void Frontend::copyMaintenanceReport() { QGuiApplication::clipboard()->setText(maintenanceReport()); }

QString Frontend::maintenanceLicenses() const {
    QString text;
    for (const auto &name : {"Project", "rexrs-MIT", "rexrs-Apache", "Ghostty-wrapper", "Ghostty", "IBM-Plex-Mono", "Symbols-Nerd-Font"}) {
        QFile file(QStringLiteral(":/licenses/") + QString::fromLatin1(name));
        if (file.open(QIODevice::ReadOnly)) text += "\n\n" + QString::fromLatin1(name) + "\n" + QString::fromUtf8(file.readAll());
    }
    text += "\n\nQt: https://www.qt.io/licensing/ and https://doc.qt.io/qt-6/licenses-used-in-qt.html";
    return text;
}
