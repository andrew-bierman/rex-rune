#pragma once
#include <QObject>
#include <QJsonObject>
#include <QHash>
#include <QQueue>
#include <QDBusConnection>
#include "sounds.h"

class Notifications : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString service READ service NOTIFY changed)
    Q_PROPERTY(QString testResult READ testResult NOTIFY changed)
    Q_PROPERTY(QString soundHealth READ soundHealth NOTIFY changed)
public:
    explicit Notifications(QObject *parent = nullptr);
    ~Notifications() override;
    QString service() const { return m_service; }
    QString testResult() const { return m_test; }
    QString soundHealth() const { return m_sounds.health(); }
    void consume(const QJsonObject &state);
    void clear();
    Q_INVOKABLE void check();
    Q_INVOKABLE void test();
    Q_INVOKABLE void testSound();
    Q_INVOKABLE void checkSound();
signals:
    void changed();
    void activated(const QVariantMap &target, const QString &token);
private slots:
    void action(uint id, const QString &action);
    void closed(uint id, uint reason);
    void token(uint id, const QString &token);
    void ownerChanged(const QString &name, const QString &before, const QString &after);
private:
    struct Note { QJsonObject target; QString title, body; QString kind; quint64 serial = 0; bool test = false; bool check = false; };
    struct Live { uint id = 0; Note note; bool clicked = false; QString token; };
    void enqueue(Note note);
    void pump();
    void resolve(const Note &note);
    void sendNote(const Note &note, const QString &owner, quint64 generation);
    void finish(const Note &note, const QString &failure = {});
    void forget(const QString &key);
    void changeOwner(const QString &owner);
    bool current(const Note &note) const;
    static QString key(const QJsonObject &target);
    QDBusConnection bus();
    QDBusConnection m_bus{QString()};
    QString m_busName;
    QHash<QString, QJsonObject> m_rows;
    QHash<QString, Live> m_live;
    QStringList m_order;
    QHash<QString, quint64> m_serials;
    QQueue<Note> m_pending;
    Sounds m_sounds;
    QString m_service = "Not checked", m_test, m_owner;
    quint64 m_generation = 0, m_next = 0;
    bool m_busy = false, m_watching = false;
};
