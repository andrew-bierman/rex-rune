#include "terminal.h"
#include "terminal_shapes.h"
#include <QImage>
#include <QGuiApplication>
#include <QInputMethodEvent>
#include <QSignalSpy>
#include <QTest>
#include <QProcess>
#include <algorithm>
#include <QWindow>
#include <QDesktopServices>
#include "platform.h"

class LayoutWindow : public QWindow {
    Q_OBJECT
signals:
    void keyReceived(int logical, quint32 scan, QString text, int physical);
protected:
    void keyPressEvent(QKeyEvent *event) override {emit keyReceived(event->key(),event->nativeScanCode(),event->text(),NativePlatform::physicalKey(*event));}
};
class InputTerminal : public TerminalItem {
public: using TerminalItem::inputMethodQuery;
};
class LinkReceiver : public QObject {
    Q_OBJECT
public: int count=0;
public slots: void opened(const QUrl &) {++count;}
};
class NativeTests : public QObject {
    Q_OBJECT
private slots:
    void linkOpenerUsesApprovedSchemes() {
        LinkReceiver receiver;
        QDesktopServices::setUrlHandler("https",&receiver,"opened");
        QVERIFY(!NativePlatform::openLink(QUrl("file:///tmp/rex-link-fixture")));
        QVERIFY(!NativePlatform::openLink(QUrl("javascript:alert(1)")));
        QCOMPARE(receiver.count,0);
        QVERIFY(NativePlatform::openLink(QUrl("https://example.com/qt-fixture")));
        QCOMPARE(receiver.count,1);QDesktopServices::unsetUrlHandler("https");
    }
    void nativeFrenchLayout() {
        if(!qEnvironmentVariableIsSet("REX_QT_NATIVE_LAYOUT_CHECK")) QSKIP("requires owned compositor and French XKB layout");
        QCOMPARE(QGuiApplication::platformName(),QString("wayland"));
        LayoutWindow window;window.resize(400,200);window.show();
        QTRY_VERIFY(window.isActive());
        QSignalSpy keys(&window,&LayoutWindow::keyReceived);
        QProcess typer;typer.start(qEnvironmentVariable("REX_QT_LAYOUT_TYPER"),QStringList{});QVERIFY(typer.waitForStarted());
        QTRY_VERIFY(std::any_of(keys.begin(),keys.end(),[](const QList<QVariant> &event){return event[0].toInt()==Qt::Key_A;}));
        const auto event=*std::find_if(keys.begin(),keys.end(),[](const QList<QVariant> &event){return event[0].toInt()==Qt::Key_A;});
        QCOMPARE(event[0].toInt(),int(Qt::Key_A));QCOMPARE(event[1].toUInt(),quint32(24));
        QCOMPARE(event[2].toString(),QString("a"));QCOMPARE(event[3].toInt(),int(Qt::Key_Q));
        QVERIFY(typer.waitForFinished());QCOMPARE(typer.exitCode(),0);
    }
    void preeditMovesInputRectangleAndCancels() {
        InputTerminal terminal;
        const auto before=terminal.inputMethodQuery(Qt::ImCursorRectangle).toRectF();
        QInputMethodEvent preedit("compose",{QInputMethodEvent::Attribute(QInputMethodEvent::Cursor,3,1,{})});
        QCoreApplication::sendEvent(&terminal,&preedit);
        const auto during=terminal.inputMethodQuery(Qt::ImCursorRectangle).toRectF();
        QVERIFY(during.left()>before.left());
        QInputMethodEvent cancel;QCoreApplication::sendEvent(&terminal,&cancel);
        QCOMPARE(terminal.inputMethodQuery(Qt::ImCursorRectangle).toRectF(),before);
    }
    void joinedLinesAndUnderlineVariants() {
        for(qreal scale:{1.,2.}) {
            QImage image(40,48,QImage::Format_ARGB32_Premultiplied);image.fill(Qt::black);image.setDevicePixelRatio(scale);
            QPainter p(&image);
            QVERIFY(paintTerminalShape(p,QRectF(0,0,20/scale,48/scale),QString::fromUtf8("─"),Qt::white));
            QVERIFY(paintTerminalShape(p,QRectF(20/scale,0,20/scale,48/scale),QString::fromUtf8("─"),Qt::white));
            p.end();for(int x=0;x<40;++x)QCOMPARE(image.pixelColor(x,23),QColor(Qt::white));
        }
        const auto underline=[](const QString &style){QImage image(80,30,QImage::Format_ARGB32_Premultiplied);image.fill(Qt::black);QPainter p(&image);paintTerminalUnderline(p,QRectF(0,0,80,30),style,Qt::white);p.end();return image;};
        const auto single=underline("single"),dotted=underline("dotted"),curly=underline("curly"),doubleLine=underline("double");
        int solidPixels=0,dotPixels=0;
        for(int x=0;x<80;++x){solidPixels+=single.pixelColor(x,28)==QColor(Qt::white);dotPixels+=dotted.pixelColor(x,28)==QColor(Qt::white);}
        QCOMPARE(solidPixels,80);QVERIFY(dotPixels>0 && dotPixels<solidPixels);
        QVERIFY(curly!=single);QVERIFY(doubleLine!=single);QVERIFY(doubleLine.pixelColor(10,26)==QColor(Qt::white));
    }
    void blocksFillDevicePixels() {
        for (qreal scale : {1.,2.}) {
            QImage image(40,48,QImage::Format_ARGB32_Premultiplied);image.fill(Qt::black);
            image.setDevicePixelRatio(scale); QPainter p(&image);
            QVERIFY(paintTerminalShape(p,QRectF(0,0,20/scale,48/scale),QString::fromUtf8("█"),Qt::white));
            QVERIFY(paintTerminalShape(p,QRectF(20/scale,0,20/scale,48/scale),QString::fromUtf8("█"),Qt::white));
            p.end();
            for(int y=0;y<48;++y) for(int x=0;x<40;++x) QCOMPARE(image.pixelColor(x,y),QColor(Qt::white));
        }
    }
    void physicalBindingUsesScanCode() {
        Actions actions;
        actions.setSnapshot({{"keymap",QJsonObject{{"actions",QJsonArray{QJsonObject{{"id","app.settings"},{"enabled",true}}}},{"bindings",QJsonArray{QJsonObject{
            {"action","app.settings"},{"chords",QJsonArray{QJsonObject{{"kind","physical"},{"key","KeyQ"},{"ctrl",true}}}}}}}}}});
        QSignalSpy requested(&actions,&Actions::requested);
        // Linux Qt native scan codes are XKB codes: physical Q = evdev 16 + 8.
        // A French layout labels that key A. The binding follows its position.
        QKeyEvent press(QEvent::KeyPress,Qt::Key_A,Qt::ControlModifier,24, 'a',0,QString(QChar(1)));
        QVERIFY(actions.handleKey(&press));
        QCOMPARE(requested.size(),1);
    }
    void deadKeyKeepsCompositionState() {
        QKeyEvent dead(QEvent::KeyPress,Qt::Key_Dead_Acute,Qt::NoModifier,48,0,0);
        QVERIFY(terminalKey(dead,"press")["composing"].toBool());
        QVERIFY(terminalKey(dead,"press")["text"].toString().isEmpty());
    }
    void nativeModifierNeverBecomesTextKey() {
        // Virtual keyboards may assign modifier keysyms to arbitrary native scans.
        QKeyEvent shift(QEvent::KeyPress,Qt::Key_Shift,Qt::ShiftModifier,9,0,0);
        QVERIFY(terminalKey(shift,"press").isEmpty());
    }
    void keypadIdentityIsDistinct() {
        QKeyEvent event(QEvent::KeyPress,Qt::Key_Enter,Qt::KeypadModifier,104,0,0,"\r");
        QCOMPARE(terminalKey(event,"press")["key"].toString(),QString("numpad_enter"));
        QCOMPARE(NativePlatform::physicalCode(event),QString("NumpadEnter"));
    }
    void terminalIdentityUsesScanCode() {
        QKeyEvent press(QEvent::KeyPress,Qt::Key_A,Qt::ControlModifier,24,'a',0,QString(QChar(1)));
        QCOMPARE(terminalKey(press,"press")["key"].toString(),QString("key_q"));
        // Logical text stays layout-owned for ordinary typing.
        QKeyEvent text(QEvent::KeyPress,Qt::Key_A,Qt::NoModifier,24,'a',0,"a");
        QCOMPARE(terminalKey(text,"press")["text"].toString(),QString("a"));
    }
};
QTEST_MAIN(NativeTests)
#include "native_tests.moc"
