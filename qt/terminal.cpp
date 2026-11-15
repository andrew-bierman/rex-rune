#include "terminal.h"
#include "settings.h"

#include <QClipboard>
#include <QDesktopServices>
#include <QUrl>
#include <QStyleHints>
#include <QFontDatabase>
#include <QFontMetricsF>
#include <QGuiApplication>
#include <QInputMethodEvent>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QHoverEvent>
#include <QWheelEvent>
#include <QPainter>
#include <QPainterPath>
#include <QQuickWindow>
#include <algorithm>
#include <cmath>

namespace {
QColor color(const QJsonValue &value) { return QColor::fromRgb(value.toInt() & 0xffffff); }
QString cellText(const QJsonObject &cell) {
    const auto text = cell["text"].toString();
    return text.isEmpty() ? QStringLiteral(" ") : text;
}
QString keyName(int key) {
    if (key >= Qt::Key_A && key <= Qt::Key_Z) return "key_" + QString(QChar('a' + key - Qt::Key_A));
    if (key >= Qt::Key_0 && key <= Qt::Key_9) return "digit_" + QString(QChar('0' + key - Qt::Key_0));
    if (key >= Qt::Key_F1 && key <= Qt::Key_F35) return "f" + QString::number(key - Qt::Key_F1 + 1);
    switch (key) {
    case Qt::Key_Return: case Qt::Key_Enter: return "enter";
    case Qt::Key_Tab: case Qt::Key_Backtab: return "tab";
    case Qt::Key_Backspace: return "backspace";
    case Qt::Key_Delete: return "delete";
    case Qt::Key_Insert: return "insert";
    case Qt::Key_Escape: return "escape";
    case Qt::Key_Left: return "arrow_left";
    case Qt::Key_Right: return "arrow_right";
    case Qt::Key_Up: return "arrow_up";
    case Qt::Key_Down: return "arrow_down";
    case Qt::Key_Home: return "home";
    case Qt::Key_End: return "end";
    case Qt::Key_PageUp: return "page_up";
    case Qt::Key_PageDown: return "page_down";
    case Qt::Key_Space: return "space";
    case Qt::Key_Minus: case Qt::Key_Underscore: return "minus";
    case Qt::Key_Equal: case Qt::Key_Plus: return "equal";
    case Qt::Key_BracketLeft: case Qt::Key_BraceLeft: return "bracket_left";
    case Qt::Key_BracketRight: case Qt::Key_BraceRight: return "bracket_right";
    case Qt::Key_Backslash: case Qt::Key_Bar: return "backslash";
    case Qt::Key_Semicolon: case Qt::Key_Colon: return "semicolon";
    case Qt::Key_Apostrophe: case Qt::Key_QuoteDbl: return "quote";
    case Qt::Key_Comma: case Qt::Key_Less: return "comma";
    case Qt::Key_Period: case Qt::Key_Greater: return "period";
    case Qt::Key_Slash: case Qt::Key_Question: return "slash";
    case Qt::Key_QuoteLeft: case Qt::Key_AsciiTilde: return "backquote";
    case Qt::Key_Exclam: return "digit_1";
    case Qt::Key_At: return "digit_2";
    case Qt::Key_NumberSign: return "digit_3";
    case Qt::Key_Dollar: return "digit_4";
    case Qt::Key_Percent: return "digit_5";
    case Qt::Key_AsciiCircum: return "digit_6";
    case Qt::Key_Ampersand: return "digit_7";
    case Qt::Key_Asterisk: return "digit_8";
    case Qt::Key_ParenLeft: return "digit_9";
    case Qt::Key_ParenRight: return "digit_0";
    default: return {};
    }
}
}

