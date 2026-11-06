/* Copyright 2026 Chris Kearney
 * SPDX-License-Identifier: MIT */
#include "mesa-m3-profile.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
#include <xf86drm.h>

_Static_assert(sizeof(struct drm_asahi_cmd_render) == 320, "render ABI");
_Static_assert(sizeof(struct drm_asahi_cmd_compute) == 128, "compute ABI");

int main(int argc, char **argv)
{
   if (argc != 3 || (strcmp(argv[1], "legacy") && strcmp(argv[1], "j613-25g83-hal200"))) {
      fprintf(stderr, "usage: mesa-m3-abi-check legacy|j613-25g83-hal200 RENDER_NODE\n");
      return 2;
   }
   int fd = open(argv[2], O_RDWR | O_CLOEXEC);
   if (fd < 0) {
      fprintf(stderr, "open: %s\n", strerror(errno));
      return 1;
   }
   drmVersionPtr version = drmGetVersion(fd);
   bool asahi = version && version->name && !strcmp(version->name, "asahi");
   drmFreeVersion(version);
   struct drm_asahi_params_global p = {0};
   struct drm_asahi_get_params query = {
      .param_group = 0, .pointer = (uintptr_t)&p, .size = sizeof(p),
   };
   int ret = asahi ? drmIoctl(fd, DRM_IOCTL_ASAHI_GET_PARAMS, &query) : -1;
   close(fd);
   if (ret || query.size < sizeof(p)) {
      fprintf(stderr, "params: rejected driver or short reply\n");
      return 1;
   }
   bool matches = mesa_m3_profile_matches(&p, argv[1]);
   printf("profile=%s chip=0x%x G%u%c rev=0x%x dies=%u clusters=%u USC%u HAL%u params=%llu match=%d\n",
          argv[1], p.chip_id, p.gpu_generation, p.gpu_variant, p.gpu_revision,
          p.num_dies, p.num_clusters_total, p.usc_generation, p.gpu_hal_generation,
          (unsigned long long)query.size, matches);
   return matches ? 0 : 1;
}
