#include "terminal.h"

#include <QClipboard>
#include <QFontDatabase>
#include <QFontMetricsF>
#include <QGuiApplication>
#include <QInputMethodEvent>
#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
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
    setAcceptedMouseButtons(Qt::LeftButton);
    setActiveFocusOnTab(false);
    setClip(true);
    m_font = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    m_font.setStyleHint(QFont::Monospace);
    m_font.setPixelSize(16);
    m_font.setKerning(false);
    m_font.setFeature(QFont::Tag("liga"), 0);
    m_font.setFeature(QFont::Tag("calt"), 0);
    const QFontMetricsF metrics(m_font);
    m_cellWidth = std::ceil(metrics.horizontalAdvance('M'));
    m_cellHeight = std::ceil(metrics.height());
    m_ascent = metrics.ascent();
    connect(this, &QQuickItem::windowChanged, this, [this](QQuickWindow *window) {
        if (window) connect(window, &QWindow::activeChanged, this, [this] { foreground(); });
    });
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
    const auto target = QString::number(state["epoch"].toInteger()) + "/" + state["session"].toString() + '/' + state["window"].toString() + '/' + state["pane"].toString();
    if (target != m_target) {
        m_target = target;
        m_frame = {};
        m_pressed.clear();
        m_anchor = m_extent = -1;
        m_lastResize.clear();
    }
    if (state["frame"].isObject()) m_frame = state["frame"].toObject();
    else if (state["connected"].toBool()) m_frame = {};
    if (m_foregroundPending && window() && window()->isActive() && m_frontend->sendPane(m_paneId, "claim")) m_foregroundPending = false;
    resizeTerminal();
    emit textChanged();
    update();
}

int TerminalItem::columns() const { return std::clamp(int(width() / m_cellWidth), 1, 65535); }
int TerminalItem::rows() const { return std::clamp(int(height() / m_cellHeight), 1, 65535); }