QJsonObject terminalKey(const QKeyEvent &event, const QString &action) {
    auto key = keyName(event.key());
    if (key.isEmpty()) {
        const auto codepoints = event.text().toUcs4();
        if (action != "release" && !std::any_of(codepoints.begin(), codepoints.end(), [](char32_t c) { return QChar::isPrint(c); })) return {};
        key = "unidentified";
    }
    const auto mods = event.modifiers();
    QStringList names;
    if (mods.testFlag(Qt::ShiftModifier)) names << "shift";
    if (mods.testFlag(Qt::ControlModifier)) names << "ctrl";
    if (mods.testFlag(Qt::AltModifier)) names << "alt";
    if (mods.testFlag(Qt::MetaModifier)) names << "super";
    QString text;
    if (action != "release" && !mods.testFlag(Qt::ControlModifier) && !mods.testFlag(Qt::MetaModifier)) {
        text = event.text();
        if (event.key() == Qt::Key_Return || event.key() == Qt::Key_Enter) text = "\n";
        if (event.key() == Qt::Key_Tab || event.key() == Qt::Key_Backtab) text = "\t";
    }
    const bool consumed = mods.testFlag(Qt::ShiftModifier) && !text.isEmpty() && text != " " && key != "tab" && key != "enter";
    uint base = 0;
    if (event.key() >= Qt::Key_A && event.key() <= Qt::Key_Z) base = 'a' + event.key() - Qt::Key_A;
    else if (event.key() >= Qt::Key_0 && event.key() <= Qt::Key_9) base = '0' + event.key() - Qt::Key_0;
    else if (!mods.testFlag(Qt::ShiftModifier)) {
        const auto codepoints = event.text().toUcs4();
        if (codepoints.size() == 1 && QChar::isPrint(codepoints[0])) base = codepoints[0];
    }
    return {{"key", key}, {"mods", names.join('+')}, {"consumed_mods", consumed ? "shift" : ""},
            {"text", text}, {"unshifted_codepoint", int(base)}, {"composing", false}, {"action", action}};
}

TerminalItem::TerminalItem(QQuickItem *parent) : QQuickPaintedItem(parent) {
    setFlag(ItemAcceptsInputMethod);
    setAcceptedMouseButtons(Qt::LeftButton | Qt::MiddleButton | Qt::RightButton);
    setActiveFocusOnTab(false);
    setClip(true);
    setAcceptHoverEvents(true);
    setPreferences({});
    connect(this, &QQuickItem::windowChanged, this, [this](QQuickWindow *window) {
        if (window) {
            connect(window, &QWindow::activeChanged, this, [this] { foreground(); });
            connect(window, &QWindow::screenChanged, this, [this] { m_lastResize.clear();resizeTerminal(); });
        }
    });
}

void TerminalItem::setPreferences(const QVariantMap &preferences) {
    if (m_preferences == preferences && !m_font.family().isEmpty()) return;
    m_preferences = preferences;
    const auto metrics = terminalMetrics(preferences);
    m_font=metrics.font; m_cellWidth=metrics.width; m_cellHeight=metrics.height; m_ascent=metrics.ascent;
    m_paddingX=std::clamp(preferences.value("pane_padding",8).toDouble(),0.,32.);
    const auto interface=preferences.value("interface").toMap();
    m_paddingY=std::clamp(interface.value("pane_padding_vertical",m_paddingX).toDouble(),0.,32.);
    const auto terminal=preferences.value("terminal").toMap();
    m_copyOnSelection=terminal.value("copy_on_selection",false).toBool();
    m_primarySelection=terminal.value("primary_selection",false).toBool();
    m_lastResize.clear();resizeTerminal();emit preferencesChanged();update();
}

void TerminalItem::setFrontend(Frontend *frontend) {
    if (m_frontend == frontend) return;
    if (m_frontend) disconnect(m_frontend, nullptr, this, nullptr);
    m_frontend = frontend;
    if (m_frontend) connect(m_frontend, &Frontend::stateChanged, this, &TerminalItem::updateState);
    emit frontendChanged();
    updateState();
}

void TerminalItem::setPaneId(const QString &id) {
    if (id == m_paneId) return;
    m_paneId = id;
    emit paneIdChanged();
    updateState();
}

