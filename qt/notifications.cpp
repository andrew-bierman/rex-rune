#include "notifications.h"
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDBusError>
#include <QJsonArray>
#include <QSet>
#include <functional>

namespace {
const QString serviceName = "org.freedesktop.Notifications";
const QString servicePath = "/org/freedesktop/Notifications";
void call(QObject *parent, const QDBusConnection &bus, const QString &owner, const QString &path,
          const QString &interface, const QString &method, const QVariantList &args,
          std::function<void(const QDBusMessage &)> done) {
    auto message = QDBusMessage::createMethodCall(owner, path, interface, method);
    message.setArguments(args);
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(message, 3000), parent);
    QObject::connect(watcher, &QDBusPendingCallWatcher::finished, parent, [watcher, done] {
        const auto reply = watcher->reply(); watcher->deleteLater(); done(reply);
    });
}
QString failure(const QDBusMessage &reply) {
    if (reply.type() != QDBusMessage::ErrorMessage) return {};
    const auto error = QDBusError(reply).type();
    if (error == QDBusError::ServiceUnknown || reply.errorName() == "org.freedesktop.DBus.Error.NameHasNoOwner") return "No notification service";
    if (error == QDBusError::NoReply || error == QDBusError::Timeout || error == QDBusError::TimedOut) return "Notification service did not respond";
    if (error == QDBusError::Disconnected || error == QDBusError::NoServer) return "Could not connect to the session bus";
    return "Service rejected the request";
}
bool enabled(const QJsonObject &config, const QString &key, bool defaultValue = true) {
    return config.value(key).toBool(defaultValue);
}
}
Notifications::Notifications(QObject *parent) : QObject(parent), m_sounds(this) {
    m_busName = "rex-notifications-" + QString::number(quintptr(this), 16);
    connect(&m_sounds, &Sounds::changed, this, &Notifications::changed);
}
Notifications::~Notifications() { QDBusConnection::disconnectFromBus(m_busName); }
QDBusConnection Notifications::bus() {
    if (!m_bus.isConnected()) {
        QDBusConnection::disconnectFromBus(m_busName);
        m_bus = QDBusConnection::connectToBus(QDBusConnection::SessionBus, m_busName);
        m_watching = false;
        m_owner.clear(); ++m_generation; m_live.clear(); m_order.clear();
    }
    return m_bus;
}
QString Notifications::key(const QJsonObject &target) { return target["key"].toString(); }
void Notifications::check() { Note note; note.check = true; enqueue(note); }
void Notifications::test() { m_test = "Sending test…"; emit changed(); Note note; note.test = true; note.title = "Rex test notification"; note.body = "Desktop notifications can reach this desktop."; enqueue(note); }
void Notifications::testSound() { m_sounds.offer(1, true); }
void Notifications::checkSound() { m_sounds.check(); }
void Notifications::enqueue(Note note) {
    if (!note.test && !note.check) {
        const auto k = key(note.target);
        note.serial = ++m_next; m_serials[k] = note.serial;
        for (auto it = m_pending.begin(); it != m_pending.end();) {
            if (!it->test && !it->check && key(it->target) == k) it = m_pending.erase(it); else ++it;
        }
    }
    m_pending.enqueue(note); pump();
}
bool Notifications::current(const Note &note) const {
    if (note.test || note.check) return true;
    const auto k = key(note.target);
    const auto row = m_rows.value(k);
    return !row.isEmpty() && !row["seen"].toBool() && !row["muted"].toBool()
        && row["activityEpoch"] == note.target["activityEpoch"] && m_serials.value(k) == note.serial;
}
void Notifications::changeOwner(const QString &owner) {
    if (m_owner == owner) return;
    auto connection = bus();
    if (!m_owner.isEmpty()) {
        connection.disconnect(m_owner, servicePath, serviceName, "ActionInvoked", this, SLOT(action(uint,QString)));
        connection.disconnect(m_owner, servicePath, serviceName, "NotificationClosed", this, SLOT(closed(uint,uint)));
        connection.disconnect(m_owner, servicePath, serviceName, "ActivationToken", this, SLOT(token(uint,QString)));
    }
    m_owner = owner; ++m_generation; m_live.clear(); m_order.clear();
    if (!owner.isEmpty()) {
        connection.connect(owner, servicePath, serviceName, "ActionInvoked", this, SLOT(action(uint,QString)));
        connection.connect(owner, servicePath, serviceName, "NotificationClosed", this, SLOT(closed(uint,uint)));
        connection.connect(owner, servicePath, serviceName, "ActivationToken", this, SLOT(token(uint,QString)));
    }
}
void Notifications::ownerChanged(const QString &name, const QString &, const QString &after) {
    if (name == serviceName && after != m_owner) {
        changeOwner(after); m_service = after.isEmpty() ? "No notification service" : "Not checked"; emit changed();
    }
}
void Notifications::pump() {
    if (m_busy || m_pending.isEmpty()) return;
    const auto note = m_pending.dequeue();
    if (!current(note)) { pump(); return; }
    m_busy = true;
    auto connection = bus();
    if (!connection.isConnected()) { finish(note, "Could not connect to the session bus"); return; }
    if (!m_watching) {
        connection.connect("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameOwnerChanged", this, SLOT(ownerChanged(QString,QString,QString)));
        m_watching = true;
    }
    if (m_owner.isEmpty()) {
        // Calling the standard service name permits its ordinary D-Bus activation,
        // just as the source client does. Numeric ids are pinned only afterwards.
        call(this, connection, serviceName, servicePath, serviceName, "GetServerInformation", {}, [this, note](const QDBusMessage &reply) {
            const auto error = failure(reply);
            if (!error.isEmpty()) finish(note, error); else resolve(note);
        });
    } else resolve(note);
}
void Notifications::resolve(const Note &note) {
    call(this, bus(), "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner", {serviceName}, [this, note](const QDBusMessage &reply) {
        const auto error = failure(reply);
        if (!error.isEmpty() || reply.arguments().isEmpty()) { finish(note, error.isEmpty() ? "No notification service" : error); return; }
        const auto owner = reply.arguments().first().toString();
        changeOwner(owner); const auto generation = m_generation;
        call(this, bus(), owner, servicePath, serviceName, "GetServerInformation", {}, [this, note, owner, generation](const QDBusMessage &info) {
            if (generation != m_generation) { finish(note, "Notification service changed"); return; }
            const auto error = failure(info);
            if (!error.isEmpty()) { finish(note, error); return; }
            if (!current(note) || note.check) { finish(note); return; }
            sendNote(note, owner, generation);
        });
    });
}
void Notifications::sendNote(const Note &note, const QString &owner, quint64 generation) {
    const auto k = key(note.target);
    const uint replaces = note.test ? 0 : m_live.value(k).id;
    QStringList actions;
    if (!note.test) actions = {"default", "Show"};
    const QVariantMap hints{{"desktop-entry", "dev.shuv.rex-rune"}, {"urgency", QVariant::fromValue(uchar(note.kind == "blocked" || note.kind == "error" ? 2 : 1))}};
    call(this, bus(), owner, servicePath, serviceName, "Notify",
        {"Rex", replaces, "dev.shuv.rex-rune", note.title, note.body.toHtmlEscaped(), actions, hints, -1},
        [this, note, owner, generation, k](const QDBusMessage &reply) {
            if (generation != m_generation) { finish(note, "Notification service changed"); return; }
            const auto error = failure(reply);
            if (error.isEmpty() && !reply.arguments().isEmpty()) {
                const uint id = reply.arguments().first().toUInt();
                if (!note.test && id) {
                    if (current(note)) {
                        for (auto it = m_live.begin(); it != m_live.end();) {
                            if (it->id == id) { m_order.removeAll(it.key()); it = m_live.erase(it); }
                            else ++it;
                        }
                        m_order.removeAll(k); m_order.append(k);
                        m_live.insert(k, Live{id, note, false, {}});
                        if (m_order.size() > 64) m_live.remove(m_order.takeFirst());
                    } else call(this, bus(), owner, servicePath, serviceName, "CloseNotification", {id}, [](const QDBusMessage &) {});
                }
            }
            finish(note, error);
        });
}
void Notifications::finish(const Note &note, const QString &error) {
    m_service = error.isEmpty() ? "Notification service available" : error;
    if (note.test) m_test = error.isEmpty() ? "Test notification sent" : "Test failed: " + error;
    m_busy = false; emit changed(); pump();
}
void Notifications::forget(const QString &k) {
    m_serials.remove(k); m_order.removeAll(k);
    auto found = m_live.find(k);
    if (found == m_live.end()) return;
    const auto id = found->id; m_live.erase(found);
    const auto generation = m_generation;
    call(this, bus(), m_owner, servicePath, serviceName, "CloseNotification", {id}, [this, generation](const QDBusMessage &reply) {
        if (generation != m_generation) return;
        const auto error = failure(reply);
        if (!error.isEmpty()) { m_service = error; emit changed(); }
    });
}
void Notifications::clear() {
    const auto keys = m_live.keys(); for (const auto &k : keys) forget(k);
    m_rows.clear(); m_serials.clear(); m_pending.clear(); m_sounds.cancel();
}
void Notifications::consume(const QJsonObject &state) {
    const auto config = state["preferences"].toObject()["notifications"].toObject();
    const bool desktop = enabled(config, "system_notifications");
    const bool sounds = enabled(config, "play_sounds", false);
    if (!sounds) m_sounds.cancel();
    QHash<QString, QJsonObject> rows;
    for (const auto &value : state["activity"].toObject()["rows"].toArray()) {
        const auto row = value.toObject(); rows.insert(key(row), row);
    }
    for (const auto &value : state["notificationSessions"].toArray()) { const auto row = value.toObject(); rows.insert(key(row), row); }
    m_rows = rows;
    for (const auto &k : m_serials.keys()) {
        const auto row = rows.value(k);
        if (row.isEmpty() || row["seen"].toBool() || row["muted"].toBool()
            || (!desktop) || (m_live.contains(k) && row["activityEpoch"] != m_live[k].note.target["activityEpoch"])) forget(k);
    }
    for (const auto &row : rows) {
        const auto kind = row["newsState"].toString();
        if (!row["news"].toBool() || !enabled(config, kind, kind != "working") || kind == "idle") continue;
        if (sounds && (kind == "blocked" || kind == "error" || kind == "done")) m_sounds.offer(kind == "blocked" ? 3 : kind == "error" ? 2 : 1);
        if (!desktop) continue;
        const auto word = kind == "done" ? "Done" : kind == "working" ? "Working" : row["word"].toString();
        const auto place = row["sessionLabel"].toString() + " · " + row["hostLabel"].toString();
        const auto message = row["message"].toString();
        enqueue({row, row["label"].toString() + " · " + word, message.isEmpty() ? place : message + "\n" + place, kind});
    }
    if (desktop && enabled(config, "program_messages")) {
        for (const auto &value : state["programNotices"].toArray()) {
            const auto notice = value.toObject();
            const auto row = rows.value(key(notice));
            if (row.isEmpty() || row["seen"].toBool() || row["muted"].toBool() || row["activityEpoch"] != notice["activityEpoch"]) continue;
            const auto place = row["sessionLabel"].toString() + " · " + row["hostLabel"].toString();
            enqueue({row, notice["title"].toString().isEmpty() ? row["label"].toString() : notice["title"].toString(), notice["body"].toString() + "\n" + place, "program"});
        }
    }
}
void Notifications::action(uint id, const QString &actionName) {
    if (actionName != "default") return;
    for (auto it = m_live.begin(); it != m_live.end(); ++it) {
        if (it->id != id || it->clicked || !current(it->note)) continue;
        it->clicked = true;
        const auto target = it->note.target; const auto activation = it->token;
        forget(it.key()); emit activated(target.toVariantMap(), activation); return;
    }
}
void Notifications::closed(uint id, uint) { for (auto it = m_live.begin(); it != m_live.end();) { if (it->id == id) { m_order.removeAll(it.key()); it = m_live.erase(it); } else ++it; } }
void Notifications::token(uint id, const QString &value) { for (auto &live : m_live) if (live.id == id) live.token = value; }
