/* Compile-time checks against the exact engine header; no runtime bridge. */
#include <stddef.h>
#include "oniguruma.h"
_Static_assert(ONIGURUMA_VERSION_INT == 60909, "exact engine version");
_Static_assert(sizeof(int) == 4, "Rust c_int layout");
_Static_assert(offsetof(OnigRegion, allocated) == 0, "region allocated");
_Static_assert(offsetof(OnigRegion, num_regs) == 4, "region count");
_Static_assert(offsetof(OnigRegion, beg) == 8, "region begins");
_Static_assert(offsetof(OnigRegion, end) == 8 + sizeof(void*), "region ends");
_Static_assert(offsetof(OnigRegion, history_root) == 8 + 2*sizeof(void*), "region history");
_Static_assert(sizeof(OnigRegion) == 8 + 3*sizeof(void*), "region size");