void TerminalItem::updateState() {
    if (!m_frontend) return;
    const auto state = m_frontend->paneSnapshot(m_paneId);
    if (state == m_state && !m_foregroundPending) return;
    m_state = state;
    const auto target = state["host"].toString() + "/" + QString::number(state["epoch"].toInteger()) + "/" + state["session"].toString() + '/' + state["window"].toString() + '/' + state["pane"].toString() + "/" + QString::number(state["terminal"].toObject()["generation"].toInteger());
    if (target != m_target) {
        m_target = target;
        m_frame = {};
        m_copyRevision = m_selectionRevision = 0;
        m_dragging = m_primaryCopy = m_reporting = false;
        m_mouseButton.clear();
        m_linkUri.clear();
        m_clickClock.invalidate();
        m_pressed.clear();
        m_lastResize.clear();
    }
    if (state["frame"].isObject()) m_frame = state["frame"].toObject();
    else if (state["connected"].toBool()) m_frame = {};
    if (m_foregroundPending && window() && window()->isActive() && m_frontend->sendPane(m_paneId, "claim")) m_foregroundPending = false;
    if (m_copyRevision && m_frame["selection_revision"].toInteger() >= qint64(m_copyRevision)) finishCopy();
    resizeTerminal();
    emit textChanged();
    update();
}

int TerminalItem::columns() const { return std::clamp(int(std::max(qreal(0),width() - 2*m_paddingX) / m_cellWidth), 1, 65535); }
int TerminalItem::rows() const { return std::clamp(int(std::max(qreal(0),height() - 2*m_paddingY) / m_cellHeight), 1, 65535); }

void TerminalItem::resizeTerminal() {
    if (!m_frontend || !isVisible() || !window()) return;
    const auto terminal = m_state["terminal"].toObject();
    if (!terminal["connected"].toBool() || !terminal["owner"].toBool()) { m_lastResize.clear(); return; }
    const auto signature = m_target + ':' + QString::number(columns()) + 'x' + QString::number(rows()) + ':' + QString::number(m_cellWidth) + ':' + QString::number(m_cellHeight) + ':' + QString::number(window()->devicePixelRatio());
    if (signature == m_lastResize) return;
    if (m_frontend->sendPane(m_paneId, "resize", {{"cols", columns()}, {"rows", rows()},
        {"cell_width", qRound(m_cellWidth*window()->devicePixelRatio())}, {"cell_height", qRound(m_cellHeight*window()->devicePixelRatio())}})) m_lastResize = signature;
}

void TerminalItem::foreground() {
    if (!window() || !window()->isActive()) return;
    m_foregroundPending = true;
    if (m_frontend && m_frontend->sendPane(m_paneId, "claim")) m_foregroundPending = false;
}

void TerminalItem::geometryChange(const QRectF &now, const QRectF &old) {
    QQuickPaintedItem::geometryChange(now, old);
    resizeTerminal();
}

QString TerminalItem::visibleText() const {
    QStringList lines;
    for (const auto &rowValue : m_frame["cells"].toArray()) {
        QString line;
        for (const auto &cell : rowValue.toArray()) if (!cell.toObject()["spacer_tail"].toBool()) line += cellText(cell.toObject());
        lines << line;
    }
    return lines.join('\n');
}

