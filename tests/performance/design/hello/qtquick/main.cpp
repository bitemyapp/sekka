// A label and a button in a 480 x 200 window, in Qt 6 Quick (QML; Plasma's
// shell and newer KDE apps).
#include <QGuiApplication>
#include <QQmlApplicationEngine>

static const char *qml = R"(
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
ApplicationWindow {
  visible: true; width: 480; height: 200; title: "Hello"
  ColumnLayout {
    anchors.fill: parent; anchors.margins: 16; spacing: 12
    Label { text: "Hello, world" }
    Button { text: "Press me" }
  }
}
)";

int main(int argc, char **argv) {
  QGuiApplication app(argc, argv);
  QQmlApplicationEngine engine;
  engine.loadData(qml);
  return app.exec();
}
