#pragma once
#include <QObject>
#include <QVariantMap>
#include <QFont>
#include <QColor>
#include <QQuickPaintedItem>

struct TerminalMetrics {
    QFont font;
    qreal width;
    qreal height;
    qreal ascent;
};
TerminalMetrics terminalMetrics(const QVariantMap &preferences);
QPair<QColor,QColor> selectionColors(const QVariantMap &preferences, QColor foreground, QColor background);

class SettingsController : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap values READ values NOTIFY valuesChanged)
    Q_PROPERTY(QStringList families READ families CONSTANT)
    Q_PROPERTY(QVariantMap metrics READ metrics NOTIFY valuesChanged)
    Q_PROPERTY(QString page READ page WRITE setPage NOTIFY pageChanged)
    Q_PROPERTY(bool canBack READ canBack NOTIFY pageChanged)
public:
    explicit SettingsController(QObject *parent = nullptr);
    QVariantMap values() const { return m_values; }
    QStringList families() const;
    QVariantMap metrics() const;
    QString page() const { return m_page; }
    void setPage(const QString &page);
    bool canBack() const { return !m_history.isEmpty(); }
    Q_INVOKABLE void begin(const QVariantMap &preferences);
    Q_INVOKABLE void setValue(const QString &path, const QVariant &value);
    Q_INVOKABLE void reset();
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void back();
    Q_INVOKABLE QVariantMap patch() const;
signals:
    void valuesChanged();
    void pageChanged();
private:
    QVariantMap m_values, m_original, m_defaults;
    QString m_page = "Terminal";
    QStringList m_history;
};

class TerminalPreview : public QQuickPaintedItem {
    Q_OBJECT
    Q_PROPERTY(QVariantMap preferences READ preferences WRITE setPreferences NOTIFY preferencesChanged)
    Q_PROPERTY(QString kind MEMBER m_kind NOTIFY preferencesChanged)
public:
    explicit TerminalPreview(QQuickItem *parent = nullptr);
    QVariantMap preferences() const { return m_preferences; }
    void setPreferences(const QVariantMap &preferences);
    void paint(QPainter *painter) override;
signals:
    void preferencesChanged();
private:
    QVariantMap m_preferences;
    QString m_kind;
};
