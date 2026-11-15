#pragma once

#include <QJsonObject>
#include <QObject>
#include <QProcess>
#include <QVariantMap>
#include "notifications.h"

class Frontend : public QObject {
    Q_OBJECT
    Q_PROPERTY(Notifications *notifications READ notifications CONSTANT)
    Q_PROPERTY(QVariantMap state READ state NOTIFY stateChanged)
    Q_PROPERTY(QVariantMap activityState READ activityState NOTIFY activityChanged)
    Q_PROPERTY(QVariantMap preferences READ preferences NOTIFY preferencesChanged)
    Q_PROPERTY(QVariantMap hosts READ hosts NOTIFY hostsChanged)
    Q_PROPERTY(QStringList paneIds READ paneIds NOTIFY paneIdsChanged)
    Q_PROPERTY(QString preferencesProblem READ preferencesProblem NOTIFY preferencesProblemChanged)
    Q_PROPERTY(QString problem READ problem NOTIFY problemChanged)
    Q_PROPERTY(QString hostFormProblem READ hostFormProblem NOTIFY hostFormProblemChanged)
public:
    explicit Frontend(QObject *parent = nullptr);
    ~Frontend() override;
    Notifications *notifications() { return &m_notifications; }
    QVariantMap state() const { return m_state.toVariantMap(); }
    QVariantMap activityState() const { return m_state["activity"].toObject().toVariantMap(); }
    QVariantMap preferences() const { return m_state["preferences"].toObject().toVariantMap(); }
    QVariantMap hosts() const { return m_state["hosts"].toObject().toVariantMap(); }
    QStringList paneIds() const { return m_paneIds; }
    QJsonObject paneSnapshot(const QString &id) const;
    bool sendPane(const QString &id, const QString &op, QJsonObject fields = {});
    const QJsonObject &snapshot() const { return m_state; }
    QString preferencesProblem() const { return m_preferencesProblem; }
    QString problem() const { return m_problem; }
    QString hostFormProblem() const { return m_hostFormProblem; }
    void start(const QString &program, const QString &endpoint, const QString &session, const QString &config = {});
    bool sendTarget(const QString &op, QJsonObject fields = {});
    Q_INVOKABLE void selectSession(const QString &id);
    Q_INVOKABLE void selectWindow(const QString &id);
    Q_INVOKABLE void selectPane(const QString &id);
    Q_INVOKABLE void refresh();
    Q_INVOKABLE void serverAction(const QString &action, quint64 requestId = 0, const QString &text = {});
    Q_INVOKABLE void reloadShortcuts();
    Q_INVOKABLE quint64 savePreferences(const QVariantMap &patch);
    Q_INVOKABLE void recordShortcut(const QString &action, const QStringList &keys, const QVariantMap &args, const QString &mode, const QString &operation);
    Q_INVOKABLE void performAction(const QString &action, const QVariantMap &args, const QString &origin);
    Q_INVOKABLE void fileAction(const QString &op, const QVariantMap &fields, const QVariantMap &captured);
    Q_INVOKABLE void workspaceAction(const QString &action, const QVariantMap &args, const QVariantMap &captured);
    Q_INVOKABLE void activityAction(const QString &action, const QVariantMap &args = {});
    Q_INVOKABLE void hostAction(const QString &action, const QVariantMap &args = {}, const QVariantMap &captured = {});
    Q_INVOKABLE void setVisible(bool visible);
    Q_INVOKABLE void dismissProblem();
    Q_INVOKABLE void dismissPreferencesProblem();
    Q_INVOKABLE void dismissHostFormProblem();
    void reportProblem(const QString &message);
signals:
    void notificationActivated(const QString &token);
    void stateChanged();
    void hostsChanged();
    void activityChanged();
    void preferencesChanged();
    void paneIdsChanged();
    void problemChanged();
    void hostFormProblemChanged();
    void actionRequested(const QString &action, const QVariantMap &args);
    void hostSaved();
    void preferencesSaved(quint64 requestId);
    void preferencesRejected(quint64 requestId);
    void preferencesProblemChanged();
private:
    bool send(QJsonObject message);
    void read();
    void fail(const QString &message);
    Notifications m_notifications;
    QProcess m_process;
    QByteArray m_buffer;
    QJsonObject m_state;
    QStringList m_paneIds;
    QString m_problem;
    QString m_preferencesProblem;
    quint64 m_preferenceRequest = 0;
    QString m_hostFormProblem;
    bool m_hello = false;
    bool m_visible = false;
    bool m_stopping = false;
};
