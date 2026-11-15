#pragma once
#include <QKeyEvent>
#include <QUrl>

namespace NativePlatform {
// Qt's native scan code identifies a position; event.key()/text identify layout text.
int physicalKey(const QKeyEvent &event);
QString physicalCode(const QKeyEvent &event);
quint64 keyIdentity(const QKeyEvent &event);
bool openLink(const QUrl &url);
}
