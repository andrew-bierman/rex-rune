#pragma once

#include <QJsonObject>
#include <QObject>
#include <QProcess>
#include <QVariantMap>

class Frontend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap state READ state NOTIFY stateChanged)
    Q_PROPERTY(QStringList paneIds READ paneIds NOTIFY paneIdsChanged)
    Q_PROPERTY(QString problem READ problem NOTIFY problemChanged)
public:
    explicit Frontend(QObject *parent = nullptr);
    ~Frontend() override;
    QVariantMap state() const { return m_state.toVariantMap(); }
    QStringList paneIds() const { return m_paneIds; }
    QJsonObject paneSnapshot(const QString &id) const;
    bool sendPane(const QString &id, const QString &op, QJsonObject fields = {});
    const QJsonObject &snapshot() const { return m_state; }
    QString problem() const { return m_problem; }
    void start(const QString &program, const QString &endpoint, const QString &session);
    bool sendTarget(const QString &op, QJsonObject fields = {});
    Q_INVOKABLE void selectSession(const QString &id);
    Q_INVOKABLE void selectWindow(const QString &id);
    Q_INVOKABLE void selectPane(const QString &id);
    Q_INVOKABLE void refresh();
    Q_INVOKABLE void dismissProblem();
signals:
    void stateChanged();
    void paneIdsChanged();
    void problemChanged();
private:
    bool send(QJsonObject message);
    void read();
    void fail(const QString &message);
    QProcess m_process;
    QByteArray m_buffer;
    QJsonObject m_state;
    QStringList m_paneIds;
    QString m_problem;
    bool m_hello = false;
    bool m_stopping = false;
};
