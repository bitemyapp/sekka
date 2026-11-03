/* The matched program for GTK 4 (and, built with -DADW, libadwaita):
 * docs/performance/why-toolkits.md.
 *
 *   tk-gtk4 text FILE           a text view of FILE, the caret at the start
 *                               of line 20 (the same in every toolkit)
 *   tk-gtk4 list [ROWS]         a list of ROWS (10,000) rows, 24 px each
 *
 * Options (after the mode's arguments):
 *   --scroll ROWS MS            every MS ms, scroll the view down by ROWS
 *                               rows (24 px each; 0 rows = a page), from a
 *                               timer on the main loop, wrapping at the end
 *   --size W H                  the window's size (1280 x 800)
 *
 * Fonts are named, as in the other toolkits' programs: IBM Plex Mono at
 * 14 px in the text view, Archivo at 14 px in the list. Nothing else is
 * changed from GTK's (or libadwaita's) defaults. */
#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef ADW
#include <adwaita.h>
#define APP_ID "org.tk.Adw"
#else
#define APP_ID "org.tk.Gtk4"
#endif

static const char *mode = "text", *file = NULL;
static int rows = 10000, scroll_rows = -1, scroll_ms = 0, win_w = 1280, win_h = 800;
static GtkAdjustment *vadj;

static const char *css =
  "textview, textview text { font-family: \"IBM Plex Mono\"; font-size: 14px; }\n"
  "listview > row { min-height: 24px; padding: 0 8px; }\n"
  "listview > row label { font-family: \"Archivo\"; font-size: 14px; min-height: 0; }\n";

static gboolean step(gpointer data) {
  (void)data;
  double page = gtk_adjustment_get_page_size(vadj);
  double by = scroll_rows > 0 ? scroll_rows * 24.0 : page;
  double v = gtk_adjustment_get_value(vadj) + by;
  if (v > gtk_adjustment_get_upper(vadj) - page) v = 0;
  gtk_adjustment_set_value(vadj, v);
  return G_SOURCE_CONTINUE;
}

static gboolean start_scroll(gpointer data) {
  (void)data;
  if (scroll_rows >= 0 && scroll_ms > 0) g_timeout_add(scroll_ms, step, NULL);
  return G_SOURCE_REMOVE;
}

static void setup_row(GtkSignalListItemFactory *f, GtkListItem *item, gpointer data) {
  (void)f; (void)data;
  GtkWidget *label = gtk_label_new(NULL);
  gtk_label_set_xalign(GTK_LABEL(label), 0);
  gtk_list_item_set_child(item, label);
}

static void bind_row(GtkSignalListItemFactory *f, GtkListItem *item, gpointer data) {
  (void)f; (void)data;
  GtkStringObject *s = gtk_list_item_get_item(item);
  gtk_label_set_label(GTK_LABEL(gtk_list_item_get_child(item)), gtk_string_object_get_string(s));
}

static gboolean place_caret(gpointer data) {
  GtkTextView *view = data;
  GtkTextBuffer *buf = gtk_text_view_get_buffer(view);
  GtkTextIter it;
  gtk_text_buffer_get_iter_at_line(buf, &it, 19);
  gtk_text_buffer_place_cursor(buf, &it);
  gtk_widget_grab_focus(GTK_WIDGET(view));
  return G_SOURCE_REMOVE;
}