void TerminalItem::paint(QPainter *p) {
    p->fillRect(boundingRect(), m_frame.isEmpty() ? QColor("#001528") : color(m_frame["bg"]));
    if (m_frame.isEmpty()) return;
    p->save();
    p->setClipRect(QRectF(m_paddingX,m_paddingY,std::max(qreal(0),width()-2*m_paddingX),std::max(qreal(0),height()-2*m_paddingY)));
    const auto cells = m_frame["cells"].toArray();
    const auto selectionRows = m_frame["selection_rows"].toArray();
    const auto selectedAt = [&selectionRows](int x, int y) {
        if (y < 0 || y >= selectionRows.size() || !selectionRows[y].isArray()) return false;
        const auto range = selectionRows[y].toArray();
        return range.size() == 2 && x >= range[0].toInt() && x <= range[1].toInt();
    };
    const auto search = m_frame["search"].toObject();
    const auto matches = search["matches"].toArray();
    const auto current = search["selected"].toObject();
    const auto contains = [](const QJsonArray &start, const QJsonArray &end, int x, int y) {
        return start.size() == 2 && end.size() == 2 && y >= start[1].toInt() && y <= end[1].toInt()
            && (y != start[1].toInt() || x >= start[0].toInt()) && (y != end[1].toInt() || x <= end[0].toInt());
    };
    const auto currentStart = current["start"].toArray(), currentEnd = current["end"].toArray();
    const auto currentAt = [&](int x, int y) {
        return contains(currentStart, currentEnd, x, y);
    };
    const auto cursor = m_frame["cursor"].toArray();
    const int cx = cursor.size() == 2 ? cursor[0].toInt() : -1;
    const int cy = cursor.size() == 2 ? cursor[1].toInt() : -1;
    int cursorCells = 1;
    if (cy >= 0 && cy < cells.size() && cx >= 0) {
        const auto row = cells[cy].toArray();
        if (cx + 1 < row.size() && row[cx + 1].toObject()["spacer_tail"].toBool()) cursorCells = 2;
    }
    const auto terminalConfig=m_preferences.value("terminal").toMap();
    const auto shape = terminalConfig.value("cursor_force").toBool() ? terminalConfig.value("cursor_style","block").toString() : m_frame["cursor_shape"].toString();
    const bool solidCursor = hasActiveFocus() && window() && window()->isActive() && shape == "block";

    for (int y = 0; y < cells.size() && y * m_cellHeight < height(); ++y) {
        const auto row = cells[y].toArray();
        for (int x = 0; x < row.size() && x * m_cellWidth < width(); ++x) {
            const auto cell = row[x].toObject();
            const QRectF rect(m_paddingX + x * m_cellWidth, m_paddingY + y * m_cellHeight, m_cellWidth, m_cellHeight);
            p->fillRect(rect, color(cell["bg"]));

            for (const auto &value : matches) {
                const auto match = value.toObject();
                const auto start = match["start"].toArray(), end = match["end"].toArray();
                if (!selectedAt(x, y) && contains(start, end, x, y))
                    {
                    const bool isCurrent = match == current;
                    p->fillRect(rect, QColor(124, 196, 255, isCurrent ? 204 : 45));
                    p->setPen(QColor(124, 196, 255, 180)); p->drawRect(rect.adjusted(.5, .5, -.5, -.5));
                }
            }
            if (selectedAt(x, y)) {
                const auto fill=selectionColors(m_preferences,color(cell["fg"]),color(cell["bg"])).first;
                if (!terminalConfig.value("selection_rounded").toBool()) p->fillRect(rect,fill);
                else {
                    // Only the outside of the continuous selection receives rounded corners.
                    const qreal r=std::min(qreal(3),m_cellWidth/2);
                    QPainterPath path;path.setFillRule(Qt::WindingFill);path.addRoundedRect(rect,r,r);
                    if(selectedAt(x-1,y)) path.addRect(QRectF(rect.left(),rect.top(),r,rect.height()));
                    if(selectedAt(x+1,y)) path.addRect(QRectF(rect.right()-r,rect.top(),r,rect.height()));
                    if(selectedAt(x,y-1)) path.addRect(QRectF(rect.left(),rect.top(),rect.width(),r));
                    if(selectedAt(x,y+1)) path.addRect(QRectF(rect.left(),rect.bottom()-r,rect.width(),r));
                    p->fillPath(path.simplified(),fill);
                }
            }
            if (y == cy && x >= cx && x < cx + cursorCells && solidCursor) p->fillRect(rect, color(m_frame["cursor_color"]));
        }
        for (int x = 0; x < row.size() && x * m_cellWidth < width(); ++x) {
            const auto cell = row[x].toObject();
            if (cell["spacer_tail"].toBool() || cell["invisible"].toBool()) continue;
            const bool wide = x + 1 < row.size() && row[x + 1].toObject()["spacer_tail"].toBool();
            const QRectF rect(m_paddingX + x * m_cellWidth, m_paddingY + y * m_cellHeight, m_cellWidth * (wide ? 2 : 1), m_cellHeight);
            QFont font = m_font;
            const auto program=terminalConfig.value("font").toMap().value("bold_italic","both").toString();
            font.setBold(m_font.bold() || (cell["bold"].toBool() && (program=="both" || program=="bold_only")));
            font.setItalic(m_font.italic() || (cell["italic"].toBool() && (program=="both" || program=="italic_only")));
            p->save();
            p->setClipRect(rect);
            p->setFont(font);
            if (cell["faint"].toBool()) p->setOpacity(.5);
            const auto cursorText=m_preferences.value("_appearance").toMap().value("cursor_text");
            const auto fg = x == cx && y == cy && solidCursor ? (cursorText.isNull() ? color(cell["bg"]) : QColor::fromRgb(cursorText.toUInt()))
                : selectedAt(x,y) ? selectionColors(m_preferences,color(cell["fg"]),color(cell["bg"])).second
                : currentAt(x, y) ? QColor("#001528") : color(cell["fg"]);
            p->setPen(fg);
            p->drawText(QPointF(rect.x(), rect.y() + m_ascent), cell["text"].toString());
            const auto underline = cell["underline"].toString();
            p->setPen(cell["underline_color"].isDouble() ? color(cell["underline_color"]) : fg);
            if (underline != "none" && !underline.isEmpty()) p->drawLine(QPointF(rect.left(), rect.bottom() - 2), QPointF(rect.right(), rect.bottom() - 2));
            if (underline == "double") p->drawLine(QPointF(rect.left(), rect.bottom() - 4), QPointF(rect.right(), rect.bottom() - 4));
            if (cell["strikethrough"].toBool()) p->drawLine(QPointF(rect.left(), rect.top() + m_ascent / 2), QPointF(rect.right(), rect.top() + m_ascent / 2));
            if (cell["overline"].toBool()) p->drawLine(rect.topLeft(), rect.topRight());
            p->restore();
        }
    }
    if (cx >= 0 && cy >= 0 && !solidCursor) {
        QRectF cursorRect(m_paddingX + cx * m_cellWidth, m_paddingY + cy * m_cellHeight, m_cellWidth * cursorCells, m_cellHeight);
        p->setPen(color(m_frame["cursor_color"]));
        if (!hasActiveFocus() || !window() || !window()->isActive() || shape == "hollow") p->drawRect(cursorRect.adjusted(.5, .5, -.5, -.5));
        else if (shape == "bar") p->fillRect(QRectF(cursorRect.topLeft(), QSizeF(2, m_cellHeight)), color(m_frame["cursor_color"]));
        else if (shape == "underline") p->fillRect(QRectF(cursorRect.left(), cursorRect.bottom() - 2, m_cellWidth, 2), color(m_frame["cursor_color"]));
    }
    p->restore();
}

