#include "settings.h"
#include <QFontDatabase>
#include <QFontMetricsF>
#include <QFontInfo>
#include <QPainter>
#include <cmath>
#include <algorithm>

TerminalMetrics terminalMetrics(const QVariantMap &preferences) {
    QFont font;
    QStringList families{preferences.value("font_family", "JetBrainsMono Nerd Font").toString()};
    for (const auto &fallback : preferences.value("font_fallbacks").toList()) families << fallback.toString();
    families << "IBM Plex Mono" << "Symbols Nerd Font" << QFontDatabase::systemFont(QFontDatabase::FixedFont).family();
    font.setFamilies(families);
    font.setStyleHint(QFont::Monospace);
    font.setPixelSize(std::clamp(qRound(preferences.value("font_size",15).toDouble()),8,48));
    const auto typography = preferences.value("terminal").toMap().value("font").toMap();
    const auto style = typography.value("style").toString();
    font.setBold(style == "bold" || style == "bold_italic");
    font.setItalic(style == "italic" || style == "bold_italic");
    font.setKerning(false);
    font.setFeature(QFont::Tag("liga"),0);
    font.setFeature(QFont::Tag("calt"),0);
    const QFontMetricsF metrics(font);
    const qreal advance = metrics.horizontalAdvance('M');
    const bool percent = typography.value("spacing_unit").toString() == "percent";
    const int spacing = std::clamp(typography.value("spacing").toInt(),percent ? -30 : -3,percent ? 100 : 12);
    const qreal height = std::max(qreal(1),std::ceil(metrics.height() * (1 + std::clamp(typography.value("line_height").toInt(),-20,100) / 100.)));
    return {font,std::max(qreal(1),std::ceil(advance + (percent ? advance * spacing / 100. : spacing))),height,
            metrics.ascent() + (height - metrics.height()) / 2};
}
QPair<QColor,QColor> selectionColors(const QVariantMap &preferences, QColor foreground, QColor background) {
    const auto config=preferences.value("terminal").toMap();
    const auto appearance=preferences.value("_appearance").toMap();
    const auto fillSource=config.value("selection_background","theme").toString();
    const auto inkSource=config.value("selection_foreground","theme").toString();
    const auto resolve = [&](const QString &source, const QVariant &theme, QColor fallback) {
        if (source == "cell_foreground") return foreground;
        if (source == "cell_background") return background;
        return theme.isNull() ? fallback : QColor::fromRgb(theme.toUInt());
    };
    auto fill = resolve(fillSource, appearance.value("selection_background"), QColor("#244866"));
    auto ink = resolve(inkSource, appearance.value("selection_foreground"), foreground);
    if ((fillSource!="theme" || inkSource!="theme") && ink==fill && foreground!=background)
        ink=fill==background ? foreground : background;
    return {fill,ink};
}
SettingsController::SettingsController(QObject *parent) : QObject(parent) {}
QStringList SettingsController::families() const {
    auto result = QFontDatabase::families(); result.removeDuplicates(); result.sort(Qt::CaseInsensitive); return result;
}
QVariantMap SettingsController::metrics() const {
    const auto m = terminalMetrics(m_values);
    return {{"width",m.width},{"height",m.height},{"ascent",m.ascent},{"family",QFontInfo(m.font).family()}};
}
void SettingsController::begin(const QVariantMap &preferences) {
    m_original=preferences; m_values=preferences; m_defaults=preferences.value("_defaults").toMap(); emit valuesChanged();
}
void SettingsController::setValue(const QString &path, const QVariant &value) {
    const auto keys=path.split('.');
    if (path=="pane_padding") { auto group=m_values["interface"].toMap();if(!group.contains("pane_padding_vertical")) group["pane_padding_vertical"]=m_values["pane_padding"];m_values["interface"]=group; }
    if (keys.size()==1) m_values[keys[0]]=value;
    else if (keys.size()==2) { auto group=m_values[keys[0]].toMap();group[keys[1]]=value;m_values[keys[0]]=group; }
    else if (keys.size()==3) { auto group=m_values[keys[0]].toMap();auto child=group[keys[1]].toMap();child[keys[2]]=value;group[keys[1]]=child;m_values[keys[0]]=group; }
    emit valuesChanged();
}
void SettingsController::reset() {
    for (auto it=m_defaults.cbegin();it!=m_defaults.cend();++it) m_values[it.key()]=it.value();
    emit valuesChanged();
}
void SettingsController::cancel() { m_values=m_original;emit valuesChanged(); }
QVariantMap SettingsController::patch() const {
    QVariantMap patch;
    for (auto it=m_values.cbegin();it!=m_values.cend();++it)
        if (!it.key().startsWith('_') && it.value()!=m_original.value(it.key())) patch[it.key()]=it.value();
    return patch;
}
void SettingsController::setPage(const QString &page) {
    if (page==m_page) return;
    m_history.append(m_page);m_page=page;emit pageChanged();
}
void SettingsController::back() { if (m_history.isEmpty()) return;m_page=m_history.takeLast();emit pageChanged(); }
TerminalPreview::TerminalPreview(QQuickItem *parent):QQuickPaintedItem(parent) {setClip(true);}
void TerminalPreview::setPreferences(const QVariantMap &preferences) {m_preferences=preferences;emit preferencesChanged();update();}
void TerminalPreview::paint(QPainter *p) {
    const auto m=terminalMetrics(m_preferences);p->fillRect(boundingRect(),QColor("#001528"));p->setFont(m.font);p->setPen(QColor("#e8eef6"));
    const QString sample=m_kind=="cursor" ? "$ echo Rex" : m_kind=="selection" ? "Select text · λ → ▓" : "AaBb 012345  λ →  ┌─┐";
    const auto config=m_preferences.value("terminal").toMap();
    if(m_kind=="selection") {
        const QRectF selected(10+2*m.width,6,9*m.width,m.height);
        p->setBrush(selectionColors(m_preferences,QColor("#e8eef6"),QColor("#001528")).first);p->setPen(Qt::NoPen);
        if(config.value("selection_rounded").toBool()) p->drawRoundedRect(selected,3,3);else p->drawRect(selected);
    }
    for(int i=0;i<sample.size();++i) {
        const QRectF cell(10+i*m.width,6,m.width,m.height);
        p->setPen(m_kind=="selection" && i>=2 && i<11 ? selectionColors(m_preferences,QColor("#e8eef6"),QColor("#001528")).second : QColor("#e8eef6"));p->drawText(QPointF(cell.left(),cell.top()+m.ascent),sample.mid(i,1));
    }
    if(m_kind=="cursor") {
        const auto shape=config.value("cursor_style","block").toString();QRectF cell(10+sample.size()*m.width,6,m.width,m.height);
        if(shape=="bar") cell.setWidth(2);else if(shape=="underline") cell=QRectF(cell.left(),cell.bottom()-2,cell.width(),2);
        p->fillRect(cell,QColor("#e8eef6"));
    }
}
