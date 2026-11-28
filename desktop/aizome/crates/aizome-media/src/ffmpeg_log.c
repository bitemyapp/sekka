#include <libavutil/log.h>
#include <stddef.h>
#include <string.h>

extern int aizome_ffmpeg_log_enabled(int level);
extern void aizome_ffmpeg_log(int level, const char *message, size_t length);

/* Keep va_list on the C side: its ABI differs between x86 and ARM. Each
 * invocation owns its buffer, including concurrent decoder callbacks. */
static void callback(void *context, int level, const char *format, va_list args)
{
    if (!aizome_ffmpeg_log_enabled(level))
        return;
    char line[4096];
    int prefix = 1;
    av_log_format_line2(context, level, format, args, line, sizeof(line), &prefix);
    line[sizeof(line) - 1] = '\0';
    aizome_ffmpeg_log(level, line, strlen(line));
}

void aizome_ffmpeg_install_log(void)
{
    av_log_set_callback(callback);
    /* Avoid even dispatching verbose per-frame messages in ordinary use. */
    av_log_set_level(aizome_ffmpeg_log_enabled(AV_LOG_TRACE) ? AV_LOG_TRACE :
                     aizome_ffmpeg_log_enabled(AV_LOG_INFO) ? AV_LOG_INFO : AV_LOG_ERROR);
}
