"""Run the backport's actual wire, gate and level-policy headers in userspace.

The small Linux header shims supply integer types and generic primitives;
no driver decisions are reimplemented here. This does not qualify a panel,
the RPC transport, framebuffer ownership or suspend on physical hardware.
"""
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

HEADERS = {
    'types.h': '#pragma once\n#include <stdint.h>\n#include <stdbool.h>\n'
               'typedef uint8_t u8; typedef uint32_t u32; typedef uint64_t u64;\n',
    'bitops.h': '#define BIT(n) (1UL << (n))\n'
                'static inline int fls(unsigned int n) { return n ? 32 - __builtin_clz(n) : 0; }\n',
    'build_bug.h': '#define static_assert _Static_assert\n',
    'errno.h': '#define EINVAL 22\n#define ERANGE 34\n#define ENODATA 61\n'
               '#define EBUSY 16\n#define EALREADY 114\n',
    'string.h': '#include <string.h>\n',
    'limits.h': '#include <limits.h>\n',
    'math64.h': '#define div_u64(n, d) ((n) / (d))\n'
                '#define mul_u32_u32(a, b) ((u64)(a) * (b))\n',
    'minmax.h': '#define min(a, b) ((a) < (b) ? (a) : (b))\n',
    'unaligned.h': '#include <stdint.h>\n'
                  '#define PUT(bits) static inline void put_unaligned_le##bits(uint##bits##_t v, void *p) { '
                  'for (unsigned i=0; i<bits/8; i++) ((uint8_t *)p)[i] = v >> (i*8); }\n'
                  'PUT(16) PUT(32) PUT(64)\n',
}

TEST = r'''
#include <assert.h>
#include <stdio.h>
#include "iomfb_v14_7_swap.h"
#include "iomfb_v14_7_backlight.h"
#include "dcp_backlight.h"

int main(void)
{
    u8 swap[DCP_V14_SWAP_SIZE], before[DCP_V14_SWAP_SIZE];
    for (u32 n = 0; n <= 500; n++) {
        memset(swap, 0xa5, sizeof(swap));
        memcpy(before, swap, sizeof(swap));
        assert(dcp_v14_encode_nits(swap, true, n) == 0);
        assert(swap[0x354] == 1);
        double value = n;
        u64 expected;
        memcpy(&expected, &value, sizeof(expected));
        for (unsigned i = 0; i < sizeof(swap); i++) {
            u8 byte = i == 0x354 ? 1 :
                i >= 0x35c && i < 0x364 ? expected >> (8 * (i - 0x35c)) : before[i];
            assert(swap[i] == byte);
        }
    }
    memcpy(before, swap, sizeof(swap));
    assert(dcp_v14_encode_nits(swap, true, 501) == -EINVAL);
    assert(memcmp(swap, before, sizeof(swap)) == 0);
    assert(dcp_v14_encode_nits(swap, false, 500) == 0);
    assert(swap[0x354] == 0);
    for (unsigned i = 0x35c; i < 0x364; i++) assert(swap[i] == 0);

    struct dcp_backlight_state state;
    struct dcp_backlight_present present;
    assert(dcp_bl_init(&state, 500, false, 0, true, 140) == 0);
    assert(dcp_bl_reported_actual(&state, true) == -ENODATA);
    assert(dcp_bl_prepare(&state, true, &present) == -EALREADY);
    assert(dcp_bl_request(&state, 501, false, false) == -ERANGE);
    assert(dcp_bl_request(&state, 250, false, false) == 0);
    assert(dcp_bl_prepare(&state, false, &present) == -ENODATA);
    assert(dcp_bl_prepare(&state, true, &present) == 0);
    assert(present.nits == 250);
    assert(!dcp_bl_complete(&state, present.sequence + 1, true));
    assert(dcp_bl_reported_actual(&state, true) == -ENODATA);
    assert(dcp_bl_complete(&state, present.sequence, false));
    assert(dcp_bl_reported_actual(&state, true) == -ENODATA);
    assert(dcp_bl_prepare(&state, true, &present) == 0);
    assert(dcp_bl_complete(&state, present.sequence, true));
    assert(dcp_bl_reported_actual(&state, true) == 250);

    struct dcp_v14_bl_facts f = {
        .gate = DCP_V14_BL_OPEN, .panel_nits = true, .started = true,
        .pending = true, .frame = true, .mode_valid = true,
    };
    assert(dcp_v14_bl_plan(&f) == DCP_V14_BL_REPRESENT);
    f.gate = dcp_v14_bl_gate_suspend(f.gate);
    assert(dcp_v14_bl_plan(&f) == DCP_V14_BL_SKIP);
    f.gate = dcp_v14_bl_gate_resume(f.gate);
    assert(dcp_v14_bl_plan(&f) == DCP_V14_BL_REPRESENT);
    assert(dcp_v14_bl_gate_resume(DCP_V14_BL_CLOSED) == DCP_V14_BL_CLOSED);
    f.typec = true;
    assert(dcp_v14_bl_plan(&f) == DCP_V14_BL_SKIP);
    f.typec = false;
    f.frame = false;
    assert(dcp_v14_bl_plan(&f) == DCP_V14_BL_WAIT);
    assert(!dcp_v14_bl_follow_dpms(true, true, false));
    assert(dcp_v14_bl_follow_dpms(true, true, true));
    puts("501 wire levels, bounds, presentation readback and work gates passed");
}
'''

if __name__ == '__main__':
    source = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / 'linux').mkdir()
        for name, text in HEADERS.items():
            (root / 'linux' / name).write_text(text)
        (root / 'test.c').write_text(TEST)
        subprocess.run([*shlex.split(os.environ.get('CC', 'cc')), '-std=c11', '-Wall', '-Wextra', '-Werror',
                        '-I', str(root), '-I', str(source), str(root / 'test.c'), '-o', str(root / 'test')],
                       check=True)
        subprocess.run([str(root / 'test')], check=True)
