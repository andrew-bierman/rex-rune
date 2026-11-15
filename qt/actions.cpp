#include "actions.h"
#include <QCoreApplication>
#include <QKeySequence>
#include <algorithm>

namespace {
quint64 identity(const QKeyEvent &event) {
    return event.nativeScanCode() ? quint64(event.nativeScanCode()) | (quint64(1) << 32) : quint64(event.key());
}
QString namedKey(int key) {
    if (key >= Qt::Key_F1 && key <= Qt::Key_F35) return "f" + QString::number(key - Qt::Key_F1 + 1);
    switch (key) {
    case Qt::Key_Return: case Qt::Key_Enter: return "enter";
    case Qt::Key_Escape: return "escape";
    case Qt::Key_Tab: case Qt::Key_Backtab: return "tab";
    case Qt::Key_Space: return "space";
    case Qt::Key_Backspace: return "backspace";
    case Qt::Key_Delete: return "delete";
    case Qt::Key_Insert: return "insert";
    case Qt::Key_Left: return "left";
    case Qt::Key_Right: return "right";
    case Qt::Key_Up: return "up";
    case Qt::Key_Down: return "down";
    case Qt::Key_Home: return "home";
    case Qt::Key_End: return "end";
    case Qt::Key_PageUp: return "pageup";
    case Qt::Key_PageDown: return "pagedown";
    default: return {};
    }
}
QString physicalKey(int key) {
    if (key >= Qt::Key_A && key <= Qt::Key_Z) return "Key" + QString(QChar('A' + key - Qt::Key_A));
    if (key >= Qt::Key_0 && key <= Qt::Key_9) return "Digit" + QString(QChar('0' + key - Qt::Key_0));
    switch (key) {
    case Qt::Key_Plus: case Qt::Key_Equal: return "Equal";
    case Qt::Key_Minus: case Qt::Key_Underscore: return "Minus";
    default: return {};
    }
}
bool chordMatches(const QJsonObject &chord, const QKeyEvent &event) {
    const auto mods = event.modifiers();
    if (chord["ctrl"].toBool() != mods.testFlag(Qt::ControlModifier) ||
        chord["alt"].toBool() != mods.testFlag(Qt::AltModifier) ||
        chord["shift"].toBool() != mods.testFlag(Qt::ShiftModifier) ||
        chord["super"].toBool() != mods.testFlag(Qt::MetaModifier)) return false;
    const auto wanted = chord["key"].toString();
    const auto kind = chord["kind"].toString();
    if (kind == "named") return namedKey(event.key()) == wanted;
    if (kind == "physical") return physicalKey(event.key()) == wanted;
    if (kind != "character") return false;
    if (event.key() >= Qt::Key_A && event.key() <= Qt::Key_Z)
        return wanted.compare(QString(QChar('a' + event.key() - Qt::Key_A)), Qt::CaseInsensitive) == 0;
    if (event.key() >= Qt::Key_Space && event.key() <= Qt::Key_AsciiTilde && wanted == QString(QChar(event.key())).toLower()) return true;
    if (event.modifiers().testFlag(Qt::ShiftModifier)) {
        const auto physical = physicalKey(event.key());
        if (physical == "Equal" && wanted == "=") return true;
        if (physical == "Minus" && wanted == "-") return true;
    }
    return !event.text().isEmpty() && event.text().compare(wanted, Qt::CaseInsensitive) == 0;
}

}

Actions::Actions(QObject *parent) : QObject(parent) {
    if (QCoreApplication::instance()) QCoreApplication::instance()->installEventFilter(this);
}