void TerminalItem::resizeTerminal() {
    if (!m_frontend || !isVisible() || !window()) return;
    const auto terminal = m_state["terminal"].toObject();
    if (!terminal["connected"].toBool() || !terminal["owner"].toBool()) { m_lastResize.clear(); return; }
    const auto signature = m_target + ':' + QString::number(columns()) + 'x' + QString::number(rows());
    if (signature == m_lastResize) return;
    if (m_frontend->sendPane(m_paneId, "resize", {{"cols", columns()}, {"rows", rows()},
        {"cell_width", int(m_cellWidth)}, {"cell_height", int(m_cellHeight)}})) m_lastResize = signature;
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
    const auto cells = m_frame["cells"].toArray();
    const auto cols = m_frame["cols"].toInt();
    const auto cursor = m_frame["cursor"].toArray();
    const int cx = cursor.size() == 2 ? cursor[0].toInt() : -1;
    const int cy = cursor.size() == 2 ? cursor[1].toInt() : -1;
    int cursorCells = 1;
    if (cy >= 0 && cy < cells.size() && cx >= 0) {
        const auto row = cells[cy].toArray();
        if (cx + 1 < row.size() && row[cx + 1].toObject()["spacer_tail"].toBool()) cursorCells = 2;
    }
    const auto shape = m_frame["cursor_shape"].toString();
    const bool solidCursor = hasActiveFocus() && window() && window()->isActive() && shape == "block";
    const int first = std::min(m_anchor, m_extent), last = std::max(m_anchor, m_extent);
    for (int y = 0; y < cells.size() && y * m_cellHeight < height(); ++y) {
        const auto row = cells[y].toArray();
        for (int x = 0; x < row.size() && x * m_cellWidth < width(); ++x) {
            const auto cell = row[x].toObject();
            const QRectF rect(x * m_cellWidth, y * m_cellHeight, m_cellWidth, m_cellHeight);
            p->fillRect(rect, color(cell["bg"]));
            const int index = y * cols + x;
            if (first >= 0 && index >= first && index <= last) p->fillRect(rect, QColor(124, 196, 255, 95));
            if (y == cy && x >= cx && x < cx + cursorCells && solidCursor) p->fillRect(rect, color(m_frame["cursor_color"]));
        }
        for (int x = 0; x < row.size() && x * m_cellWidth < width(); ++x) {
            const auto cell = row[x].toObject();
            if (cell["spacer_tail"].toBool() || cell["invisible"].toBool()) continue;
            const bool wide = x + 1 < row.size() && row[x + 1].toObject()["spacer_tail"].toBool();
            const QRectF rect(x * m_cellWidth, y * m_cellHeight, m_cellWidth * (wide ? 2 : 1), m_cellHeight);
            QFont font = m_font;
            font.setBold(cell["bold"].toBool());
            font.setItalic(cell["italic"].toBool());
            p->save();
            p->setClipRect(rect);
            p->setFont(font);
            if (cell["faint"].toBool()) p->setOpacity(.5);
            const auto fg = x == cx && y == cy && solidCursor ? color(cell["bg"]) : color(cell["fg"]);
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
        QRectF cursorRect(cx * m_cellWidth, cy * m_cellHeight, m_cellWidth * cursorCells, m_cellHeight);
        p->setPen(color(m_frame["cursor_color"]));
        if (!hasActiveFocus() || !window() || !window()->isActive() || shape == "hollow") p->drawRect(cursorRect.adjusted(.5, .5, -.5, -.5));
        else if (shape == "bar") p->fillRect(QRectF(cursorRect.topLeft(), QSizeF(2, m_cellHeight)), color(m_frame["cursor_color"]));
        else if (shape == "underline") p->fillRect(QRectF(cursorRect.left(), cursorRect.bottom() - 2, m_cellWidth, 2), color(m_frame["cursor_color"]));
    }
}

bool TerminalItem::event(QEvent *event) {
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
    if (sendKey(event, event->isAutoRepeat() ? "repeat" : "press")) {
        m_anchor = m_extent = -1;
        update();
    }
    event->accept();
}
void TerminalItem::keyReleaseEvent(QKeyEvent *event) {
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
        return QRectF(cursor.size() == 2 ? cursor[0].toInt() * m_cellWidth : 0,
                      cursor.size() == 2 ? cursor[1].toInt() * m_cellHeight : 0, m_cellWidth, m_cellHeight);
    }
    return {};
}
int TerminalItem::cellAt(const QPointF &point) const {
    const auto cols = m_frame["cols"].toInt();
    const auto rows = m_frame["rows"].toInt();
    if (cols <= 0 || rows <= 0) return -1;
    return std::clamp(int(point.y() / m_cellHeight), 0, rows - 1) * cols + std::clamp(int(point.x() / m_cellWidth), 0, cols - 1);
}
void TerminalItem::mousePressEvent(QMouseEvent *event) {
    forceActiveFocus(Qt::MouseFocusReason);
    m_anchor = m_extent = cellAt(event->position());
    m_dragging = true;
    update();
    event->accept();
}
void TerminalItem::mouseMoveEvent(QMouseEvent *event) {
    if (m_dragging) {
        const auto extent = cellAt(event->position());
        if (extent != m_extent) {
            m_extent = extent;
            update();
        }
    }
    event->accept();
}
void TerminalItem::mouseReleaseEvent(QMouseEvent *event) { m_dragging = false; event->accept(); }
void TerminalItem::selectAll() {
    m_anchor = 0;
    m_extent = m_frame["cols"].toInt() * m_frame["rows"].toInt() - 1;
    update();
}
QString TerminalItem::selectedText() const {
    if (m_anchor < 0 || m_extent < 0) return {};
    const int cols = m_frame["cols"].toInt();
    if (cols <= 0) return {};
    const int first = std::min(m_anchor, m_extent), last = std::max(m_anchor, m_extent);
    const auto cells = m_frame["cells"].toArray();
    const auto wrapped = m_frame["wrapped"].toArray();
    QString result;
    for (int y = first / cols; y <= last / cols && y < cells.size(); ++y) {
        const auto row = cells[y].toArray();
        QString line;
        int start = y == first / cols ? first % cols : 0;
        if (start > 0 && start < row.size() && row[start].toObject()["spacer_tail"].toBool()) --start;
        const int end = y == last / cols ? last % cols : cols - 1;
        for (int x = start; x <= end && x < row.size(); ++x) if (!row[x].toObject()["spacer_tail"].toBool()) line += cellText(row[x].toObject());
        const bool continues = y < wrapped.size() && wrapped[y].toBool();
        if (end == cols - 1 && !continues) while (line.endsWith(' ')) line.chop(1);
        result += line;
        if (y != last / cols && !continues) result += '\n';
    }
    return result;
}
void TerminalItem::copy() { const auto text = selectedText(); if (!text.isEmpty()) QGuiApplication::clipboard()->setText(text); }
void TerminalItem::paste() {
    if (m_frontend) m_frontend->sendPane(m_paneId, "paste", {{"text", QGuiApplication::clipboard()->text()}});
    forceActiveFocus();
}