bool TerminalItem::event(QEvent *event) {
    if (event->type()==QEvent::DevicePixelRatioChange) { m_lastResize.clear();resizeTerminal(); }
    if (event->type() == QEvent::ShortcutOverride) { event->accept(); return true; }
    // QQuickItem normally treats Tab as focus traversal before keyPressEvent.
    if (event->type() == QEvent::KeyPress) { keyPressEvent(static_cast<QKeyEvent *>(event)); return true; }
    if (event->type() == QEvent::KeyRelease) { keyReleaseEvent(static_cast<QKeyEvent *>(event)); return true; }
    return QQuickPaintedItem::event(event);
}

bool TerminalItem::sendKey(QKeyEvent *event, const QString &action) {
    auto payload = terminalKey(*event, action);
    if (action == "release") {
        const auto press = m_pressed.constFind(event->key());
        if (press == m_pressed.cend()) return false;
        payload["key"] = (*press)["key"];
        payload["unshifted_codepoint"] = (*press)["unshifted_codepoint"];
    }
    if (payload.isEmpty() || !m_frontend || !m_frontend->sendPane(m_paneId, "key", {{"payload", payload}})) return false;
    if (action == "press") m_pressed.insert(event->key(), payload);
    return true;
}
void TerminalItem::keyPressEvent(QKeyEvent *event) {
    if (m_actions && (m_actions->handleKey(event) || m_actions->modalOpen())) { event->accept(); return; }
    if (sendKey(event, event->isAutoRepeat() ? "repeat" : "press")) {
        selection("clear", {});
        update();
    }
    event->accept();
}
void TerminalItem::keyReleaseEvent(QKeyEvent *event) {
    if (m_actions && m_actions->handleKey(event) && !m_pressed.contains(event->key())) { event->accept(); return; }
    if (!event->isAutoRepeat()) {
        sendKey(event, "release");
        m_pressed.remove(event->key());
    }
    event->accept();
}
void TerminalItem::focusInEvent(QFocusEvent *event) {
    QQuickPaintedItem::focusInEvent(event);
    if (m_frontend && m_frontend->snapshot()["pane"].toString() != m_paneId) m_frontend->selectPane(m_paneId);
    update();
}
void TerminalItem::focusOutEvent(QFocusEvent *event) {
    m_linkUri.clear();
    if (m_dragging) selection("up", m_selectionPosition);
    m_dragging = false;
    m_copyRevision = 0;
    m_pressed.clear();
    QQuickPaintedItem::focusOutEvent(event);
    update();
}
void TerminalItem::inputMethodEvent(QInputMethodEvent *event) {
    if (!event->commitString().isEmpty() && m_frontend) m_frontend->sendPane(m_paneId, "paste", {{"text", event->commitString()}});
    event->accept();
}
QVariant TerminalItem::inputMethodQuery(Qt::InputMethodQuery query) const {
    if (query == Qt::ImEnabled) return true;
    if (query == Qt::ImCursorRectangle) {
        const auto cursor = m_frame["cursor"].toArray();
        return QRectF(cursor.size() == 2 ? m_paddingX + cursor[0].toInt() * m_cellWidth : m_paddingX,
                      cursor.size() == 2 ? m_paddingY + cursor[1].toInt() * m_cellHeight : m_paddingY, m_cellWidth, m_cellHeight);
    }
    return {};
}
int TerminalItem::cellAt(const QPointF &point) const {
    const auto cols = m_frame["cols"].toInt();
    const auto rows = m_frame["rows"].toInt();
    if (cols <= 0 || rows <= 0) return -1;
    return std::clamp(int(std::floor((point.y()-m_paddingY) / m_cellHeight)), 0, rows - 1) * cols + std::clamp(int(std::floor((point.x()-m_paddingX) / m_cellWidth)), 0, cols - 1);
}
void TerminalItem::selection(const QString &phase, const QPointF &point, int clicks) {
    const int at = std::max(0, cellAt(point)), cols = std::max(1, m_frame["cols"].toInt());
    m_selectionPosition = point;
    ++m_selectionRevision;
    m_copyRevision = 0;
    m_primaryCopy = false;
    if (m_frontend) m_frontend->sendPane(m_paneId, "selection", {
        {"phase", phase}, {"clicks", clicks}, {"revision", qint64(m_selectionRevision)},
        {"generation", m_state["terminal"].toObject()["generation"]},
        {"x", at % cols}, {"y", at / cols}, {"px", point.x()-m_paddingX}, {"py", point.y()-m_paddingY},
        {"columns", cols}, {"cell_width", int(std::ceil(m_cellWidth))}, {"height", int(std::max(qreal(0),height()-2*m_paddingY))}});
}
bool TerminalItem::pointer(QMouseEvent *event, const QString &action) {
    if (!m_frame["mouse_tracking"].toBool() || ((event->modifiers() & Qt::ShiftModifier) && event->button() != Qt::RightButton && !m_reporting)) return false;
    const int at = std::max(0, cellAt(event->position())), cols = std::max(1, m_frame["cols"].toInt());
    QStringList mods;
    if (event->modifiers() & Qt::ControlModifier) mods.append("ctrl");
    if (event->modifiers() & Qt::AltModifier) mods.append("alt");
    if (event->modifiers() & Qt::MetaModifier) mods.append("super");
    const auto button = event->button() == Qt::LeftButton ? "left" : event->button() == Qt::RightButton ? "right" : "middle";
    QJsonObject payload{{"action", action}, {"x", at % cols}, {"y", at / cols}, {"mods", mods.join("+")}};
    if (action == "press") { m_reporting = true; m_mouseButton = button; }
    if (m_reporting) payload["button"] = m_mouseButton;
    if (action == "release") { m_reporting = false; m_mouseButton.clear(); }
    if (m_frontend) m_frontend->sendPane(m_paneId, "mouse", {{"payload", payload}});
    return true;
}
void TerminalItem::mousePressEvent(QMouseEvent *event) {
    forceActiveFocus(Qt::MouseFocusReason);
    if (event->button() == Qt::RightButton && !(event->modifiers() & Qt::ShiftModifier)) {
        emit contextMenuRequested(event->position().x(), event->position().y());
        event->accept(); return;
    }
    if (event->button() == Qt::MiddleButton && m_primarySelection && (!m_frame["mouse_tracking"].toBool() || (event->modifiers() & Qt::ShiftModifier)) && QGuiApplication::clipboard()->supportsSelection()) {
        if (m_frontend) m_frontend->sendPane(m_paneId, "paste", {{"text", QGuiApplication::clipboard()->text(QClipboard::Selection)}});
        event->accept(); return;
    }
    if (event->button() == Qt::LeftButton && (event->modifiers() & Qt::ControlModifier) && !(event->modifiers() & Qt::ShiftModifier)) {
        const auto at = cellAt(event->position());
        for (const auto &value : m_frame["links"].toArray()) {
            const auto link = value.toObject();
            if (at >= link["start"].toInt() && at <= link["end"].toInt()) {
                m_linkUri = link["uri"].toString();
                m_linkTarget = m_target;
                m_linkStart = link["start"].toInt();
                m_linkEnd = link["end"].toInt();
                event->accept(); return;
            }
        }
    }
    if (!pointer(event, "press") && event->button() == Qt::LeftButton) {
        if (m_clickClock.isValid() && m_clickClock.elapsed() < QGuiApplication::styleHints()->mouseDoubleClickInterval()
            && (event->position() - m_clickPosition).manhattanLength() < 5) m_clicks = std::min(3, m_clicks + 1);
        else m_clicks = 1;
        m_clickClock.restart(); m_clickPosition = event->position();
        selection("down", event->position(), m_clicks); m_dragging = true;
    }
    event->accept();
}
void TerminalItem::mouseDoubleClickEvent(QMouseEvent *event) {
    forceActiveFocus(Qt::MouseFocusReason);
    if (!pointer(event, "press") && event->button() == Qt::LeftButton) {
        m_clicks = 2; m_clickClock.restart(); m_clickPosition = event->position();
        selection("down", event->position(), 2); m_dragging = true;
    }
    event->accept();
}
void TerminalItem::mouseMoveEvent(QMouseEvent *event) { if (!m_linkUri.isEmpty()) { event->accept(); return; } if (!m_dragging) pointer(event, "motion"); if (m_dragging) selection("move", event->position()); event->accept(); }
void TerminalItem::mouseReleaseEvent(QMouseEvent *event) {
    if (!m_linkUri.isEmpty()) {
        const auto uri = m_linkUri;
        m_linkUri.clear();
        if (event->button() == Qt::LeftButton && (event->modifiers() & Qt::ControlModifier) &&
            !(event->modifiers() & Qt::ShiftModifier) && m_target == m_linkTarget) {
            const int at = cellAt(event->position());
            for (const auto &value : m_frame["links"].toArray()) {
                const auto link = value.toObject();
                if (link["uri"].toString() == uri && link["start"].toInt() == m_linkStart && link["end"].toInt() == m_linkEnd && at >= m_linkStart && at <= m_linkEnd) {
                    if (!QDesktopServices::openUrl(QUrl(uri)) && m_frontend) m_frontend->reportProblem("Could not open link");
                    break;
                }
            }
        }
        event->accept(); return;
    }
    if (!m_dragging && m_reporting) pointer(event, "release");
    if (m_dragging) {
        selection("up", event->position());
        if (m_copyOnSelection || m_primarySelection) { m_copyRevision = m_selectionRevision; m_primaryCopy = m_primarySelection; }
    }
    m_dragging = false; event->accept();
}
void TerminalItem::wheelEvent(QWheelEvent *event) {
    if (m_frontend) {
        m_scrollFraction -= event->pixelDelta().isNull() ? event->angleDelta().y() / 120.0 * QGuiApplication::styleHints()->wheelScrollLines() : event->pixelDelta().y() / m_cellHeight;
        const int lines = int(m_scrollFraction);
        m_scrollFraction -= lines;
        if (m_frame["mouse_tracking"].toBool() && !(event->modifiers() & Qt::ShiftModifier)) {
            const int at = std::max(0, cellAt(event->position())), cols = std::max(1, m_frame["cols"].toInt());
            for (int i = 0; i < std::min(8, std::abs(lines)); ++i)
                m_frontend->sendPane(m_paneId, "wheel", {{"payload", QJsonObject{{"x", at % cols}, {"y", at / cols}, {"delta_x", 0}, {"delta_y", lines < 0 ? -1 : 1}}}});
        } else m_frontend->sendPane(m_paneId, "scroll", {{"lines", lines}});
    }
    event->accept();
}
void TerminalItem::selectAll() { selection("all", {}); }
QString TerminalItem::selectedText() const { return m_frame["selection_text"].toString(); }
void TerminalItem::copy() {
    m_primaryCopy = false;
    m_copyRevision = m_selectionRevision;
    finishCopy();
}
void TerminalItem::finishCopy() {
    if (!m_frontend || m_frontend->snapshot()["pane"].toString() != m_paneId || !window() || !window()->isActive() || !window()->isVisible()) { m_copyRevision = 0; return; }
    if (m_frame["selection_revision"].toInteger() >= qint64(m_copyRevision)) {
        const auto text = selectedText();
        if (!text.isEmpty() && m_frame["selection_error"].isNull()) {
            if (!m_primaryCopy || m_copyOnSelection) QGuiApplication::clipboard()->setText(text);
            if (m_primaryCopy && QGuiApplication::clipboard()->supportsSelection()) QGuiApplication::clipboard()->setText(text, QClipboard::Selection);
        }
        if (!m_frame["selection_error"].toString().isEmpty()) m_frontend->reportProblem(m_frame["selection_error"].toString());
        m_primaryCopy = false;
        m_copyRevision = 0;
    }
}
void TerminalItem::paste() {
    if (m_frontend) m_frontend->sendPane(m_paneId, "paste", {{"text", QGuiApplication::clipboard()->text()}});
    forceActiveFocus();
}

