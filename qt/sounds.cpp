#include "sounds.h"
#include <QProcess>
#include <QFileInfo>
#include <QDir>
#include <QStandardPaths>
#include <QMutexLocker>

Sounds::Sounds(QObject *parent) : QThread(parent) {
    connect(this, &Sounds::result, this, [this](const QString &health) { m_health = health; emit changed(); });
}
Sounds::~Sounds() {
    { QMutexLocker lock(&m_mutex); m_stop = true; m_wake.wakeOne(); }
    wait(2000);
}
void Sounds::offer(int severity, bool preview) {
    QMutexLocker lock(&m_mutex);
    const auto now = Clock::now();
    if (m_busy || (!preview && now < m_next)) return;
    if (preview) { m_pending = 1; m_preview = true; m_due = now; }
    else if (!m_pending) { m_pending = severity; m_due = now + std::chrono::milliseconds(100); }
    else if (!m_preview) m_pending = qMax(severity, m_pending);
    if (!isRunning()) start();
    m_wake.wakeOne();
}
void Sounds::cancel() { QMutexLocker lock(&m_mutex); if (!m_preview) m_pending = 0; }
void Sounds::check() { QMutexLocker lock(&m_mutex); m_check = true; if (!isRunning()) start(); m_wake.wakeOne(); }
void Sounds::run() {
    QString lastHealth;
    while (true) {
        int severity = 0; bool checking = false;
        {
            QMutexLocker lock(&m_mutex);
            while (!m_stop) {
                const auto now = Clock::now();
                if (m_check) { checking = true; m_check = false; break; }
                if (m_pending && now >= m_due) {
                    if (now <= m_due + std::chrono::seconds(1)) severity = m_pending;
                    m_pending = 0; m_preview = false;
                    if (severity) { m_busy = true; m_next = now + std::chrono::seconds(1); break; }
                }
                if (m_pending) m_wake.wait(&m_mutex, qMax<qint64>(1, std::chrono::duration_cast<std::chrono::milliseconds>(m_due - now).count()));
                else m_wake.wait(&m_mutex);
            }
            if (m_stop) return;
        }
        const QString sound = severity == 3 ? "dialog-warning" : severity == 2 ? "dialog-error" : "complete";
        QString program; QStringList args;
        const auto paths = qEnvironmentVariable("PATH").split(':');
        for (const auto &name : {"canberra-gtk-play", "pw-play", "paplay"}) {
            for (const auto &path : paths) {
                if (!QDir::isAbsolutePath(path)) continue;
                const QFileInfo candidate(QDir(path).filePath(name));
                if (!candidate.isFile() || !candidate.isExecutable()) continue;
                if (QString(name) == "canberra-gtk-play") args = {"-i", sound};
                else {
                    auto dirs = QStandardPaths::standardLocations(QStandardPaths::GenericDataLocation);
                    for (const auto &dir : dirs) {
                        const auto file = QDir(dir).filePath("sounds/freedesktop/stereo/" + sound + ".oga");
                        if (QFileInfo(file).isFile()) { args = {file}; break; }
                    }
                    if (args.isEmpty()) continue;
                }
                program = candidate.absoluteFilePath(); break;
            }
            if (!program.isEmpty()) break;
        }
        QString health = program.isEmpty() ? "No sound player or sound files found." : "Sound player available.";
        if (!program.isEmpty() && !checking) {
            QProcess child;
            child.setStandardInputFile(QProcess::nullDevice());
            child.setStandardOutputFile(QProcess::nullDevice());
            child.setStandardErrorFile(QProcess::nullDevice());
            child.start(program, args);
            if (!child.waitForStarted(500)) health = "The last sound could not be played.";
            else {
                const auto until = Clock::now() + std::chrono::seconds(3);
                bool stopped = false;
                while (!child.waitForFinished(20)) {
                    { QMutexLocker lock(&m_mutex); stopped = m_stop; }
                    if (stopped || Clock::now() >= until) { child.kill(); child.waitForFinished(500); health = "The last sound did not finish and was stopped."; break; }
                }
                if (child.state() == QProcess::NotRunning && health == "Sound player available." && (child.exitStatus() != QProcess::NormalExit || child.exitCode())) health = "The last sound could not be played.";
            }
        }
        { QMutexLocker lock(&m_mutex); m_busy = false; }
        if (checking && health == "Sound player available." && lastHealth.startsWith("The last sound")) health = lastHealth;
        lastHealth = health;
        emit result(health);
    }
}
