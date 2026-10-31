#include <stddef.h>
#include <stdint.h>
#include <ghostty/vt/osc.h>

/* Diagnostic adapter only. The actual parser and allocations remain in the
 * unchanged pinned shared library. No command layout or parser is modeled. */
int osc52_parser_type(const uint8_t *input, size_t length, uint8_t terminator) {
    if (length > 8u * 1024u * 1024u + 64u) return -2;
    GhosttyOscParser parser = NULL;
    if (ghostty_osc_new(NULL, &parser) != GHOSTTY_SUCCESS) return -3;
    for (size_t i = 0; i < length; ++i) ghostty_osc_next(parser, input[i]);
    GhosttyOscCommand command = ghostty_osc_end(parser, terminator);
    int result = (int)ghostty_osc_command_type(command);
    ghostty_osc_free(parser);
    return result;
}
