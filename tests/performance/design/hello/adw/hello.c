/* A label and a button in a 480 x 200 window, in libadwaita (GNOME's apps). */
#include <adwaita.h>

static void activate(GtkApplication *app, gpointer data) {
  (void)data;
  GtkWidget *win = adw_application_window_new(app);
  gtk_window_set_title(GTK_WINDOW(win), "Hello");
  gtk_window_set_default_size(GTK_WINDOW(win), 480, 200);
  GtkWidget *view = adw_toolbar_view_new();
  adw_toolbar_view_add_top_bar(ADW_TOOLBAR_VIEW(view), adw_header_bar_new());
  GtkWidget *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 12);
  gtk_widget_set_margin_start(box, 16); gtk_widget_set_margin_end(box, 16);
  gtk_widget_set_margin_top(box, 16); gtk_widget_set_margin_bottom(box, 16);
  gtk_box_append(GTK_BOX(box), gtk_label_new("Hello, world"));
  gtk_box_append(GTK_BOX(box), gtk_button_new_with_label("Press me"));
  adw_toolbar_view_set_content(ADW_TOOLBAR_VIEW(view), box);
  adw_application_window_set_content(ADW_APPLICATION_WINDOW(win), view);
  gtk_window_present(GTK_WINDOW(win));
}

int main(int argc, char **argv) {
  AdwApplication *app = adw_application_new("org.example.HelloAdw", G_APPLICATION_NON_UNIQUE);
  g_signal_connect(app, "activate", G_CALLBACK(activate), NULL);
  int status = g_application_run(G_APPLICATION(app), argc, argv);
  g_object_unref(app);
  return status;
}
