QT += core gui qml quick testlib dbus
CONFIG += c++17 console testcase
TEMPLATE = app
TARGET = native-tests
SOURCES += native_tests.cpp terminal.cpp frontend.cpp actions.cpp settings.cpp platform.cpp terminal_shapes.cpp notifications.cpp sounds.cpp
HEADERS += terminal.h frontend.h actions.h settings.h platform.h terminal_shapes.h notifications.h sounds.h
