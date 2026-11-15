// Independent native Wayland reader for qt-wayland-private.py; public Qt APIs only.
#include <QClipboard>
#include <QFile>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPainter>
#include <QRasterWindow>
#include <QTimer>

class ReaderWindow : public QRasterWindow {
    void paintEvent(QPaintEvent *) override {
        QPainter painter(this);
        painter.fillRect(QRect(QPoint(), size()), Qt::black);
    }
};

int main(int argc, char **argv) {
    QGuiApplication app(argc, argv);
    if (app.arguments().size() != 3 || app.platformName() != "wayland") return 2;
    const auto expected = app.arguments()[1];
    const auto report = app.arguments()[2];
    auto *clipboard = app.clipboard();
    if (!clipboard->supportsSelection()) return 3;
    ReaderWindow window;
    window.resize(320, 240);
    window.show();
    QTimer poll;
    QObject::connect(&poll, &QTimer::timeout, &app, [&] {
        if (!window.isActive()) return;
        const auto copied = clipboard->text(QClipboard::Clipboard);
        const auto primary = clipboard->text(QClipboard::Selection);
        if (copied != expected || primary != expected) return;
        QFile file(report);
        if (!file.open(QIODevice::WriteOnly)) { app.exit(4); return; }
        file.write(QJsonDocument(QJsonObject{{"platform", app.platformName()},
            {"supportsSelection", true}, {"clipboard", copied}, {"primary", primary}}).toJson());
        file.close();
        app.exit(0);
    });
    poll.start(100);
    QTimer::singleShot(6000, &app, [&] { app.exit(5); });
    return app.exec();
}