static GtkWidget *content(void) {
  GtkWidget *sw = gtk_scrolled_window_new();
  gtk_widget_set_vexpand(sw, TRUE);
  if (!strcmp(mode, "text")) {
    gchar *text = NULL;
    gsize len = 0;
    if (!g_file_get_contents(file, &text, &len, NULL)) { fprintf(stderr, "can't read %s\n", file); exit(2); }
    GtkWidget *view = gtk_text_view_new();
    gtk_text_view_set_wrap_mode(GTK_TEXT_VIEW(view), GTK_WRAP_NONE);
    gtk_text_view_set_monospace(GTK_TEXT_VIEW(view), TRUE);
    gtk_text_buffer_set_text(gtk_text_view_get_buffer(GTK_TEXT_VIEW(view)), text, (int)len);
    g_free(text);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(sw), view);
    g_idle_add(place_caret, view);
  } else {
    GtkStringList *model = gtk_string_list_new(NULL);
    char line[96];
    for (int i = 0; i < rows; i++) {
      snprintf(line, sizeof line, "Row %05d  fixture item %d of %d, in a list of generated rows", i + 1, i + 1, rows);
      gtk_string_list_append(model, line);
    }
    GtkListItemFactory *factory = gtk_signal_list_item_factory_new();
    g_signal_connect(factory, "setup", G_CALLBACK(setup_row), NULL);
    g_signal_connect(factory, "bind", G_CALLBACK(bind_row), NULL);
    GtkWidget *list = gtk_list_view_new(GTK_SELECTION_MODEL(gtk_single_selection_new(G_LIST_MODEL(model))), factory);
    gtk_scrolled_window_set_child(GTK_SCROLLED_WINDOW(sw), list);
  }
  vadj = gtk_scrolled_window_get_vadjustment(GTK_SCROLLED_WINDOW(sw));
  /* Scrolling starts once the window has been up a while (the harness
   * waits for it to settle before it measures). */
  g_timeout_add(1500, start_scroll, NULL);
  return sw;
}

static void activate(GtkApplication *app, gpointer data) {
  (void)data;
  GtkCssProvider *provider = gtk_css_provider_new();
  gtk_css_provider_load_from_string(provider, css);
  gtk_style_context_add_provider_for_display(gdk_display_get_default(), GTK_STYLE_PROVIDER(provider),
                                             GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
#ifdef ADW
  GtkWidget *win = adw_application_window_new(app);
  GtkWidget *bar = adw_toolbar_view_new();
  adw_toolbar_view_add_top_bar(ADW_TOOLBAR_VIEW(bar), adw_header_bar_new());
  adw_toolbar_view_set_content(ADW_TOOLBAR_VIEW(bar), content());
  adw_application_window_set_content(ADW_APPLICATION_WINDOW(win), bar);
#else
  GtkWidget *win = gtk_application_window_new(app);
  gtk_window_set_child(GTK_WINDOW(win), content());
#endif
  gtk_window_set_title(GTK_WINDOW(win), "tk");
  gtk_window_set_default_size(GTK_WINDOW(win), win_w, win_h);
  gtk_window_present(GTK_WINDOW(win));
}

int main(int argc, char **argv) {
  int i = 1;
  if (i < argc) mode = argv[i++];
  if (!strcmp(mode, "text")) {
    if (i >= argc) { fprintf(stderr, "text FILE\n"); return 2; }
    file = argv[i++];
  } else if (i < argc && argv[i][0] != '-') {
    rows = atoi(argv[i++]);
  }
  for (; i < argc; i++) {
    if (!strcmp(argv[i], "--scroll") && i + 2 < argc) { scroll_rows = atoi(argv[i + 1]); scroll_ms = atoi(argv[i + 2]); i += 2; }
    else if (!strcmp(argv[i], "--size") && i + 2 < argc) { win_w = atoi(argv[i + 1]); win_h = atoi(argv[i + 2]); i += 2; }
    else { fprintf(stderr, "unknown argument %s\n", argv[i]); return 2; }
  }
#ifdef ADW
  AdwApplication *app = adw_application_new(APP_ID, G_APPLICATION_NON_UNIQUE);
#else
  GtkApplication *app = gtk_application_new(APP_ID, G_APPLICATION_NON_UNIQUE);
#endif
  g_signal_connect(app, "activate", G_CALLBACK(activate), NULL);
  char *args[] = {argv[0], NULL};
  int status = g_application_run(G_APPLICATION(app), 1, args);
  g_object_unref(app);
  return status;
}
