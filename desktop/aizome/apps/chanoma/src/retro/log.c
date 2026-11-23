/* A core's log, as libretro has it: a function taking printf's arguments,
 * which Rust can't write. Each line is formatted here and handed to
 * Chanoma's own log (chanoma_core_said, in host.rs). */

#include <stdarg.h>
#include <stdio.h>

void chanoma_core_said(unsigned level, const char *line);

void chanoma_core_log(unsigned level, const char *fmt, ...)
{
    char line[1024];
    va_list args;

    if (fmt == NULL)
        return;
    va_start(args, fmt);
    vsnprintf(line, sizeof line, fmt, args);
    va_end(args);
    chanoma_core_said(level, line);
}
