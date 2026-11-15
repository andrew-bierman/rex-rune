#pragma once
#include <QObject>
#include <QVariantMap>
#include <QVariantList>

class ActivityModel : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap inventory READ inventory WRITE setInventory NOTIFY changed)
    Q_PROPERTY(QString filter READ filter WRITE setFilter NOTIFY changed)
    Q_PROPERTY(bool sessionView READ sessionView WRITE setSessionView NOTIFY changed)
    Q_PROPERTY(QString selectedKey READ selectedKey WRITE setSelectedKey NOTIFY selectionChanged)
    Q_PROPERTY(QVariantList rows READ rows NOTIFY changed)
    Q_PROPERTY(int chipCount READ chipCount NOTIFY changed)
public:
    explicit ActivityModel(QObject *parent = nullptr) : QObject(parent) {}
    QVariantMap inventory() const { return m_inventory; }
    QString filter() const { return m_filter; }
    bool sessionView() const { return m_sessionView; }
    QString selectedKey() const { return m_selected; }
    QVariantList rows() const { return m_rows; }
    int chipCount() const;
    void setInventory(const QVariantMap &value);
    void setFilter(const QString &value);
    void setSessionView(bool value);
    void setSelectedKey(const QString &value);
    Q_INVOKABLE void step(int delta);
    Q_INVOKABLE void activateSelected();
signals:
    void changed();
    void selectionChanged();
    void activate(const QVariantMap &target);
private:
    void reconcile();
    QVariantList projectRows() const;
    QVariantMap m_inventory;
    QVariantList m_rows;
    QString m_filter, m_selected;
    bool m_sessionView = false;
};
