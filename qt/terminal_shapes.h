#pragma once
#include <QPainter>
// Terminal-only device-pixel sprites; geometry follows src/app/sprites.rs.
bool paintTerminalShape(QPainter &painter, const QRectF &cell, const QString &text, const QColor &ink);
void paintTerminalUnderline(QPainter &painter, const QRectF &cell, const QString &style, const QColor &ink);
