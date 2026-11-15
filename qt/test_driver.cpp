#include "test_driver.h"
#include "frontend.h"
#include "terminal.h"

#include <QClipboard>
#include <QFile>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QKeyEvent>
#include <QQuickItem>
#include <QQuickWindow>
#include <QTest>
#include <QTextStream>
#include <stdexcept>

namespace {
void writeJson(const QString &path, const QJsonObject &value) {
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly)) throw std::runtime_error("cannot write artifact");
    file.write(QJsonDocument(value).toJson());
}
Qt::Key key(const QString &name) {
    static const QHash<QString, Qt::Key> keys = {{"Return",Qt::Key_Return}, {"Enter",Qt::Key_Return},
        {"Escape",Qt::Key_Escape}, {"Tab",Qt::Key_Tab}, {"Backspace",Qt::Key_Backspace},
        {"Delete",Qt::Key_Delete}, {"Up",Qt::Key_Up}, {"Down",Qt::Key_Down},
        {"Left",Qt::Key_Left}, {"Right",Qt::Key_Right}, {"Home",Qt::Key_Home},
        {"End",Qt::Key_End}, {"PageUp",Qt::Key_PageUp}, {"PageDown",Qt::Key_PageDown},
        {"Space",Qt::Key_Space}};
    if (keys.contains(name)) return keys[name];
    if (name.startsWith('F')) {
        bool ok; int n = name.mid(1).toInt(&ok);
        if (ok && n >= 1 && n <= 35) return Qt::Key(Qt::Key_F1 + n - 1);
    }
    if (name.size() == 1 && name[0].isLetterOrNumber()) return Qt::Key(name[0].toUpper().unicode());
    throw std::runtime_error("unknown test key");
}
Qt::KeyboardModifiers modifiers(const QString &text) {
    Qt::KeyboardModifiers result;
    for (const auto &name : text.split('+', Qt::SkipEmptyParts)) {
        if (name == "ctrl") result |= Qt::ControlModifier;
        else if (name == "shift") result |= Qt::ShiftModifier;
        else if (name == "alt") result |= Qt::AltModifier;
        else if (name == "super") result |= Qt::MetaModifier;
        else throw std::runtime_error("unknown test modifier");
    }
    return result;
}
void keyContract() {
    const auto check = [](bool pass) { if (!pass) throw std::runtime_error("key payload contract failed"); };
    QKeyEvent enter(QEvent::KeyPress, Qt::Key_Return, Qt::NoModifier, "\r");
    check(terminalKey(enter, "press")["text"] == "\n");
    QKeyEvent ctrl(QEvent::KeyPress, Qt::Key_C, Qt::ControlModifier, QString(QChar(3)));
    const auto payload = terminalKey(ctrl, "press");
    check(payload["key"] == "key_c" && payload["mods"] == "ctrl" && payload["text"] == "");
    QKeyEvent tab(QEvent::KeyPress, Qt::Key_Tab, Qt::NoModifier, "\t");
    check(terminalKey(tab, "press")["text"] == "\t");
    QKeyEvent shift(QEvent::KeyPress, Qt::Key_A, Qt::ShiftModifier, "A");
    check(terminalKey(shift, "press")["consumed_mods"] == "shift");
    check(terminalKey(shift, "release")["text"] == "");
    QKeyEvent arrow(QEvent::KeyPress, Qt::Key_Left, Qt::AltModifier);
    check(terminalKey(arrow, "press")["key"] == "arrow_left");
}
}

TestDriver::TestDriver(QQuickWindow *window, Frontend *backend, const QString &script, const QString &report)
    : m_window(window), m_backend(backend), m_terminal(window->findChild<TerminalItem *>("terminal")), m_script(script), m_report(report) {
    m_timer.setInterval(100);
    connect(&m_timer, &QTimer::timeout, this, &TestDriver::step);
}

void TestDriver::start() {
    try {
        keyContract();
        if (!m_terminal) throw std::runtime_error("terminal item missing");
        QFile file(m_script);
        if (!file.open(QIODevice::ReadOnly)) throw std::runtime_error("cannot read test script");
        QJsonParseError error;
        const auto doc = QJsonDocument::fromJson(file.readAll(), &error);
        if (error.error != QJsonParseError::NoError || !doc.isArray()) throw std::runtime_error("script must be a JSON array");
        m_steps = doc.array();
        m_terminal->forceActiveFocus();
        m_window->requestActivate();
        m_timer.start();
    } catch (const std::exception &error) { finish(false, error.what()); }
}

void TestDriver::clickControl(const QString &name) {
    const auto item = m_window->findChild<QQuickItem *>(name);
    if (!item) throw std::runtime_error("control not found");
    const auto pos = item->mapToScene(QPointF(item->width() / 2, item->height() / 2)).toPoint();
    QTest::mouseClick(m_window, Qt::LeftButton, Qt::NoModifier, pos);
}

