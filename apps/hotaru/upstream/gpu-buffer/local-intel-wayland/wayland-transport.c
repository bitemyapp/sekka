/* Test-only private-compositor DMA-BUF ownership diagnostic. Not product code. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "linux-dmabuf-client.h"
#include "layer-shell-client.h"
#include "screencopy-client.h"
#include "presentation-client.h"
static struct wl_display *d;
static struct wl_compositor *compositor;
static struct wl_output *output;
static struct wl_shm *shm;
static struct zwp_linux_dmabuf_v1 *dmabuf;
static struct zwlr_layer_shell_v1 *shell;
static struct zwlr_screencopy_manager_v1 *copies;
static struct wp_presentation *presentation;
static struct wl_buffer *imported, *capture;
static struct wl_surface *surface;
static bool created, configured, frame_done, captured, detached, released, feedback_done;
static uint32_t presentation_clock = UINT32_MAX, capture_flags, cf, cw, ch, cs;
static uint8_t *pixels;
static size_t allocation;
struct advertised { uint32_t format; uint64_t modifier; };
static struct advertised formats[4096];
static size_t format_count;
static uint64_t now(void) { struct timespec t; if (clock_gettime(CLOCK_MONOTONIC,&t)) exit(2); return (uint64_t)t.tv_sec*1000000000u+(uint64_t)t.tv_nsec; }
static void fail(const char *s) { printf("{\"event\":\"wayland_failure\",\"stage\":\"%s\",\"errno\":%d}\n",s,errno); exit(2); }
static void pump(bool *done) {
    uint64_t deadline=now()+UINT64_C(5000000000);
    while (!*done) {
        if (wl_display_dispatch_pending(d)<0) fail("dispatch");
        if (*done) break;
        if (now()>=deadline) fail("phase timeout");
        while (wl_display_prepare_read(d)!=0) {
            if (now()>=deadline) fail("prepare phase timeout");
            if (wl_display_dispatch_pending(d)<0) fail("prepare");
            if (*done) return;
        }
        int flush=wl_display_flush(d);
        if (flush<0 && errno!=EAGAIN) { wl_display_cancel_read(d); fail("flush"); }
        struct pollfd p={wl_display_get_fd(d),POLLIN|(flush<0?POLLOUT:0),0};
        int rc=poll(&p,1,50);
        if (rc<0) { wl_display_cancel_read(d); if(errno==EINTR) continue; fail("poll"); }
        if (p.revents&POLLIN) { if(wl_display_read_events(d)<0) fail("read"); }
        else wl_display_cancel_read(d);
        if(p.revents&(POLLERR|POLLHUP|POLLNVAL)) fail("disconnect");
    }
}
static void synced(void *data,struct wl_callback *o,uint32_t time) { (void)time; *(bool*)data=true; wl_callback_destroy(o); }
static const struct wl_callback_listener sync_listener={.done=synced};
static void barrier(void) { bool done=false; struct wl_callback *cb=wl_display_sync(d); wl_callback_add_listener(cb,&sync_listener,&done); pump(&done); }
static void fmt(void *data,struct zwp_linux_dmabuf_v1 *o,uint32_t f) { (void)data;(void)o;(void)f; }
static void mod(void *data,struct zwp_linux_dmabuf_v1 *o,uint32_t f,uint32_t hi,uint32_t lo) {
    (void)data;(void)o; if(format_count==4096) fail("format limit");
    formats[format_count++]=(struct advertised){f,((uint64_t)hi<<32)|lo};
}
static const struct zwp_linux_dmabuf_v1_listener dmabuf_listener={.format=fmt,.modifier=mod};
static void clock_id(void *data,struct wp_presentation *o,uint32_t id) { (void)data;(void)o; presentation_clock=id; }
static const struct wp_presentation_listener presentation_listener={.clock_id=clock_id};
static void global(void *data,struct wl_registry *r,uint32_t id,const char *name,uint32_t v) {
    (void)data;
    printf("{\"event\":\"wayland_global\",\"name\":\"%s\",\"version\":%u}\n",name,v);
    if(!strcmp(name,"wl_compositor")) compositor=wl_registry_bind(r,id,&wl_compositor_interface,v<4?v:4);
    else if(!strcmp(name,"wl_output")&&!output) output=wl_registry_bind(r,id,&wl_output_interface,1);
    else if(!strcmp(name,"wl_shm")) shm=wl_registry_bind(r,id,&wl_shm_interface,1);
    else if(!strcmp(name,"zwlr_layer_shell_v1")) shell=wl_registry_bind(r,id,&zwlr_layer_shell_v1_interface,1);
    else if(!strcmp(name,"zwlr_screencopy_manager_v1")&&v>=3) copies=wl_registry_bind(r,id,&zwlr_screencopy_manager_v1_interface,3);
    else if(!strcmp(name,"zwp_linux_dmabuf_v1")&&v>=3) { dmabuf=wl_registry_bind(r,id,&zwp_linux_dmabuf_v1_interface,3); zwp_linux_dmabuf_v1_add_listener(dmabuf,&dmabuf_listener,NULL); }
    else if(!strcmp(name,"wp_presentation")) { presentation=wl_registry_bind(r,id,&wp_presentation_interface,1); wp_presentation_add_listener(presentation,&presentation_listener,NULL); }
}
static void removed(void *data,struct wl_registry *r,uint32_t id) {(void)data;(void)r;(void)id;}
static const struct wl_registry_listener registry_listener={.global=global,.global_remove=removed};
static void release(void *data,struct wl_buffer *o) { (void)data;(void)o; released=true; printf("{\"event\":\"buffer_release\",\"after_detach\":%s,\"received_monotonic_ns\":%"PRIu64"}\n",detached?"true":"false",now()); }
static const struct wl_buffer_listener release_listener={.release=release};
static void made(void *data,struct zwp_linux_buffer_params_v1 *p,struct wl_buffer *b) { (void)data;(void)p; imported=b; wl_buffer_add_listener(b,&release_listener,NULL); created=true; puts("{\"event\":\"params_created\"}"); }
static void rejected(void *data,struct zwp_linux_buffer_params_v1 *p) {(void)data;(void)p;fail("asynchronous dmabuf import failed");}
static const struct zwp_linux_buffer_params_v1_listener params_listener={.created=made,.failed=rejected};
static void configure(void *data,struct zwlr_layer_surface_v1 *s,uint32_t serial,uint32_t w,uint32_t h) { (void)data; if(w!=32||h!=32)fail("layer geometry"); zwlr_layer_surface_v1_ack_configure(s,serial); configured=true; printf("{\"event\":\"layer_configure\",\"width\":%u,\"height\":%u}\n",w,h); }
static void closed(void *data,struct zwlr_layer_surface_v1 *s) {(void)data;(void)s;fail("layer closed");}
static const struct zwlr_layer_surface_v1_listener layer_listener={.configure=configure,.closed=closed};
static void frame(void *data,struct wl_callback *o,uint32_t milliseconds) { (void)data; frame_done=true; wl_callback_destroy(o); printf("{\"event\":\"surface_frame\",\"callback_ms_unspecified_epoch\":%u,\"received_monotonic_ns\":%"PRIu64"}\n",milliseconds,now()); }
static const struct wl_callback_listener frame_listener={.done=frame};
static void sync_output(void *data,struct wp_presentation_feedback *o,struct wl_output *out) {(void)data;(void)o;(void)out;}
static void presented(void *data,struct wp_presentation_feedback *o,uint32_t hi,uint32_t lo,uint32_t ns,uint32_t refresh,uint32_t seqhi,uint32_t seqlo,uint32_t flags) {
    (void)data; feedback_done=true;
    printf("{\"event\":\"presentation_feedback\",\"clock_id\":%u,\"seconds_hi\":%u,\"seconds_lo\":%u,\"nanoseconds\":%u,\"refresh_ns\":%u,\"sequence_hi\":%u,\"sequence_lo\":%u,\"flags\":%u,\"physical_scanout\":false}\n",presentation_clock,hi,lo,ns,refresh,seqhi,seqlo,flags);
    wp_presentation_feedback_destroy(o);
}
static void discarded(void *data,struct wp_presentation_feedback *o) {(void)data;wp_presentation_feedback_destroy(o);fail("presentation discarded");}
static const struct wp_presentation_feedback_listener feedback_listener={.sync_output=sync_output,.presented=presented,.discarded=discarded};
static void copy_buffer(void *data,struct zwlr_screencopy_frame_v1 *o,uint32_t f,uint32_t w,uint32_t h,uint32_t stride) {
    (void)data;(void)o;
    if(f!=WL_SHM_FORMAT_XRGB8888&&f!=WL_SHM_FORMAT_ARGB8888&&f!=WL_SHM_FORMAT_XBGR8888&&f!=WL_SHM_FORMAT_ABGR8888)fail("capture format");
    if(w<32||h<32||w>1024||h>1024||stride<w*4||(uint64_t)stride*h>16*1024*1024)fail("capture size");
    cf=f;cw=w;ch=h;cs=stride;allocation=(size_t)stride*h;
    int fd=memfd_create("hotaru-gpu-capture",MFD_CLOEXEC);if(fd<0||ftruncate(fd,(off_t)allocation))fail("capture memfd");
    pixels=mmap(NULL,allocation,PROT_READ|PROT_WRITE,MAP_SHARED,fd,0);if(pixels==MAP_FAILED)fail("capture mmap");
    struct wl_shm_pool *pool=wl_shm_create_pool(shm,fd,(int)allocation);
    capture=wl_shm_pool_create_buffer(pool,0,(int)w,(int)h,(int)stride,f);wl_shm_pool_destroy(pool);close(fd);
}
static void copy_flags(void *data,struct zwlr_screencopy_frame_v1 *o,uint32_t flags) {(void)data;(void)o;capture_flags=flags;}
static void copy_ready(void *data,struct zwlr_screencopy_frame_v1 *o,uint32_t hi,uint32_t lo,uint32_t ns) {
    (void)data;
    static const uint8_t colors[4][3]={{255,0,0},{0,255,0},{0,0,255},{255,255,255}};
    uint32_t x0=(cw-32)/2,y0=(ch-32)/2;
    for(uint32_t y=0;y<32;y++)for(uint32_t x=0;x<32;x++) {
        uint32_t sy=y0+y;if(capture_flags&ZWLR_SCREENCOPY_FRAME_V1_FLAGS_Y_INVERT)sy=ch-1-sy;
        const uint8_t *p=pixels+(size_t)sy*cs+(x0+x)*4;
        uint8_t r=p[2],g=p[1],b=p[0];
        if(cf==WL_SHM_FORMAT_XBGR8888||cf==WL_SHM_FORMAT_ABGR8888){r=p[0];b=p[2];}
        /* Normal DMA-BUF row order: the producer readback's first row must
         * become the surface top row. Future GL scene renderers must choose
         * their projection/export orientation explicitly; no Y flag is used. */
        const uint8_t *expect=colors[(x>=16)+2*(y>=16)];
        if(r!=expect[0]||g!=expect[1]||b!=expect[2]) {printf("{\"event\":\"capture_mismatch\",\"x\":%u,\"y\":%u,\"rgb\":[%u,%u,%u]}\n",x,y,r,g,b);fail("captured pixel mismatch");}
        if((cf==WL_SHM_FORMAT_ARGB8888||cf==WL_SHM_FORMAT_ABGR8888)&&p[3]!=255)fail("captured alpha");
    }
    const char *path=getenv("HOTARU_GPU_CAPTURE_PATH");if(!path)fail("capture path");FILE *file=fopen(path,"wb");if(!file||fwrite(pixels,1,allocation,file)!=allocation||fclose(file))fail("capture save");
    printf("{\"event\":\"capture_verified\",\"checked_rgb_bytes\":3072,\"alpha_defined\":%s,\"format\":%u,\"width\":%u,\"height\":%u,\"stride\":%u,\"flags\":%u,\"x\":%u,\"y\":%u,\"compositor_sec_hi\":%u,\"compositor_sec_lo\":%u,\"compositor_nsec\":%u,\"received_monotonic_ns\":%"PRIu64"}\n",(cf==WL_SHM_FORMAT_ARGB8888||cf==WL_SHM_FORMAT_ABGR8888)?"true":"false",cf,cw,ch,cs,capture_flags,x0,y0,hi,lo,ns,now());
    captured=true;zwlr_screencopy_frame_v1_destroy(o);
}
static void copy_failed(void *data,struct zwlr_screencopy_frame_v1 *o){(void)data;(void)o;fail("screencopy failed");}
static void copy_damage(void *data,struct zwlr_screencopy_frame_v1 *o,uint32_t x,uint32_t y,uint32_t w,uint32_t h){(void)data;(void)o;(void)x;(void)y;(void)w;(void)h;}
static void copy_dma(void *data,struct zwlr_screencopy_frame_v1 *o,uint32_t f,uint32_t w,uint32_t h){(void)data;(void)o;(void)f;(void)w;(void)h;}
static void copy_done(void *data,struct zwlr_screencopy_frame_v1 *o){(void)data;if(!capture)fail("SHM capture unavailable");zwlr_screencopy_frame_v1_copy(o,capture);}
static const struct zwlr_screencopy_frame_v1_listener copy_listener={.buffer=copy_buffer,.flags=copy_flags,.ready=copy_ready,.failed=copy_failed,.damage=copy_damage,.linux_dmabuf=copy_dma,.buffer_done=copy_done};
void hotaru_probe_wayland_present(int fd,uint32_t fourcc,uint32_t stride,uint32_t offset,uint64_t modifier) {
    d=wl_display_connect(NULL);if(!d)fail("private display connect");
    struct wl_registry *registry=wl_display_get_registry(d);wl_registry_add_listener(registry,&registry_listener,NULL);barrier();barrier();
    if(!compositor||!output||!shm||!dmabuf||!shell||!copies)fail("required protocols");
    bool advertised=false;for(size_t i=0;i<format_count;i++)if(formats[i].format==fourcc&&formats[i].modifier==modifier)advertised=true;
    printf("{\"event\":\"format_modifier_check\",\"advertised_pairs\":%zu,\"exact_match\":%s,\"fourcc\":%u,\"modifier\":\"0x%016"PRIx64"\"}\n",format_count,advertised?"true":"false",fourcc,modifier);if(!advertised)fail("format modifier not advertised");
    int transport_fd=fcntl(fd,F_DUPFD_CLOEXEC,0);if(transport_fd<0)fail("transport fd dup");
    struct zwp_linux_buffer_params_v1 *params=zwp_linux_dmabuf_v1_create_params(dmabuf);zwp_linux_buffer_params_v1_add_listener(params,&params_listener,NULL);
    zwp_linux_buffer_params_v1_add(params,transport_fd,0,offset,stride,(uint32_t)(modifier>>32),(uint32_t)modifier);
    zwp_linux_buffer_params_v1_create(params,32,32,fourcc,0);pump(&created);
    if(close(transport_fd)) { fail("transport fd close"); }
    zwp_linux_buffer_params_v1_destroy(params);
    puts("{\"event\":\"transport_fd_closed_after_created\",\"y_invert\":false,\"orientation\":\"producer_readback_row0_to_surface_top\"}");
    surface=wl_compositor_create_surface(compositor);
    struct zwlr_layer_surface_v1 *layer=zwlr_layer_shell_v1_get_layer_surface(shell,surface,output,ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY,"hotaru-gpu-probe");
    zwlr_layer_surface_v1_add_listener(layer,&layer_listener,NULL);zwlr_layer_surface_v1_set_size(layer,32,32);zwlr_layer_surface_v1_set_exclusive_zone(layer,-1);wl_surface_commit(surface);pump(&configured);
    struct wl_callback *cb=wl_surface_frame(surface);wl_callback_add_listener(cb,&frame_listener,NULL);
    if(presentation){struct wp_presentation_feedback *f=wp_presentation_feedback(presentation,surface);wp_presentation_feedback_add_listener(f,&feedback_listener,NULL);}
    wl_surface_attach(surface,imported,0,0);wl_surface_damage(surface,0,0,32,32);wl_surface_commit(surface);pump(&frame_done);if(presentation)pump(&feedback_done);
    struct zwlr_screencopy_frame_v1 *copy=zwlr_screencopy_manager_v1_capture_output(copies,0,output);zwlr_screencopy_frame_v1_add_listener(copy,&copy_listener,NULL);pump(&captured);
    if(released)fail("buffer released before deliberate detach");
    detached=true;wl_surface_attach(surface,NULL,0,0);wl_surface_commit(surface);pump(&released);
    wl_buffer_destroy(imported);wl_buffer_destroy(capture);if(munmap(pixels,allocation))fail("capture munmap");
    zwlr_layer_surface_v1_destroy(layer);wl_surface_destroy(surface);barrier();
    if(presentation) { wp_presentation_destroy(presentation); }
    zwlr_screencopy_manager_v1_destroy(copies);zwlr_layer_shell_v1_destroy(shell);zwp_linux_dmabuf_v1_destroy(dmabuf);wl_output_destroy(output);wl_shm_destroy(shm);wl_compositor_destroy(compositor);wl_registry_destroy(registry);wl_display_disconnect(d);
    printf("{\"event\":\"wayland_transport_passed\",\"after_detach_release\":true,\"presentation_protocol\":%s}\n", presentation ? "true" : "false");
}