void Actions::setSnapshot(const QJsonObject &state) {
    const auto map = state["keymap"].toObject();
    const auto context = state["host"].toString() + "/" + state["session"].toString() + '/' + state["pane"].toString() + '/' +
        QString::number(state["epoch"].toInteger()) + '/' + QString::number(map["generation"].toInteger()) + '/' + QString::number(map["revision"].toInteger());
    const bool contextChanged = context != m_context;
    if (contextChanged) {
        clearSequence();
        if (state["host"] != m_state["host"] || state["session"] != m_state["session"] || state["epoch"] != m_state["epoch"] || map["generation"] != m_state["keymap"].toObject()["generation"] ||
            map["revision"] != m_state["keymap"].toObject()["revision"]) m_activeModes.clear();
        m_context = context;
    }
    const auto commands = map["actions"].toArray();
    const auto bindings = map["bindings"].toArray();
    const auto modes = map["modes"].toArray();
    const bool changed = commands != m_commands || bindings != m_bindings || modes != m_modes;
    m_state = state;
    m_commands = commands;
    m_bindings = bindings;
    m_modes = modes;
    if (changed || contextChanged) emit this->changed();
}

QVariantList Actions::commands() const { return m_commands.toVariantList(); }
QVariantList Actions::bindings() const { return m_bindings.toVariantList(); }
QStringList Actions::activeModes() const {
    QStringList names;
    for (const auto &mode : m_activeModes) names << mode.first;
    return names;
}
void Actions::clearSequence() {
    m_depth = 0;
    m_candidates.clear();
    m_pending.clear();
}
void Actions::setModalOpen(bool value) {
    if (m_modal == value) return;
    m_modal = value;
    clearSequence();
    emit changed();
}

bool Actions::available(const QJsonObject &binding, bool globalOnly) const {
    const auto id = binding["action"].toString();
    if (id == "client.mode.block") return !globalOnly;
    const auto when = binding["args"].toObject()["when"].toString();
    if (when == "searching" && !m_searching) return false;
    for (const auto &value : m_commands) {
        const auto command = value.toObject();
        if (command["id"].toString() == id) return command["enabled"].toBool() &&
            (!globalOnly || command["global"].toBool()) && (command["context"] != "search" || m_searching);
    }
    return false;
}

bool Actions::handleKey(QKeyEvent *event, bool globalOnly) {
    const auto key = identity(*event);
    if (event->type() == QEvent::KeyRelease) {
        if (event->isAutoRepeat()) return m_consumed.contains(key);
        return m_consumed.remove(key) > 0;
    }
    if (event->type() != QEvent::KeyPress) return false;
    if (!event->isAutoRepeat()) m_consumed.remove(key);
    else if (m_consumed.value(key, false)) return true;
    if (event->key() == Qt::Key_Shift || event->key() == Qt::Key_Control || event->key() == Qt::Key_Alt || event->key() == Qt::Key_Meta) return false;
    if (m_recording) {
        if (event->key() == Qt::Key_Escape) { m_recording = false; m_consumed.insert(key, true); emit changed(); return true; }
        if (!event->isAutoRepeat()) m_recorded << QKeySequence(event->keyCombination()).toString(QKeySequence::PortableText).toLower();
        m_consumed.insert(key, true);
        emit changed();
        return true;
    }
    if (m_depth > 0 && event->key() == Qt::Key_Escape) {
        clearSequence();
        m_consumed.insert(key, true);
        emit changed();
        return true;
    }
    QVector<int> indices;
    if (m_depth) indices = m_candidates;
    else {
        bool exclusive = false;
        if (!globalOnly) for (auto mode = m_activeModes.crbegin(); mode != m_activeModes.crend(); ++mode) {
            for (int i = 0; i < m_bindings.size(); ++i)
                if (m_bindings[i].toObject()["mode"].toString() == mode->first) indices << i;
            for (const auto &definition : m_modes)
                if (definition.toObject()["name"].toString() == mode->first && definition.toObject()["exclusive"].toBool()) exclusive = true;
            if (exclusive) break;
        }
        if (!exclusive) for (int i = 0; i < m_bindings.size(); ++i)
            if (m_bindings[i].toObject()["mode"].isNull() || !m_bindings[i].toObject().contains("mode")) indices << i;
    }
    QVector<int> matching;
    for (int i : indices) {
        const auto binding = m_bindings[i].toObject();
        const auto chords = binding["chords"].toArray();
        if (m_depth < chords.size() && chordMatches(chords[m_depth].toObject(), *event) && available(binding, globalOnly)) matching << i;
    }
    if (matching.isEmpty()) {
        const bool wasPending = m_depth > 0;
        clearSequence();
        if (wasPending) emit changed();
        bool exclusive = false;
        for (const auto &mode : m_modes) for (const auto &active : m_activeModes)
            if (mode.toObject()["name"].toString() == active.first && mode.toObject()["exclusive"].toBool()) exclusive = true;
        if (!globalOnly && exclusive) { m_consumed.insert(key, true); return true; }
        return false;
    }
    const auto binding = m_bindings[matching.first()].toObject();
    const auto chords = binding["chords"].toArray();
    m_consumed.insert(key, true);
    if (chords.size() == m_depth + 1) {
        const bool sequence = m_depth > 0;
        const auto id = binding["action"].toString();
        bool repeats = false;
        for (const auto &value : m_commands) if (value.toObject()["id"].toString() == id) repeats = value.toObject()["repeats"].toBool();
        m_consumed[key] = sequence || !repeats;
        clearSequence();
        const auto mode = binding["mode"].toString();
        if (!mode.isEmpty() && !id.startsWith("client.mode.exit"))
            for (int i = m_activeModes.size() - 1; i >= 0; --i)
                if (m_activeModes[i].first == mode && m_activeModes[i].second) { m_activeModes.removeAt(i); break; }
        if (id != "client.mode.block") run(id, binding["args"].toObject().toVariantMap(), "key");
    } else {
        ++m_depth;
        m_candidates.clear();
        for (int i : matching) if (m_bindings[i].toObject()["chords"].toArray().size() > m_depth) m_candidates << i;
        QStringList keys;
        const auto spelled = binding["keys"].toArray();
        for (int i = 0; i < m_depth && i < spelled.size(); ++i) keys << (chords[i].toObject()["label"].toString().isEmpty() ? spelled[i].toString() : chords[i].toObject()["label"].toString());
        m_pending = keys.join(", ") + " …";
    }
    emit changed();
    return true;
}

