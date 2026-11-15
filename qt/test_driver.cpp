#include "test_driver.h"
#include "frontend.h"
#include "terminal.h"

#include <QClipboard>
#include <QMimeData>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QDesktopServices>
#include <QUrl>
#include <QFile>
#include <QSaveFile>
#include <QGuiApplication>
#include <QInputMethodEvent>
#include <QJsonDocument>
#include <QKeyEvent>
#include <QQuickItem>
#include <QPointer>
#include <QQuickWindow>
#include <QTest>
#include <QTextStream>
#include <QWheelEvent>
#include <cmath>
#include <stdexcept>

namespace {
QQuickItem *controlItem(QQuickItem *root, const QString &name) {
    if (root->objectName() == name && root->isVisible()) return root;
    for (auto *child : root->childItems()) if (auto *item = controlItem(child, name)) return item;
    return nullptr;
}
TerminalItem *selectedTerminal(QQuickItem *root, const QString &id) {
    if (auto *terminal = qobject_cast<TerminalItem *>(root); terminal && terminal->paneId() == id) return terminal;
    for (auto *child : root->childItems()) if (auto *terminal = selectedTerminal(child, id)) return terminal;
    return nullptr;
}
void writeJson(const QString &path, const QJsonObject &value) {
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) throw std::runtime_error("cannot write artifact");
    const auto data = QJsonDocument(value).toJson();
    if (file.write(data) != data.size() || !file.commit()) throw std::runtime_error("cannot commit artifact");
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
    QKeyEvent unicode(QEvent::KeyPress, Qt::Key_Eacute, Qt::NoModifier, QString::fromUtf8("é"));
    check(terminalKey(unicode, "press")["key"] == "unidentified");
    check(terminalKey(unicode, "repeat")["text"] == QString::fromUtf8("é"));
    QKeyEvent unicodeRelease(QEvent::KeyRelease, Qt::Key_Eacute, Qt::NoModifier);
    check(terminalKey(unicodeRelease, "release")["key"] == "unidentified");
    check(terminalKey(unicodeRelease, "release")["text"] == "");
    QKeyEvent emoji(QEvent::KeyPress, Qt::Key_unknown, Qt::NoModifier, QString::fromUtf8("🙂"));
    check(terminalKey(emoji, "press")["text"] == QString::fromUtf8("🙂"));
    QKeyEvent modifier(QEvent::KeyPress, Qt::Key_Shift, Qt::ShiftModifier);
    check(terminalKey(modifier, "press").isEmpty());
}
}

TestDriver::TestDriver(QQuickWindow *window, Frontend *backend, const QString &script, const QString &report)
    : m_window(window), m_backend(backend), m_terminal(window->findChild<TerminalItem *>("terminal")), m_script(script), m_report(report) {
    for (const auto &scheme : {"https", "http", "ftp", "mailto"}) QDesktopServices::setUrlHandler(scheme, this, "openedUrl");
    m_timer.setInterval(100);
    connect(&m_timer, &QTimer::timeout, this, &TestDriver::step);
}

void TestDriver::openedUrl(const QUrl &url) { m_openedUrls << url.toString(); }

void TestDriver::start() {
    try {
        keyContract();

        QFile file(m_script);
        if (!file.open(QIODevice::ReadOnly)) throw std::runtime_error("cannot read test script");
        QJsonParseError error;
        const auto doc = QJsonDocument::fromJson(file.readAll(), &error);
        if (error.error != QJsonParseError::NoError || !doc.isArray()) throw std::runtime_error("script must be a JSON array");
        m_steps = doc.array();
        if (m_terminal) m_terminal->forceActiveFocus();
        m_window->requestActivate();
        m_timer.start();
    } catch (const std::exception &error) { finish(false, error.what()); }
}

