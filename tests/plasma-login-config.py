"""Keep the production loader/helper body; replace only config paths and its
settings-object construction so the real KConfig merge can run in isolation.
"""
from pathlib import Path
import json
import sys

source = Path(sys.argv[1]).read_text()
root = Path(sys.argv[2])
start = source.index("static void ")
end = source.index("    static PlasmaLoginSettings instance(config);", start)
loader = source[start:end]
signature = "PlasmaLoginSettings &PlasmaLoginSettings::getInstance()"
assert loader.count(signature) == 1
loader = loader.replace(signature, "static KSharedConfig::Ptr loadConfig()")
print("""
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QTextStream>
#include <KSharedConfig>
#include <KConfigGroup>
""")
for macro, name in {
    "PLASMALOGIN_CONFIG_FILE": "main.conf",
    "PLASMALOGIN_CONFIG_DIR": "admin",
    "PLASMALOGIN_SYSTEM_CONFIG_FILE": "defaults.conf",
    "PLASMALOGIN_SYSTEM_CONFIG_DIR": "system",
}.items():
    print(f"#define {macro} {json.dumps(str(root / name))}")
print(f"#define TEST_ROOT {json.dumps(str(root))}")
print(loader + "    return config;\n}\n")
print(r'''
static void reset() {
    QDir(TEST_ROOT).removeRecursively();
    QDir().mkpath(TEST_ROOT);
}
static void write(const QString &name, const QString &session) {
    const QString path = QStringLiteral(TEST_ROOT) + '/' + name;
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly)) qFatal("cannot create fixture");
    file.write(("[Greeter]\nPreselectedSession=" + session + "\n").toUtf8());
}
static void check(const char *name, const QString &expected) {
    const QString actual = loadConfig()->group("Greeter").readEntry("PreselectedSession", QString("unset"));
    QTextStream(stdout) << name << ": " << actual << '\n';
    if (actual != expected) qFatal("%s: expected %s, got %s", name, qPrintable(expected), qPrintable(actual));
}
int main(int argc, char **argv) {
    QCoreApplication app(argc, argv);
    const bool original = QString::fromLocal8Bit(argv[1]) == "original";
    reset();
    check("all files missing", "unset");
    write("defaults.conf", "plasma.desktop");
    check("shipped fallback", "plasma.desktop");
    write("admin/10-local.conf", "xfce.desktop");
    check("administrator overrides shipped default", original ? "plasma.desktop" : "xfce.desktop");
    if (original) return 0; // Exact unpatched loader reproduced the defect.
    write("system/10-vendor.conf", "hyprland-uwsm.desktop");
    check("administrator overrides vendor", "xfce.desktop");
    QDir(QStringLiteral(TEST_ROOT) + "/admin").removeRecursively();
    check("vendor overrides shipped fallback", "hyprland-uwsm.desktop");
    write("system/90-site.conf", "tatami.desktop");
    check("later vendor fragment wins", "tatami.desktop");
    write("admin/10-local.conf", "xfce.desktop");
    write("admin/90-local.conf", "custom.desktop");
    check("later administrator fragment wins", "custom.desktop");
    write("main.conf", "main.desktop");
    check("primary config overrides all fragments", "main.desktop");
    reset();
    write("admin/10-local.conf", "xfce.desktop");
    check("administrator works without shipped files", "xfce.desktop");
    write("main.conf", "");
    check("explicit empty primary value is preserved", "");
    reset();
    write("system/10-vendor.conf", "custom.desktop");
    check("vendor works without other files", "custom.desktop");
}
''')
