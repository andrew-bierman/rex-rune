QT += core gui qml quick quickcontrols2
CONFIG += c++17 warn_on
TARGET = rex-qt
TEMPLATE = app
SOURCES += main.cpp frontend.cpp terminal.cpp actions.cpp settings.cpp activity.cpp
HEADERS += frontend.h terminal.h actions.h settings.h activity.h
RESOURCES += resources.qrc
contains(CONFIG, test_driver) {
    QT += testlib
    DEFINES += REX_QT_TEST_DRIVER
    TARGET = rex-qt-test-driver
    SOURCES += test_driver.cpp
    HEADERS += test_driver.h
}
