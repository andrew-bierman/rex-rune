#pragma once
#include <QJsonObject>
#include <QJsonArray>
#include <QKeyEvent>
#include <QObject>
#include <QVariantList>
#include <QHash>

class Actions : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantList commands READ commands NOTIFY changed)
    Q_PROPERTY(QVariantList bindings READ bindings NOTIFY changed)
    Q_PROPERTY(QString pending READ pending NOTIFY changed)
    Q_PROPERTY(QStringList activeModes READ activeModes NOTIFY changed)
    Q_PROPERTY(bool modalOpen READ modalOpen WRITE setModalOpen NOTIFY changed)
    Q_PROPERTY(bool searching READ searching WRITE setSearching NOTIFY changed)
    Q_PROPERTY(bool recording READ recording NOTIFY changed)
    Q_PROPERTY(QStringList recordedKeys READ recordedKeys NOTIFY changed)
public:
    explicit Actions(QObject *parent = nullptr);
    void setSnapshot(const QJsonObject &state);
    bool handleKey(QKeyEvent *event, bool globalOnly = false);
    QVariantList commands() const;
    QVariantList bindings() const;
    QString pending() const { return m_pending; }
    QStringList activeModes() const;
    bool modalOpen() const { return m_modal; }
    void setModalOpen(bool value);
    bool searching() const { return m_searching; }
    void setSearching(bool value) { if (m_searching != value) { m_searching = value; emit changed(); } }
    bool recording() const { return m_recording; }
    QStringList recordedKeys() const { return m_recorded; }
    Q_INVOKABLE void run(const QString &id, const QVariantMap &args = {}, const QString &origin = "palette");
    Q_INVOKABLE void beginRecording();
    Q_INVOKABLE void stopRecording();
    Q_INVOKABLE QString hint(const QString &id) const;
signals:
    void keypress();
    void changed();
    void requested(const QString &id, const QVariantMap &args, const QString &origin);
    void failed(const QString &message);
private:
    bool eventFilter(QObject *object, QEvent *event) override;
    bool available(const QJsonObject &binding, bool globalOnly) const;
    void clearSequence();
    QJsonObject m_state;
    QJsonArray m_commands;
    QJsonArray m_bindings;
    QJsonArray m_modes;
    QVector<int> m_candidates;
    QHash<quint64, bool> m_consumed;
    QList<QPair<QString, bool>> m_activeModes;
    int m_depth = 0;
    QString m_pending;
    QString m_context;
    bool m_modal = false;
    bool m_recording = false;
    bool m_searching = false;
    QStringList m_recorded;
};
