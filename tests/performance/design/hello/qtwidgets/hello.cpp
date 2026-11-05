// A label and a button in a 480 x 200 window, in Qt 6 Widgets (KDE's apps).
#include <QApplication>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>
#include <QWidget>

int main(int argc, char **argv) {
  QApplication app(argc, argv);
  QWidget win;
  win.setWindowTitle("Hello");
  win.resize(480, 200);
  auto *layout = new QVBoxLayout(&win);
  layout->setContentsMargins(16, 16, 16, 16);
  layout->setSpacing(12);
  layout->addWidget(new QLabel("Hello, world"));
  layout->addWidget(new QPushButton("Press me"));
  win.show();
  return app.exec();
}