void Actions::run(const QString &id, const QVariantMap &args, const QString &origin) {
    QJsonObject command;
    for (const auto &value : m_commands) if (value.toObject()["id"].toString() == id) { command = value.toObject(); break; }
    if (command.isEmpty() || !command["enabled"].toBool()) { emit failed("Action is unavailable"); return; }
    if (id == "client.mode.enter") {
        const auto name = args.value("name").toString();
        const bool exists = std::any_of(m_modes.begin(), m_modes.end(), [&](const QJsonValue &mode) { return mode.toObject()["name"].toString() == name; });
        if (!exists) { emit failed("Unknown key mode: " + name); return; }
        m_activeModes.removeIf([&](const auto &mode) { return mode.first == name; });
        m_activeModes.append({name, args.value("once").toBool()});
        clearSequence();
        emit changed();
        return;
    }
    if (id == "client.mode.exit" || id == "client.mode.exit_all") {
        if (id.endsWith("exit_all")) m_activeModes.clear();
        else if (!m_activeModes.isEmpty()) m_activeModes.removeLast();
        clearSequence();
        emit changed();
        return;
    }
    emit requested(id, args, origin);
}

QString Actions::hint(const QString &id) const {
    for (const auto &value : m_commands) if (value.toObject()["id"].toString() == id) return value.toObject()["hint"].toString();
    return {};
}
void Actions::beginRecording() {
    clearSequence(); m_recorded.clear(); m_recording = true; emit changed();
}
void Actions::stopRecording() { m_recording = false; emit changed(); }
bool Actions::eventFilter(QObject *, QEvent *event) {
    if (event->type() == QEvent::KeyPress && !m_recording && !m_modal && (m_depth > 0 || !m_activeModes.isEmpty())) return false;
    if (event->type() == QEvent::KeyPress || event->type() == QEvent::KeyRelease) return handleKey(static_cast<QKeyEvent *>(event), true);
    return false;
}
