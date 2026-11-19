QT += core testlib dbus
CONFIG += console testcase c++17
TARGET = notification-services-test
SOURCES += notification_services.cpp ../notifications.cpp ../sounds.cpp
HEADERS += ../notifications.h ../sounds.h
