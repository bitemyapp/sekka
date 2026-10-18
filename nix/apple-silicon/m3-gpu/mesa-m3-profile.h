/* Copyright 2026 Chris Kearney
 * SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <string.h>
#include "drm-uapi/asahi_drm.h"

/* Profile admission does not assume an enabled core count. The same command
 * ABI serves fused 8-core and 10-core J613 GPUs. */
static inline bool
mesa_m3_profile_matches(const struct drm_asahi_params_global *p,
                        const char *profile)
{
   if (p->gpu_generation != 15 || p->num_dies != 1 || p->usc_generation != 3)
      return false;
   if (!strcmp(profile, "j613-25g83-hal200"))
      return p->chip_id == 0x8122 && p->gpu_variant == 'G' &&
             p->gpu_revision == 0x20 && p->num_clusters_total == 1 &&
             p->gpu_hal_generation == DRM_ASAHI_GPU_HAL_200;
   if (!strcmp(profile, "legacy"))
      return p->gpu_hal_generation == DRM_ASAHI_GPU_HAL_LEGACY &&
             ((p->chip_id == 0x8122 && p->gpu_variant == 'G' &&
               p->num_clusters_total == 1) ||
              (p->chip_id == 0x6030 && p->gpu_variant == 'S' &&
               p->num_clusters_total == 2));
   return false;
}
