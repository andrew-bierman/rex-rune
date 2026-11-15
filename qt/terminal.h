#pragma once

#include "frontend.h"
#include <QFont>
#include <QJsonArray>
#include <QQuickPaintedItem>
#include <QHash>
#include <QElapsedTimer>

QJsonObject terminalKey(const QKeyEvent &event, const QString &action);

class TerminalItem : public QQuickPaintedItem {
    Q_OBJECT
    Q_PROPERTY(Frontend *frontend READ frontend WRITE setFrontend NOTIFY frontendChanged)
    Q_PROPERTY(QString paneId READ paneId WRITE setPaneId NOTIFY paneIdChanged)
    Q_PROPERTY(QVariantMap paneState READ paneState NOTIFY textChanged)
    Q_PROPERTY(bool copyOnSelection MEMBER m_copyOnSelection)
    Q_PROPERTY(bool primarySelection MEMBER m_primarySelection)
    Q_PROPERTY(QString visibleText READ visibleText NOTIFY textChanged)
public:
    explicit TerminalItem(QQuickItem *parent = nullptr);
    Frontend *frontend() const { return m_frontend; }
    void setFrontend(Frontend *frontend);
    QString paneId() const { return m_paneId; }
    void setPaneId(const QString &id);
    QVariantMap paneState() const { return m_state.toVariantMap(); }
    QString visibleText() const;
    void paint(QPainter *painter) override;
    Q_INVOKABLE void search(const QString &text);
    Q_INVOKABLE void searchStep(bool older);
    Q_INVOKABLE void promptStep(bool older);
    Q_INVOKABLE void copy();
    Q_INVOKABLE void paste();
    Q_INVOKABLE void selectAll();
    void foreground();
    int columns() const;
    int rows() const;
    qreal cellWidth() const { return m_cellWidth; }
    qreal cellHeight() const { return m_cellHeight; }
    bool event(QEvent *event) override;
signals:
    void frontendChanged();
    void paneIdChanged();
    void textChanged();
protected:
    void keyPressEvent(QKeyEvent *event) override;
    void keyReleaseEvent(QKeyEvent *event) override;
    void inputMethodEvent(QInputMethodEvent *event) override;
    QVariant inputMethodQuery(Qt::InputMethodQuery query) const override;
    void mousePressEvent(QMouseEvent *event) override;
    void mouseDoubleClickEvent(QMouseEvent *event) override;
    void wheelEvent(QWheelEvent *event) override;
    void hoverMoveEvent(QHoverEvent *event) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void mouseReleaseEvent(QMouseEvent *event) override;
    void geometryChange(const QRectF &newGeometry, const QRectF &oldGeometry) override;
    void focusInEvent(QFocusEvent *event) override;
    void focusOutEvent(QFocusEvent *event) override;
private:
    void updateState();
    void resizeTerminal();
    int cellAt(const QPointF &point) const;
    QString selectedText() const;
    void finishCopy();
    bool pointer(QMouseEvent *event, const QString &action);
    void selection(const QString &phase, const QPointF &point, int clicks = 1);
    quint64 m_selectionRevision = 0;
    quint64 m_copyRevision = 0;
    bool sendKey(QKeyEvent *event, const QString &action);
    Frontend *m_frontend = nullptr;
    QJsonObject m_frame;
    QJsonObject m_state;
    QString m_paneId;
    QString m_target;
    QString m_linkUri;
    QString m_linkTarget;
    int m_linkStart = -1;
    int m_linkEnd = -1;
    QString m_lastResize;
    QFont m_font;
    qreal m_cellWidth = 10;
    qreal m_cellHeight = 20;
    qreal m_ascent = 15;
    QHash<int, QJsonObject> m_pressed;
    bool m_dragging = false;
    bool m_reporting = false;
    QString m_mouseButton;
    QElapsedTimer m_clickClock;
    QPointF m_clickPosition;
    QPointF m_selectionPosition;
    int m_clicks = 0;
    qreal m_scrollFraction = 0;
    bool m_copyOnSelection = false;
    bool m_primarySelection = false;
    bool m_primaryCopy = false;
    bool m_foregroundPending = false;
};