void TestDriver::clickControl(const QString &name) {
    QPointer<QQuickItem> item = controlItem(m_window->contentItem(), name);
    if (!item) throw std::runtime_error("control not found");
    // Scroll clipped list delegates into view using the same wheel path as a person.
    for (int attempt = 0; attempt < 20; ++attempt) {
        if (!item) item = controlItem(m_window->contentItem(), name);
        if (!item) throw std::runtime_error("control disappeared while scrolling");
        auto *clip = item->parentItem();
        const auto center = item->mapToScene(QPointF(item->width() / 2, item->height() / 2));
        while (clip && (!clip->clip() || clip->mapRectToScene(clip->boundingRect()).contains(center))) clip = clip->parentItem();
        if (!clip) break;
        const auto rect = clip->mapRectToScene(clip->boundingRect());
        const auto at = rect.center();
        QWheelEvent wheel(at, m_window->mapToGlobal(at.toPoint()), {}, QPoint(0, center.y() > rect.bottom() ? -120 : 120),
                          Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
        QCoreApplication::sendEvent(m_window, &wheel);
        QTest::qWait(30);
        QPointer<QQuickItem> moving = clip;
        if (!QTest::qWaitFor([moving] { return !moving || !moving->property("moving").toBool(); }, 1500))
            throw std::runtime_error("control list did not stop scrolling");
    }
    if (!item) item = controlItem(m_window->contentItem(), name);
    if (!item) throw std::runtime_error("control disappeared after scrolling");
    const auto pos = item->mapToScene(QPointF(item->width() / 2, item->height() / 2)).toPoint();
    for (auto *clip = item->parentItem(); clip; clip = clip->parentItem())
        if (clip->clip() && !clip->mapRectToScene(clip->boundingRect()).contains(pos)) throw std::runtime_error("control is clipped");
    QTest::mouseClick(m_window, Qt::LeftButton, Qt::NoModifier, pos);
}

void TestDriver::step() {
    try {
        if (m_index >= m_steps.size()) { finish(true); return; }
        const auto pane = m_backend->snapshot()["pane"].toString();
        if (!pane.isEmpty()) m_lastPane = pane;
        m_terminal = selectedTerminal(m_window->contentItem(), pane);
        if (!m_terminal && !m_backend->snapshot()["connected"].toBool()) m_terminal = selectedTerminal(m_window->contentItem(), m_lastPane);
        const auto s = m_steps[m_index].toObject();
        if (!m_waiting) QTextStream(stderr) << "Qt test step " << m_index << ": " << s["op"].toString() << Qt::endl;
        const auto op = s["op"].toString();
        bool complete = true;
        if (!m_terminal && !QStringList{"waitState","waitText","waitProblem","pause","click","waitControl","screenshot","controlKey","field","snapshot","resize","serverConfirm"}.contains(op))
            throw std::runtime_error("selected terminal item missing");
        if (op == "waitText" || op == "waitState" || op == "waitClipboard" || op == "waitPrimary" || op == "waitProblem" || op == "waitOpenedUrl" || op == "waitControl") {
            if (!m_waiting) { m_wait.start(); m_waiting = true; }
            if (op == "waitText") complete = m_terminal && m_terminal->visibleText().contains(s["text"].toString());
            else if (op == "waitPrimary") complete = s["exact"].toBool() ? QGuiApplication::clipboard()->text(QClipboard::Selection) == s["text"].toString() : QGuiApplication::clipboard()->text(QClipboard::Selection).contains(s["text"].toString());
            else if (op == "waitClipboard") complete = s["exact"].toBool() ? QGuiApplication::clipboard()->text() == s["text"].toString() : QGuiApplication::clipboard()->text().contains(s["text"].toString());
            else if (op == "waitControl") {
                auto *object = m_window->findChild<QObject *>(s["control"].toString());
                if (!object) object = controlItem(m_window->contentItem(), s["control"].toString());
                complete = object && QJsonValue::fromVariant(object->property(s["property"].toString().toUtf8().constData())) == s["value"];
            }
            else if (op == "waitOpenedUrl") complete = m_openedUrls.contains(s["text"].toString());
            else if (op == "waitProblem") complete = (m_backend->problem() + '\n' + m_backend->hostFormProblem()).contains(s["text"].toString());
            else {
                QJsonValue value(m_backend->snapshot());
                for (const auto &part : s["field"].toString().split('.')) {
                    if (value.isArray()) { bool ok = false; const int index = part.toInt(&ok); const auto values = value.toArray(); value = ok && index >= 0 && index < values.size() ? values[index] : QJsonValue(); }
                    else value = value.toObject()[part];
                }
                complete = s.contains("tolerance") && value.isDouble() && s["value"].isDouble()
                    ? std::abs(value.toDouble() - s["value"].toDouble()) <= s["tolerance"].toDouble()
                    : value == s["value"];
            }
            if (!complete && m_wait.elapsed() > s["timeout"].toInt(15000)) throw std::runtime_error(("timeout at " + op + ": " + s["text"].toString() + s["field"].toString()).toStdString());
        } else if (op == "dragDivider") {
            auto *canvas = m_window->findChild<QQuickItem *>("terminalCanvas");
            const auto dividers = m_backend->snapshot()["dividers"].toArray();
            const int index = s["index"].toInt();
            if (!canvas || index < 0 || index >= dividers.size()) throw std::runtime_error("divider missing");
            const auto divider = dividers[index].toObject();
            const auto rect = divider["rect"].toArray();
            const bool horizontal = divider["horizontal"].toBool();
            const auto position = [&](double ratio) {
                const double x = (rect[0].toDouble() + rect[2].toDouble() * (horizontal ? ratio : .5)) * canvas->width();
                const double y = (rect[1].toDouble() + rect[3].toDouble() * (horizontal ? .5 : ratio)) * canvas->height();
                return canvas->mapToScene(QPointF(x, y)).toPoint();
            };
            QTest::mousePress(m_window, Qt::LeftButton, Qt::NoModifier, position(divider["ratio"].toDouble()));
            QTest::mouseMove(m_window, position(s["ratio"].toDouble()), 100);
            QTest::mouseRelease(m_window, Qt::LeftButton, Qt::NoModifier, position(s["ratio"].toDouble()));
        } else if (op == "clickPane") {
            const auto ids = m_backend->paneIds();
            const int index = s["index"].toInt();
            if (index < 0 || index >= ids.size()) throw std::runtime_error("pane index missing");
            auto *item = selectedTerminal(m_window->contentItem(), ids[index]);
            if (!item) throw std::runtime_error("visible pane item missing");
            QTest::mouseClick(m_window, Qt::LeftButton, Qt::NoModifier,
                item->mapToScene(QPointF(item->width() / 2, item->height() / 2)).toPoint());
        } else if (op == "find") {
            QMetaObject::invokeMethod(m_window, "find");
        } else if (op == "searchText") {
            auto *item = m_window->findChild<QQuickItem *>("terminalSearch");
            if (!item) throw std::runtime_error("search field missing");
            item->forceActiveFocus();
            for (QChar character : s["text"].toString()) QTest::keyClick(m_window, character.toLatin1(), Qt::NoModifier, 2);
        } else if (op == "searchPrevious" || op == "searchNext") {
            clickControl(op == "searchPrevious" ? "findPrevious" : "findNext");
        } else if (op == "searchKey") {
            QTest::keyClick(m_window, key(s["key"].toString()), modifiers(s["mods"].toString()));
        } else if (op == "serverConfirm") {
            m_backend->serverAction("confirm", quint64(s["request_id"].toInteger()));
        } else if (op == "click") {
            clickControl(s["control"].toString());
        } else if (op == "field") {
            auto *item = controlItem(m_window->contentItem(), s["control"].toString());
            if (!item) throw std::runtime_error("field not found");
            item->forceActiveFocus();
            QTest::keyClick(m_window, Qt::Key_A, Qt::ControlModifier);
            for (const auto character : s["text"].toString()) {
                if (character.unicode() > 127) throw std::runtime_error("field accepts ASCII");
                QTest::keyClick(m_window, character.toLatin1());
            }
        } else if (op == "controlKey") {
            QTest::keyClick(m_window, key(s["key"].toString()), modifiers(s["mods"].toString()));
        } else if (op == "wheel") {
            const auto pos = m_terminal->mapToScene(QPointF(m_terminal->gridLeft() + m_terminal->cellWidth() * (s["x"].toInt() + .5), m_terminal->gridTop() + m_terminal->cellHeight() * (s["y"].toInt() + .5)));
            QWheelEvent event(pos, m_window->mapToGlobal(pos.toPoint()), {}, QPoint(0, s["delta"].toInt()), Qt::NoButton,
                              modifiers(s["mods"].toString()), Qt::NoScrollPhase, false);
            QCoreApplication::sendEvent(m_window, &event);
        } else if (op == "pointer") {
            const auto pos = m_terminal->mapToScene(QPointF(m_terminal->gridLeft() + m_terminal->cellWidth() * (s["x"].toInt() + .5), m_terminal->gridTop() + m_terminal->cellHeight() * (s["y"].toInt() + .5))).toPoint();
            const auto mods = modifiers(s["mods"].toString());
            const auto button = s["button"] == "middle" ? Qt::MiddleButton : s["button"] == "right" ? Qt::RightButton :
                s["button"] == "back" ? Qt::BackButton : s["button"] == "forward" ? Qt::ForwardButton : Qt::LeftButton;
            if (s["phase"] == "click") QTest::mouseClick(m_window, button, mods, pos);
            else if (s["phase"] == "double" || s["phase"] == "triple") {
                QTest::mouseDClick(m_window, button, mods, pos);
                QTest::mouseRelease(m_window, button, mods, pos);
                if (s["phase"] == "triple") QTest::mouseClick(m_window, button, mods, pos, 20);
            } else if (s["phase"] == "down") QTest::mousePress(m_window, button, mods, pos);
            else if (s["phase"] == "up") QTest::mouseRelease(m_window, button, mods, pos);
            else if (s["phase"] == "move") QTest::mouseMove(m_window, pos);
            else throw std::runtime_error("unknown pointer phase");
        } else if (op == "type") {
            m_terminal->forceActiveFocus();
            const auto text = s["text"].toString();
            for (QChar character : text) {
                if (character.unicode() > 127) throw std::runtime_error("type accepts ASCII; use paste for Unicode");
                QTest::keyClick(m_window, character.toLatin1(), Qt::NoModifier, 2);
            }
        } else if (op == "key") {
            m_terminal->forceActiveFocus();
            const auto code = key(s["key"].toString());
            const auto mods = modifiers(s["mods"].toString());
            if (s.contains("text") || s.contains("action")) {
                const auto action = s["action"].toString();
                if (!action.isEmpty() && action != "press" && action != "repeat" && action != "release") throw std::runtime_error("unknown key action");
                QKeyEvent event(action == "release" ? QEvent::KeyRelease : QEvent::KeyPress,
                                code, mods, s["text"].toString(), action == "repeat");
                QCoreApplication::sendEvent(m_window, &event);
                if (action.isEmpty()) {
                    QKeyEvent release(QEvent::KeyRelease, code, mods);
                    QCoreApplication::sendEvent(m_window, &release);
                }
            } else QTest::keyClick(m_window, code, mods, 5);
        } else if (op == "ime") {
            m_terminal->forceActiveFocus();
            QInputMethodEvent event;
            event.setCommitString(s["text"].toString());
            QCoreApplication::sendEvent(m_terminal, &event);
        } else if (op == "setMimeClipboard" || op == "fileDrop") {
            auto *mime = new QMimeData;
            QList<QUrl> urls;
            for (const auto &path : s["paths"].toArray()) urls.append(QUrl::fromLocalFile(path.toString()));
            mime->setUrls(urls);
            if (op == "setMimeClipboard") QGuiApplication::clipboard()->setMimeData(mime);
            else {
                const auto point = QPointF(m_terminal->width() / 2, m_terminal->height() / 2);
                QDragEnterEvent enter(point.toPoint(), Qt::CopyAction, mime, Qt::LeftButton, Qt::NoModifier);
                QCoreApplication::sendEvent(m_terminal, &enter);
                QDropEvent drop(point, Qt::CopyAction, mime, Qt::LeftButton, Qt::NoModifier);
                QCoreApplication::sendEvent(m_terminal, &drop);
                delete mime;
                if (!drop.isAccepted()) throw std::runtime_error("native file drop was refused");
            }
        } else if (op == "paste") {
            QGuiApplication::clipboard()->setText(s["text"].toString());
            clickControl("pasteButton");
        } else if (op == "clipboardPolicy") {
            m_terminal->setProperty("copyOnSelection", s["copyOnSelection"].toBool());
            m_terminal->setProperty("primarySelection", s["primarySelection"].toBool());
        } else if (op == "windowVisible") {
            m_window->setVisible(s["visible"].toBool());
            if (s["visible"].toBool()) m_window->requestActivate();
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
        } else if (op == "selectAll" || op == "promptPrevious" || op == "promptNext") {
            const auto menu = m_window->findChild<QObject *>("actionsMenu");
            if (!menu) throw std::runtime_error("terminal menu not found");
            complete = false;
            if (!m_waiting) {
                clickControl("moreButton");
                m_wait.start();
                m_waiting = true;
            } else if (!m_menuActivated && menu->property("opened").toBool()) {
                clickControl(op == "selectAll" ? "selectAllButton" : op == "promptPrevious" ? "previousPromptButton" : "nextPromptButton");
                m_menuActivated = true;
            } else if (m_menuActivated && !menu->property("visible").toBool()) {
                complete = true;
                m_menuActivated = false;
            }
            if (m_wait.elapsed() > 5000) throw std::runtime_error("terminal menu did not open and close");
        } else if (op == "snapshot") {
            auto snapshot = m_backend->snapshot();
            snapshot["clipboard"] = QGuiApplication::clipboard()->text();
            if (m_terminal && m_terminal->actions()) {
                snapshot["shortcut_pending"] = m_terminal->actions()->pending();
                snapshot["shortcut_modes"] = QJsonArray::fromStringList(m_terminal->actions()->activeModes());
            }
            snapshot["supportsSelection"] = QGuiApplication::clipboard()->supportsSelection();
            snapshot["selectionText"] = QGuiApplication::clipboard()->text(QClipboard::Selection);
            snapshot["problem"] = m_backend->problem();
            QJsonArray panes;
            for (const auto &id : m_backend->paneIds()) {
                auto *item = selectedTerminal(m_window->contentItem(), id);
                if (!item) throw std::runtime_error("scene pane missing from visual tree");
                const auto at = item->mapToScene(QPointF(0, 0));
                panes.append(QJsonObject{{"pane",id},{"columns",item->columns()},{"rows",item->rows()},
                    {"x",at.x()},{"y",at.y()},{"width",item->width()},{"height",item->height()},
                    {"text",item->visibleText()}});
            }
            if (s.contains("control")) {
                const auto object = m_window->findChild<QObject *>(s["control"].toString());
                if (!object) throw std::runtime_error("snapshot control missing");
                QJsonObject control;
                for (const auto *property : {"x","y","width","height","padding","contentWidth","contentHeight"})
                    control[property] = QJsonValue::fromVariant(object->property(property));
                for (const auto &property : s["properties"].toArray())
                    control[property.toString()] = QJsonValue::fromVariant(object->property(property.toString().toUtf8().constData()));
                auto *content = qobject_cast<QQuickItem *>(object->property("contentItem").value<QObject *>());
                if (content) {
                    control["actualContentWidth"] = content->width();
                    double overflow = 0;
                    const auto inspect = [&](const auto &self, QQuickItem *item) -> void {
                        if (item->clip()) return;
                        for (auto *child : item->childItems()) {
                            if (!child->isVisible()) continue;
                            overflow = std::max(overflow, std::max(-child->x(), child->x() + child->width() - item->width()));
                            self(self, child);
                        }
                    };
                    inspect(inspect, content);
                    control["horizontalOverflow"] = overflow;
                }
                snapshot["control"] = control;
            }
            snapshot["openedUrls"] = QJsonArray::fromStringList(m_openedUrls);
            snapshot["qt_panes"] = panes;
            QJsonArray programIcons;
            const auto inspectIcons = [&](const auto &self, QQuickItem *item) -> void {
                if (!item->isVisible()) return;
                const auto icon = item->property("icon");
                if (icon.canConvert<QVariantMap>() && item->property("colored").isValid() && item->property("ink").isValid()) {
                    QString surface = "other";
                    for (auto *ancestor = item->parentItem(); ancestor; ancestor = ancestor->parentItem()) {
                        if (ancestor->objectName() == "tabs") { surface = "tabs"; break; }
                        if (ancestor->objectName() == "hostInventory") { surface = "sidebar"; break; }
                        if (ancestor->objectName() == "activityRows") { surface = "activity"; break; }
                        if (QByteArray(ancestor->metaObject()->className()).startsWith("PaneHeader")) { surface = "header"; break; }
                    }
                    QString ink;
                    for (auto *child : item->childItems()) {
                        if (child->property("color").canConvert<QColor>()) {
                            ink = child->property("color").value<QColor>().name();
                            break;
                        }
                    }
                    const auto origin = item->mapToScene(QPointF(0, 0));
                    programIcons.append(QJsonObject{{"surface",surface},{"icon",QJsonObject::fromVariantMap(icon.toMap())},
                        {"colored",item->property("colored").toBool()},{"color",ink},
                        {"x",origin.x()},{"y",origin.y()},{"width",item->width()},{"height",item->height()}});
                }
                for (auto *child : item->childItems()) self(self, child);
            };
            inspectIcons(inspectIcons, m_window->contentItem());
            snapshot["qt_program_icons"] = programIcons;
            snapshot["qt_columns"] = m_terminal ? m_terminal->columns() : 0;
            snapshot["qt_rows"] = m_terminal ? m_terminal->rows() : 0;
            snapshot["qt_grid_left"] = m_terminal ? m_terminal->gridLeft() : 0;
            snapshot["qt_grid_top"] = m_terminal ? m_terminal->gridTop() : 0;
            snapshot["qt_cell_width"] = m_terminal ? m_terminal->cellWidth() : 0;
            snapshot["qt_cell_height"] = m_terminal ? m_terminal->cellHeight() : 0;
            writeJson(s["path"].toString(), snapshot);
        } else if (op == "screenshot") {
            if (!m_window->grabWindow().save(s["path"].toString())) throw std::runtime_error("cannot save window image");
        } else if (op == "assertWideCursor") {
            const auto frame = m_backend->snapshot()["frame"].toObject();
            const auto cursor = frame["cursor"].toArray();
            if (cursor.size() != 2 || frame["cursor_shape"] != "block" || !m_window->isActive() || !m_terminal->hasActiveFocus()) throw std::runtime_error("active block cursor fixture missing");
            const int x = cursor[0].toInt(), y = cursor[1].toInt();
            const auto row = frame["cells"].toArray()[y].toArray();
            if (x + 1 >= row.size() || !row[x + 1].toObject()["spacer_tail"].toBool()) throw std::runtime_error("wide cursor fixture missing");
            const auto image = m_window->grabWindow();
            const auto origin = m_terminal->mapToScene(QPointF(0, 0));
            const auto expected = QColor::fromRgb(frame["cursor_color"].toInt());
            for (int offset = 0; offset < 2; ++offset) {
                const QPoint sample(qRound((origin.x() + m_terminal->gridLeft() + (x + offset + .5) * m_terminal->cellWidth()) * image.devicePixelRatio()),
                                    qRound((origin.y() + m_terminal->gridTop() + (y + 1) * m_terminal->cellHeight() - 2) * image.devicePixelRatio()));
                if (!image.rect().contains(sample) || image.pixelColor(sample) != expected) throw std::runtime_error("block cursor does not cover both wide cells");
            }
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
    if (!success) {
        if (m_terminal) report["visible_text"] = m_terminal->visibleText();
        auto state = m_backend->snapshot(); state.remove("server");
        report["state"] = state;
        report["problem"] = m_backend->problem();
        if (!m_report.isEmpty()) m_window->grabWindow().save(m_report + ".png");
    }
    try { if (!m_report.isEmpty()) writeJson(m_report, report); } catch (...) {}
    QTextStream(stdout) << QJsonDocument(report).toJson(QJsonDocument::Compact) << Qt::endl;
    QCoreApplication::exit(success ? 0 : 1);
}
