/* A real GTK editor for the IBus component VM. No synthetic text injection. */
#include <gtk/gtk.h>

static const char *output;
static void changed(GtkEditable *entry, gpointer data) {
    (void)data;
    g_file_set_contents(output, gtk_entry_get_text(GTK_ENTRY(entry)), -1, NULL);
}
static void preedit(GtkEntry *entry, const gchar *text, gpointer data) {
    (void)entry;
    (void)data;
    if (*text)
        g_file_set_contents("/tmp/gtk-preedit", text, -1, NULL);
}
static void activated(GtkEntry *entry, gpointer data) {
    (void)data;
    g_file_set_contents("/tmp/gtk-activated", gtk_entry_get_text(entry), -1, NULL);
}
int main(int argc, char **argv) {
    gtk_init(&argc, &argv);
    if (argc != 2) return 2;
    output = argv[1];
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "IBus GTK composition");
    gtk_window_set_default_size(GTK_WINDOW(window), 500, 100);
    GtkWidget *entry = gtk_entry_new();
    gtk_container_add(GTK_CONTAINER(window), entry);
    g_signal_connect(entry, "changed", G_CALLBACK(changed), NULL);
    g_signal_connect(entry, "preedit-changed", G_CALLBACK(preedit), NULL);
    g_signal_connect(entry, "activate", G_CALLBACK(activated), NULL);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    gtk_widget_show_all(window);
    gtk_widget_grab_focus(entry);
    gtk_main();
    return 0;
}