void TerminalItem::search(const QString &text) { if (m_frontend) m_frontend->sendPane(m_paneId, "search", {{"text", text}}); }
void TerminalItem::searchStep(bool older) { if (m_frontend) m_frontend->sendPane(m_paneId, "search_step", {{"older", older}}); }
void TerminalItem::promptStep(bool older) { if (m_frontend) m_frontend->sendPane(m_paneId, "prompt_step", {{"older", older}}); }

void TerminalItem::hoverMoveEvent(QHoverEvent *event) {
    QMouseEvent motion(QEvent::MouseMove, event->position(), event->globalPosition(), Qt::NoButton, Qt::NoButton, event->modifiers());
    pointer(&motion, "motion");
    event->accept();
}

void TerminalItem::scroll(int lines) { if (m_frontend) m_frontend->sendPane(m_paneId, "scroll", {{"lines", lines}}); }
void TerminalItem::reconnect() { if (m_frontend) m_frontend->sendPane(m_paneId, "reconnect"); }

void TerminalItem::copyId() { QGuiApplication::clipboard()->setText(m_paneId); }
void TerminalItem::clear() { if (m_frontend) m_frontend->sendPane(m_paneId, "clear"); }
void TerminalItem::reset() { if (m_frontend) m_frontend->sendPane(m_paneId, "reset"); }
