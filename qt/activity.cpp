#include "activity.h"
#include <algorithm>
#include <QMap>

QVariantList ActivityModel::rows() const {
    QVariantList result;
    for (const auto &value : m_inventory.value("rows").toList()) {
        const auto row = value.toMap();
        if (!row.value("listed").toBool()) continue;
        const QString haystack = row.value("hostLabel").toString() + " " + row.value("sessionLabel").toString()
            + " " + row.value("label").toString() + " " + row.value("word").toString() + " " + row.value("message").toString() + " " + row.value("groupName").toString();
        if (haystack.contains(m_filter, Qt::CaseInsensitive)) result.append(row);
    }
    QMap<QString, int> sessionRanks;
    const auto sessionKey = [](const QVariantMap &r) { return r.value("host").toString() + QChar(0x1f) + r.value("session").toString(); };
    for (const auto &v : result) {
        const auto row = v.toMap();
        const auto key = sessionKey(row);
        const int rank = row.value("rank").toInt();
        sessionRanks[key] = sessionRanks.contains(key) ? std::min(sessionRanks[key], rank) : rank;
    }
    // Session groups sort by their most pressing row, preserving host/session input
    // order among equal priorities. Rows within each session keep priority/tab order.
    QStringList order;
    for (const auto &v : result) { const auto k = sessionKey(v.toMap()); if (!order.contains(k)) order.append(k); }
    std::stable_sort(order.begin(), order.end(), [&](const QString &a, const QString &b) { return sessionRanks[a] < sessionRanks[b]; });
    std::stable_sort(result.begin(), result.end(), [&](const QVariant &a, const QVariant &b) {
        const auto x = a.toMap(), y = b.toMap();
        if (m_sessionView && sessionKey(x) != sessionKey(y)) return order.indexOf(sessionKey(x)) < order.indexOf(sessionKey(y));
        return x.value("rank").toInt() < y.value("rank").toInt();
    });
    QString previous;
    for (auto &value : result) {
        auto row = value.toMap();
        const QString key = m_sessionView ? sessionKey(row) : row.value("groupName").toString();
        const QString group = m_sessionView ? row.value("hostLabel").toString() + " · " + row.value("sessionLabel").toString()
            : row.value("groupName").toString();
        row.insert("group", group);
        row.insert("showGroup", key != previous);
        previous = key;
        value = row;
    }
    return result;
}
int ActivityModel::chipCount() const {
    int waiting = 0, errors = 0;
    for (const auto &v : m_inventory.value("rows").toList()) {
        const auto row = v.toMap();
        if (row.value("inFront").toBool() || !row.value("eligible").toBool()) continue;
        if (row.value("state").toString() == "blocked") ++waiting;
        if (row.value("state").toString() == "error") ++errors;
    }
    return waiting ? waiting : errors;
}
void ActivityModel::reconcile() {
    const auto list = rows();
    for (const auto &v : list) if (v.toMap().value("key").toString() == m_selected) return;
    m_selected = list.isEmpty() ? QString() : list.first().toMap().value("key").toString();
}
void ActivityModel::setInventory(const QVariantMap &value) { m_inventory = value; if (value.contains("view")) m_sessionView = value.value("view").toString() == "session"; reconcile(); emit changed(); }
void ActivityModel::setFilter(const QString &value) { if (m_filter == value) return; m_filter = value; reconcile(); emit changed(); }
void ActivityModel::setSessionView(bool value) { if (m_sessionView == value) return; m_sessionView = value; reconcile(); emit changed(); }
void ActivityModel::setSelectedKey(const QString &value) { if (m_selected == value) return; m_selected = value; reconcile(); emit changed(); }
void ActivityModel::step(int delta) {
    const auto list = rows();
    if (list.isEmpty()) return;
    int at = 0;
    for (int i = 0; i < list.size(); ++i) if (list[i].toMap().value("key").toString() == m_selected) { at = i; break; }
    at = (at + delta % list.size() + list.size()) % list.size();
    setSelectedKey(list[at].toMap().value("key").toString());
}
void ActivityModel::activateSelected() {
    for (const auto &v : rows()) {
        const auto row = v.toMap();
        if (row.value("key").toString() == m_selected) { emit activate(row); return; }
    }
}
