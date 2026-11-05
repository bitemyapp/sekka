// The matched program for Qt 6 Quick (QML, QtQuick.Controls):
// docs/performance/why-toolkits.md.
//
//   tk-qtquick text FILE        a TextArea in a ScrollView (no wrapping), the
//                               caret at the start of line 20
//   tk-qtquick list [ROWS]      a ListView of ROWS (10,000) ItemDelegates,
//                               24 px each
//
// Options: --scroll ROWS MS (every MS ms scroll down ROWS rows, 0 = a page,
// from a QML Timer, wrapping at the end), --size W H (1280 x 800).
//
// Fonts are named as in the other programs (IBM Plex Mono 14 px, Archivo
// 14 px), the caret doesn't blink (TK_BLINK=1 leaves Qt's blink); otherwise
// Qt Quick's defaults (the Controls style the platform gives).
#include <QFile>
#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QStyleHints>
#include <cstdio>
#include <cstring>

static const char *qml = R"(
import QtQuick
import QtQuick.Controls
ApplicationWindow {
  id: win
  visible: true; width: winW; height: winH; title: "tk"
  function step(f) {
    var by = scrollRows > 0 ? scrollRows * 24 : f.height
    var v = f.contentY + by
    if (v > f.contentHeight - f.height) v = f.originY
    f.contentY = v
  }
  Loader { anchors.fill: parent; sourceComponent: mode === "text" ? textView : listView }
  Component {
    id: textView
    ScrollView {
      id: sv
      TextArea {
        id: area
        text: fileText
        font.family: "IBM Plex Mono"; font.pixelSize: 14
        wrapMode: TextEdit.NoWrap
        focus: true
      }
      // Once the view has its size: placed earlier, the caret is scrolled
      // to the top of a view that isn't laid out yet.
      Timer {
        interval: 300; running: true
        onTriggered: { area.cursorPosition = caretAt; area.forceActiveFocus(); sv.contentItem.contentY = 0 }
      }
      Timer {
        id: tick; interval: Math.max(1, scrollMs); repeat: true; running: false
        onTriggered: win.step(sv.contentItem)
      }
      Timer { interval: 1500; running: scrollRows >= 0 && scrollMs > 0; onTriggered: tick.running = true }
    }
  }
  Component {
    id: listView
    ListView {
      id: lv
      model: rowCount
      clip: true
      ScrollBar.vertical: ScrollBar {}
      delegate: ItemDelegate {
        width: ListView.view.width; height: 24
        leftPadding: 8; topPadding: 0; bottomPadding: 0
        font.family: "Archivo"; font.pixelSize: 14
        text: "Row " + String(index + 1).padStart(5, "0") + "  fixture item " + (index + 1) + " of " + rowCount + ", in a list of generated rows"
      }
      Timer {
        id: tick; interval: Math.max(1, scrollMs); repeat: true; running: false
        onTriggered: win.step(lv)
      }
      Timer { interval: 1500; running: scrollRows >= 0 && scrollMs > 0; onTriggered: tick.running = true }
    }
  }
}
)";

int main(int argc, char **argv) {
  QGuiApplication app(argc, argv);
  QGuiApplication::setDesktopFileName("org.tk.QtQuick");
  if (!qEnvironmentVariableIsSet("TK_BLINK")) QGuiApplication::styleHints()->setCursorFlashTime(0);
  const char *mode = argc > 1 ? argv[1] : "text";
  QString text;
  int rows = 10000, scrollRows = -1, scrollMs = 0, w = 1280, h = 800, i = 2, caret = 0;
  if (!strcmp(mode, "text")) {
    if (argc < 3) { fprintf(stderr, "text FILE\n"); return 2; }
    QFile f(QString::fromLocal8Bit(argv[i++]));
    if (!f.open(QIODevice::ReadOnly)) { fprintf(stderr, "can't read the file\n"); return 2; }
    text = QString::fromUtf8(f.readAll());
    // The start of line 20.
    for (int line = 0, at = 0; line < 19 && at >= 0; line++) {
      at = text.indexOf(QLatin1Char('\n'), at);
      if (at >= 0) caret = ++at;
    }
  } else if (i < argc && argv[i][0] != '-') {
    rows = atoi(argv[i++]);
  }
  for (; i < argc; i++) {
    if (!strcmp(argv[i], "--scroll") && i + 2 < argc) { scrollRows = atoi(argv[i + 1]); scrollMs = atoi(argv[i + 2]); i += 2; }
    else if (!strcmp(argv[i], "--size") && i + 2 < argc) { w = atoi(argv[i + 1]); h = atoi(argv[i + 2]); i += 2; }
    else { fprintf(stderr, "unknown argument %s\n", argv[i]); return 2; }
  }
  QQmlApplicationEngine engine;
  QQmlContext *c = engine.rootContext();
  c->setContextProperty("mode", QString::fromLatin1(mode));
  c->setContextProperty("fileText", text);
  c->setContextProperty("caretAt", caret);
  c->setContextProperty("rowCount", rows);
  c->setContextProperty("scrollRows", scrollRows);
  c->setContextProperty("scrollMs", scrollMs);
  c->setContextProperty("winW", w);
  c->setContextProperty("winH", h);
  engine.loadData(qml);
  if (engine.rootObjects().isEmpty()) return 1;
  return app.exec();
}