void TestDriver::step() {
    try {
        if (m_index >= m_steps.size()) { finish(true); return; }
        const auto s = m_steps[m_index].toObject();
        const auto op = s["op"].toString();
        bool complete = true;
        if (op == "waitText" || op == "waitState" || op == "waitClipboard") {
            if (!m_waiting) { m_wait.start(); m_waiting = true; }
            if (op == "waitText") complete = m_terminal->visibleText().contains(s["text"].toString());
            else if (op == "waitClipboard") complete = QGuiApplication::clipboard()->text().contains(s["text"].toString());
            else {
                QJsonValue value(m_backend->snapshot());
                for (const auto &part : s["field"].toString().split('.')) value = value.toObject()[part];
                complete = value == s["value"];
            }
            if (!complete && m_wait.elapsed() > s["timeout"].toInt(15000)) throw std::runtime_error(("timeout at " + op + ": " + s["text"].toString() + s["field"].toString()).toStdString());
        } else if (op == "type") {
            m_terminal->forceActiveFocus();
            const auto text = s["text"].toString();
            for (QChar character : text) {
                if (character.unicode() > 127) throw std::runtime_error("type accepts ASCII; use paste for Unicode");
                QTest::keyClick(m_window, character.toLatin1(), Qt::NoModifier, 2);
            }
        } else if (op == "key") {
            m_terminal->forceActiveFocus();
            QTest::keyClick(m_window, key(s["key"].toString()), modifiers(s["mods"].toString()), 5);
        } else if (op == "paste") {
            QGuiApplication::clipboard()->setText(s["text"].toString());
            m_terminal->forceActiveFocus();
            QTest::keyClick(m_window, Qt::Key_V, Qt::ControlModifier | Qt::ShiftModifier);
        } else if (op == "resize") {
            m_window->resize(s["width"].toInt(), s["height"].toInt());
        } else if (op == "select") {
            const auto item = m_window->findChild<QQuickItem *>(s["control"].toString());
            if (!item) throw std::runtime_error("selector not found");
            item->forceActiveFocus();
            QTest::keyClick(m_window, Qt::Key_Space);
            QTest::keyClick(m_window, Qt::Key_Home);
            for (int i = 0; i < s["index"].toInt(); ++i) QTest::keyClick(m_window, Qt::Key_Down);
            QTest::keyClick(m_window, Qt::Key_Return);
        } else if (op == "copy") {
            clickControl("copyButton");
        } else if (op == "selectAll") {
            const auto menu = m_window->findChild<QObject *>("actionsMenu");
            if (!menu) throw std::runtime_error("terminal menu not found");
            complete = false;
            if (!m_waiting) {
                clickControl("moreButton");
                m_wait.start();
                m_waiting = true;
            } else if (!m_menuActivated && menu->property("opened").toBool()) {
                clickControl("selectAllButton");
                m_menuActivated = true;
            } else if (m_menuActivated && !menu->property("visible").toBool()) {
                complete = true;
                m_menuActivated = false;
            }
            if (m_wait.elapsed() > 5000) throw std::runtime_error("terminal menu did not open and close");
        } else if (op == "snapshot") {
            auto snapshot = m_backend->snapshot();
            snapshot["clipboard"] = QGuiApplication::clipboard()->text();
            snapshot["qt_columns"] = m_terminal->columns();
            snapshot["qt_rows"] = m_terminal->rows();
            snapshot["qt_cell_width"] = m_terminal->cellWidth();
            snapshot["qt_cell_height"] = m_terminal->cellHeight();
            writeJson(s["path"].toString(), snapshot);
        } else if (op == "screenshot") {
            if (!m_window->grabWindow().save(s["path"].toString())) throw std::runtime_error("cannot save window image");
        } else if (op == "pause") {
            if (!m_waiting) { m_wait.start(); m_waiting = true; }
            complete = m_wait.elapsed() >= s["ms"].toInt(200);
        } else throw std::runtime_error("unknown script operation");
        if (complete) {
            m_evidence.append(QJsonObject{{"step", m_index}, {"op", op}, {"passed", true}});
            ++m_index;
            m_waiting = false;
        }
    } catch (const std::exception &error) { finish(false, error.what()); }
}

void TestDriver::finish(bool success, const QString &message) {
    m_timer.stop();
    QJsonObject report{{"success", success}, {"step", m_index}, {"message", message}, {"evidence", m_evidence}};
    if (!success && m_terminal) report["visible_text"] = m_terminal->visibleText();
    try { if (!m_report.isEmpty()) writeJson(m_report, report); } catch (...) {}
    QTextStream(stdout) << QJsonDocument(report).toJson(QJsonDocument::Compact) << Qt::endl;
    QCoreApplication::exit(success ? 0 : 1);
}
