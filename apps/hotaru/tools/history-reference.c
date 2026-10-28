/* Development-only history memory/restore adapter to the pinned Ghostty library. */
#define _POSIX_C_SOURCE 200809L
#include <ghostty/vt.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <inttypes.h>
#include <string.h>
#include <time.h>
static void check(GhosttyResult value) { if(value!=GHOSTTY_SUCCESS){fprintf(stderr,"API failure %d\n",value);exit(1);} }
static uint64_t now(void) {struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return(uint64_t)t.tv_sec*1000000000+t.tv_nsec;}
static unsigned long rss(void) {FILE*f=fopen("/proc/self/status","r");char line[256];unsigned long n=0;while(fgets(line,sizeof line,f)){if(sscanf(line,"VmRSS: %lu kB",&n)==1)break;}fclose(f);return n;}
int main(int argc,char**argv) {
 if(argc!=6)return 2;
 FILE*f=fopen(argv[1],"rb");if(!f)return 3;fseek(f,0,SEEK_END);long len=ftell(f);rewind(f);char*data=malloc(len?len:1);if(!data||fread(data,1,len,f)!=(size_t)len)return 4;fclose(f);
 unsigned cols=strtoul(argv[2],0,10),rows=strtoul(argv[3],0,10);size_t history=strtoull(argv[4],0,10),max_bytes=128*1024*1024;int compress=atoi(argv[5]);
 unsigned long before=rss();GhosttyTerminal terminal;check(ghostty_terminal_new(NULL,&terminal,cols,rows));
 check(ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,&history));check(ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,&max_bytes));
 uint64_t start=now();for(size_t off=0;off<(size_t)len;){size_t n=(size_t)len-off;if(n>65536)n=65536;ghostty_terminal_vt_write(terminal,(uint8_t*)data+off,n);off+=n;}uint64_t feed=now()-start;
 unsigned long populated=rss();GhosttyTerminalMemoryUsage initial={.size=sizeof initial};check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,&initial));
 start=now();if(compress){GhosttyTerminalCompressionResult result;check(ghostty_terminal_compress(terminal,GHOSTTY_TERMINAL_COMPRESSION_MODE_FULL,&result));}uint64_t compression=now()-start;
 unsigned long compressed=rss();GhosttyTerminalMemoryUsage cold={.size=sizeof cold};check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,&cold));
 size_t retained=0;check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,&retained));uint64_t checksum=0;start=now();
 for(size_t y=0;y<retained+rows;y++){GhosttyPoint point={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=0,.y=y}};GhosttyGridRef ref={.size=sizeof ref};check(ghostty_terminal_grid_ref(terminal,point,&ref));GhosttyCell cell;check(ghostty_grid_ref_cell(&ref,&cell));uint32_t cp=0;check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_CODEPOINT,&cp));checksum+=cp?cp:32;}
 uint64_t read=now()-start;
 printf("{\"engine\":\"ghostty\",\"phase\":\"%s\",\"input_bytes\":%ld,\"cols\":%u,\"rows\":%u,\"history_rows\":%zu,\"feed_ns\":%"PRIu64",\"compression_ns\":%"PRIu64",\"read_history_ns\":%"PRIu64",\"rss_before_kib\":%lu,\"rss_populated_kib\":%lu,\"rss_cold_kib\":%lu,\"rss_read_kib\":%lu,\"storage_populated_bytes\":%"PRIu64",\"storage_cold_bytes\":%"PRIu64",\"compressed_pages\":%"PRIu64",\"checksum\":%"PRIu64"}\n",compress?"compressed":"uncompressed",len,cols,rows,retained,feed,compression,read,before,populated,compressed,rss(),initial.primary_resident_bytes,cold.primary_resident_bytes,cold.primary_compressed_pages,checksum);
 ghostty_terminal_free(terminal);free(data);
}
