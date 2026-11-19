QT += core gui testlib
CONFIG += c++17 console testcase
CONFIG -= app_bundle
TEMPLATE = app
TARGET = rex-qt-actions-test
SOURCES += actions.cpp actions_test.cpp platform.cpp
HEADERS += actions.h platform.h
