#pragma once
#include <QObject>
#include <QThread>
#include <QMutex>
#include <QWaitCondition>
#include <chrono>

class Sounds : public QThread {
    Q_OBJECT
public:
    explicit Sounds(QObject *parent = nullptr);
    ~Sounds() override;
    QString health() const { return m_health; }
    void offer(int severity, bool preview = false);
    void cancel();
    void check();
signals:
    void changed();
    void result(const QString &health);
protected:
    void run() override;
private:
    using Clock = std::chrono::steady_clock;
    QMutex m_mutex;
    QWaitCondition m_wake;
    int m_pending = 0;
    bool m_preview = false, m_busy = false, m_check = false, m_stop = false;
    Clock::time_point m_due{}, m_next{};
    QString m_health = "For unseen tabs. Sounds are off by default.";
};
