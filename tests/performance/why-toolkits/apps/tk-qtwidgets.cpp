// The matched program for Qt 6 Widgets: docs/performance/why-toolkits.md.
//
//   tk-qtwidgets text FILE      a QPlainTextEdit of FILE (no wrapping), the
//                               caret at the start of line 20
//   tk-qtwidgets list [ROWS]    a QListView of ROWS (10,000) rows, 24 px each
//
// Options: --scroll ROWS MS (every MS ms scroll down ROWS rows, 0 = a page,
// from a QTimer, wrapping at the end), --size W H (1280 x 800).
//
// Fonts are named as in the other toolkits' programs (IBM Plex Mono 14 px,
// Archivo 14 px); rows are 24 px through the delegate's size hint; the caret
// doesn't blink (as the other programs are set; TK_BLINK=1 leaves Qt's blink).
// Otherwise Qt's defaults.
#include <QApplication>
#include <QFile>
#include <QListView>
#include <QPlainTextEdit>
#include <QScrollBar>
#include <QStringListModel>
#include <QStyledItemDelegate>
#include <QTextBlock>
#include <QTimer>
#include <cstdio>
#include <cstring>

class RowDelegate : public QStyledItemDelegate {
public:
  using QStyledItemDelegate::QStyledItemDelegate;
  QSize sizeHint(const QStyleOptionViewItem &o, const QModelIndex &i) const override {
    QSize s = QStyledItemDelegate::sizeHint(o, i);
    return QSize(s.width(), 24);
  }
};

int main(int argc, char **argv) {
  QApplication app(argc, argv);
  QApplication::setDesktopFileName("org.tk.QtWidgets");
  if (!qEnvironmentVariableIsSet("TK_BLINK")) QApplication::setCursorFlashTime(0);
  const char *mode = argc > 1 ? argv[1] : "text";
  const char *file = nullptr;
  int rows = 10000, scrollRows = -1, scrollMs = 0, w = 1280, h = 800, i = 2;
  if (!strcmp(mode, "text")) {
    if (argc < 3) { fprintf(stderr, "text FILE\n"); return 2; }
    file = argv[i++];
  } else if (i < argc && argv[i][0] != '-') {
    rows = atoi(argv[i++]);
  }
  for (; i < argc; i++) {
    if (!strcmp(argv[i], "--scroll") && i + 2 < argc) { scrollRows = atoi(argv[i + 1]); scrollMs = atoi(argv[i + 2]); i += 2; }
    else if (!strcmp(argv[i], "--size") && i + 2 < argc) { w = atoi(argv[i + 1]); h = atoi(argv[i + 2]); i += 2; }
    else { fprintf(stderr, "unknown argument %s\n", argv[i]); return 2; }
  }
  QWidget *top = nullptr;
  QScrollBar *bar = nullptr;
  int stepUnit = 24;  // pixels per row, or 1 for a list scrolled by item
  QListView *listView = nullptr;
  if (!strcmp(mode, "text")) {
    QFile f(QString::fromLocal8Bit(file));
    if (!f.open(QIODevice::ReadOnly)) { fprintf(stderr, "can't read %s\n", file); return 2; }
    auto *edit = new QPlainTextEdit;
    QFont font("IBM Plex Mono");
    font.setPixelSize(14);
    edit->setFont(font);
    edit->setLineWrapMode(QPlainTextEdit::NoWrap);
    edit->setPlainText(QString::fromUtf8(f.readAll()));
    QTextCursor c(edit->document()->findBlockByNumber(19));
    edit->setTextCursor(c);
    edit->setFocus();
    top = edit;
    bar = edit->verticalScrollBar();
    stepUnit = 1;  // QPlainTextEdit scrolls by line
  } else {
    QStringList items;
    items.reserve(rows);
    char line[96];
    for (int r = 0; r < rows; r++) {
      snprintf(line, sizeof line, "Row %05d  fixture item %d of %d, in a list of generated rows", r + 1, r + 1, rows);
      items << QString::fromLatin1(line);
    }
    auto *view = new QListView;
    view->setModel(new QStringListModel(items, view));
    view->setUniformItemSizes(true);
    view->setItemDelegate(new RowDelegate(view));
    QFont font("Archivo");
    font.setPixelSize(14);
    view->setFont(font);
    top = view;
    listView = view;
    bar = view->verticalScrollBar();
    // QListView scrolls per item by default: the bar counts rows.
    stepUnit = view->verticalScrollMode() == QAbstractItemView::ScrollPerItem ? 1 : 24;
  }
  top->setWindowTitle("tk");
  top->resize(w, h);
  top->show();
  if (scrollRows >= 0 && scrollMs > 0) {
    auto *timer = new QTimer(top);
    timer->setTimerType(Qt::PreciseTimer);
    QObject::connect(timer, &QTimer::timeout, [=] {
      int by = scrollRows > 0 ? scrollRows * stepUnit : bar->pageStep();
      int v = bar->value() + by;
      if (v > bar->maximum()) v = 0;
      bar->setValue(v);
    });
    QTimer::singleShot(1500, top, [=] { timer->start(scrollMs); });
  }
  (void)listView;
  return app.exec();
}
