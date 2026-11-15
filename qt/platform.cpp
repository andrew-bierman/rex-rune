#include "platform.h"
#include <QDesktopServices>

int NativePlatform::physicalKey(const QKeyEvent &event) {
#ifdef Q_OS_LINUX
    // Both Qt's xcb and Wayland plugins expose XKB keycodes (evdev + 8).
    const auto scan = event.nativeScanCode();
    if (scan >= 10 && scan <= 18) return Qt::Key_1 + int(scan - 10);
    if (scan == 19) return Qt::Key_0;
    static const char *rows[] = {"QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"};
    static const unsigned starts[] = {24,38,52};
    for (int row=0; row<3; ++row) {
        const auto length = qstrlen(rows[row]);
        if (scan >= starts[row] && scan < starts[row] + length) return rows[row][scan-starts[row]];
    }
    switch (scan) {
    case 20: return Qt::Key_Minus;
    case 21: return Qt::Key_Equal;
    case 34: return Qt::Key_BracketLeft;
    case 35: return Qt::Key_BracketRight;
    case 47: return Qt::Key_Semicolon;
    case 48: return Qt::Key_Apostrophe;
    case 49: return Qt::Key_QuoteLeft;
    case 51: return Qt::Key_Backslash;
    case 59: return Qt::Key_Comma;
    case 60: return Qt::Key_Period;
    case 61: return Qt::Key_Slash;
    default: break;
    }
#endif
    // Synthetic Qt events have no native scan code. Named keys are layout-independent.
    return event.key();
}

QString NativePlatform::physicalCode(const QKeyEvent &event) {
#ifdef Q_OS_LINUX
    switch(event.nativeScanCode()) {
    case 9:return "Escape";case 22:return "Backspace";case 23:return "Tab";
    case 36:return "Enter";case 65:return "Space";case 66:return "CapsLock";
    case 77:return "NumLock";case 78:return "ScrollLock";
    case 79:return "Numpad7";case 80:return "Numpad8";case 81:return "Numpad9";
    case 82:return "NumpadSubtract";case 83:return "Numpad4";case 84:return "Numpad5";
    case 85:return "Numpad6";case 86:return "NumpadAdd";case 87:return "Numpad1";
    case 88:return "Numpad2";case 89:return "Numpad3";case 90:return "Numpad0";
    case 91:return "NumpadDecimal";case 63:return "NumpadMultiply";
    case 94:return "IntlBackslash";case 104:return "NumpadEnter";case 106:return "NumpadDivide";
    case 110:return "Home";case 111:return "ArrowUp";case 112:return "PageUp";
    case 113:return "ArrowLeft";case 114:return "ArrowRight";case 115:return "End";
    case 116:return "ArrowDown";case 117:return "PageDown";case 118:return "Insert";
    case 119:return "Delete";case 107:return "PrintScreen";case 127:return "Pause";
    default:break;
    }
#endif
    const int key=physicalKey(event);
    if(key>=Qt::Key_A && key<=Qt::Key_Z)return "Key"+QString(QChar(key));
    if(key>=Qt::Key_0 && key<=Qt::Key_9)return "Digit"+QString(QChar(key));
    if(key>=Qt::Key_F1 && key<=Qt::Key_F35)return "F"+QString::number(key-Qt::Key_F1+1);
    switch(key) {
    case Qt::Key_Plus:case Qt::Key_Equal:return "Equal";
    case Qt::Key_Minus:case Qt::Key_Underscore:return "Minus";
    case Qt::Key_BracketLeft:return "BracketLeft";case Qt::Key_BracketRight:return "BracketRight";
    case Qt::Key_Backslash:return "Backslash";case Qt::Key_Semicolon:return "Semicolon";
    case Qt::Key_Apostrophe:return "Quote";case Qt::Key_QuoteLeft:return "Backquote";
    case Qt::Key_Comma:return "Comma";case Qt::Key_Period:return "Period";case Qt::Key_Slash:return "Slash";
    case Qt::Key_Space:return "Space";case Qt::Key_Escape:return "Escape";
    case Qt::Key_Return:case Qt::Key_Enter:return "Enter";
    case Qt::Key_Tab:case Qt::Key_Backtab:return "Tab";case Qt::Key_Backspace:return "Backspace";
    case Qt::Key_Delete:return "Delete";case Qt::Key_Insert:return "Insert";
    case Qt::Key_Home:return "Home";case Qt::Key_End:return "End";
    case Qt::Key_PageUp:return "PageUp";case Qt::Key_PageDown:return "PageDown";
    case Qt::Key_Up:return "ArrowUp";case Qt::Key_Down:return "ArrowDown";
    case Qt::Key_Left:return "ArrowLeft";case Qt::Key_Right:return "ArrowRight";
    default:return {};
    }
}

quint64 NativePlatform::keyIdentity(const QKeyEvent &event) {
    return event.nativeScanCode() ? quint64(event.nativeScanCode()) | (quint64(1)<<32) : quint64(event.key());
}

bool NativePlatform::openLink(const QUrl &url) {
    const auto scheme = url.scheme().toLower();
    if (!url.isValid() || (scheme != "https" && scheme != "http" && scheme != "ftp" && scheme != "mailto")) return false;
    return QDesktopServices::openUrl(url);
}
