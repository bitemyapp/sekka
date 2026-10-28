/* Real X11 requests, controlled by the integration test over stdin/stdout. */
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/Xatom.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc != 5) return 2;
    Display *display = XOpenDisplay(NULL);
    if (!display) return 3;
    Window root = DefaultRootWindow(display);
    Window window = XCreateSimpleWindow(display, root, 0, 0,
        atoi(argv[2]), atoi(argv[3]), 0, 0, 0x2255aa);
    XClassHint class = {argv[1], argv[1]};
    XSetClassHint(display, window, &class);
    XStoreName(display, window, argv[1]);
    if (!strcmp(argv[4], "borderless")) {
        unsigned long motif[] = {2, 0, 0, 0, 0};
        Atom hints = XInternAtom(display, "_MOTIF_WM_HINTS", False);
        XChangeProperty(display, window, hints, hints, 32, PropModeReplace,
            (unsigned char *)motif, 5);
    } else if (!strcmp(argv[4], "unmanaged")) {
        XSetWindowAttributes attrs = {.override_redirect = True};
        XChangeWindowAttributes(display, window, CWOverrideRedirect, &attrs);
    }
    Atom state = XInternAtom(display, "_NET_WM_STATE", False);
    Atom fullscreen = XInternAtom(display, "_NET_WM_STATE_FULLSCREEN", False);
    Atom hidden = XInternAtom(display, "_NET_WM_STATE_HIDDEN", False);
    Atom wm_state = XInternAtom(display, "WM_STATE", False);
    XSync(display, False);
    puts("ready");
    fflush(stdout);
    char command[64];
    while (fgets(command, sizeof command, stdin)) {
        command[strcspn(command, "\n")] = 0;
        if (!strcmp(command, "quit")) break;
        if (!strcmp(command, "map")) XMapWindow(display, window);
        else if (!strcmp(command, "unmap")) XUnmapWindow(display, window);
        else if (!strcmp(command, "minimize")) XIconifyWindow(display, window, DefaultScreen(display));
        else if (!strcmp(command, "restore-size")) XResizeWindow(display, window, atoi(argv[2]), atoi(argv[3]));
        else if (!strcmp(command, "property-full") || !strcmp(command, "property-normal")) {
            XChangeProperty(display, window, state, XA_ATOM, 32, PropModeReplace,
                (unsigned char *)&fullscreen, !strcmp(command, "property-full") ? 1 : 0);
        } else if (!strcmp(command, "property-delete")) {
            XDeleteProperty(display, window, state);
        } else if (!strcmp(command, "add") || !strcmp(command, "remove") || !strcmp(command, "toggle")) {
            XEvent event = {0};
            event.xclient.type = ClientMessage;
            event.xclient.display = display;
            event.xclient.window = window;
            event.xclient.message_type = state;
            event.xclient.format = 32;
            event.xclient.data.l[0] = !strcmp(command, "add") ? 1 : !strcmp(command, "remove") ? 0 : 2;
            event.xclient.data.l[1] = fullscreen;
            event.xclient.data.l[3] = 1;
            XSendEvent(display, root, False, SubstructureRedirectMask | SubstructureNotifyMask, &event);
        } else if (!strcmp(command, "state") || !strcmp(command, "hidden") || !strcmp(command, "iconic")) {
            Atom type;
            int format;
            unsigned long count, remaining;
            unsigned char *value = NULL;
            int present = 0;
            int iconic = !strcmp(command, "iconic");
            XGetWindowProperty(display, window, iconic ? wm_state : state, 0, 1024,
                False, iconic ? wm_state : XA_ATOM,
                &type, &format, &count, &remaining, &value);
            if (iconic && type == wm_state && format == 32 && count > 0) {
                present = ((long *)value)[0] == IconicState;
            } else if (!iconic && type == XA_ATOM && format == 32) {
                for (unsigned long i = 0; i < count; i++)
                    if (((Atom *)value)[i] == (!strcmp(command, "hidden") ? hidden : fullscreen)) present = 1;
            }
            if (value) XFree(value);
            printf("%d\n", present);
            fflush(stdout);
            continue;
        } else return 4;
        XSync(display, False);
        puts("done");
        fflush(stdout);
    }
    XDestroyWindow(display, window);
    XCloseDisplay(display);
    return 0;
}
