/* Development-only Linux50MB history/RSS audit against pinned libghostty-vt. */
#define _POSIX_C_SOURCE 200809L
#include <ghostty/vt.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <inttypes.h>
#include <time.h>
static void check(GhosttyResult result){if(result!=GHOSTTY_SUCCESS){fprintf(stderr,"API failure%d\n",result);exit(1);}}
static uint64_t now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return(uint64_t)t.tv_sec*1000000000+t.tv_nsec;}
static unsigned long rss(void){FILE*f=fopen("/proc/self/status","r");char line[256];unsigned long n=0;while(fgets(line,sizeof line,f)){if(sscanf(line,"VmRSS: %lu kB",&n)==1)break;}fclose(f);return n;}
static void number(uint64_t*h,uint64_t value){for(int i=0;i<8;i++){*h=(*h^(value&255))*UINT64_C(0x100000001b3);value>>=8;}}
static void color(uint64_t*h,GhosttyStyleColor c){number(h,c.tag);if(c.tag==GHOSTTY_STYLE_COLOR_PALETTE)number(h,c.value.palette);else if(c.tag==GHOSTTY_STYLE_COLOR_RGB){number(h,c.value.rgb.r);number(h,c.value.rgb.g);number(h,c.value.rgb.b);}}
static GhosttyGridRef ref_at(GhosttyTerminal terminal,size_t x,size_t y){GhosttyPoint point={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=x,.y=y}};GhosttyGridRef ref={.size=sizeof ref};check(ghostty_terminal_grid_ref(terminal,point,&ref));return ref;}
static uint64_t digest(GhosttyTerminal terminal,size_t cols,size_t rows){
 uint64_t hash=UINT64_C(0xcbf29ce484222325);number(&hash,cols);number(&hash,rows);
 for(size_t y=0;y<rows;y++)for(size_t x=0;x<cols;x++){
  GhosttyGridRef ref=ref_at(terminal,x,y);GhosttyCell cell;check(ghostty_grid_ref_cell(&ref,&cell));
  GhosttyCellWide wide;bool protected=false;check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_WIDE,&wide));check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_PROTECTED,&protected));
  number(&hash,wide==GHOSTTY_CELL_WIDE_SPACER_HEAD?0:wide);number(&hash,protected);
  uint32_t graphemes[65];size_t length=0;check(ghostty_grid_ref_graphemes(&ref,graphemes,65,&length));
  if(!length){length=1;graphemes[0]=32;}number(&hash,length);for(size_t i=0;i<length;i++)number(&hash,graphemes[i]?graphemes[i]:32);
  GhosttyStyle style={.size=sizeof style};check(ghostty_grid_ref_style(&ref,&style));
  bool flags[]={style.bold,style.italic,style.faint,style.blink,style.inverse,style.invisible,style.strikethrough,style.overline};for(size_t i=0;i<8;i++)number(&hash,flags[i]);
  number(&hash,style.underline);color(&hash,style.fg_color);color(&hash,style.bg_color);color(&hash,style.underline_color);
  uint8_t uri[2048];length=0;check(ghostty_grid_ref_hyperlink_uri(&ref,uri,sizeof uri,&length));number(&hash,length);for(size_t i=0;i<length;i++)number(&hash,uri[i]);
 }
 return hash;
}
int main(int argc,char**argv){
 if(argc!=7)return 2;
 unsigned long base=rss();FILE*f=fopen(argv[1],"rb");if(!f)return 3;fseek(f,0,SEEK_END);long length=ftell(f);rewind(f);char*data=malloc(length?length:1);if(!data||fread(data,1,length,f)!=(size_t)length)return 4;fclose(f);
 unsigned cols=strtoul(argv[2],0,10),rows=strtoul(argv[3],0,10);size_t row_limit=strtoull(argv[4],0,10),byte_limit=strtoull(argv[5],0,10);int compress=atoi(argv[6]);
 GhosttyTerminal terminal;check(ghostty_terminal_new(NULL,&terminal,cols,rows));check(ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,&row_limit));check(ghostty_terminal_set(terminal,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,&byte_limit));
 uint64_t start=now();for(size_t off=0;off<(size_t)length;){size_t n=(size_t)length-off;if(n>65536)n=65536;ghostty_terminal_vt_write(terminal,(uint8_t*)data+off,n);off+=n;}uint64_t feed=now()-start;free(data);unsigned long populated_rss=rss();
 size_t retained=0;check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,&retained));uint64_t validation=digest(terminal,cols,retained+rows);
 GhosttyTerminalMemoryUsage populated={.size=sizeof populated};check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,&populated));
 start=now();if(compress){GhosttyTerminalCompressionResult result;check(ghostty_terminal_compress(terminal,GHOSTTY_TERMINAL_COMPRESSION_MODE_FULL,&result));}uint64_t compression=now()-start;unsigned long cold_rss=rss();
 GhosttyTerminalMemoryUsage cold={.size=sizeof cold};check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,&cold));
 uint64_t checksum=0;start=now();for(size_t y=0;y<retained+rows;y++){GhosttyGridRef ref=ref_at(terminal,0,y);GhosttyCell cell;check(ghostty_grid_ref_cell(&ref,&cell));uint32_t cp=0;check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_CODEPOINT,&cp));checksum+=cp?cp:32;}uint64_t read=now()-start;unsigned long read_rss=rss();
 GhosttyTerminalMemoryUsage restored={.size=sizeof restored};check(ghostty_terminal_get(terminal,GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,&restored));if(digest(terminal,cols,retained+rows)!=validation)return 5;
 ghostty_terminal_free(terminal);unsigned long released_rss=rss();
 printf("{\"engine\":\"ghostty\",\"input_bytes\":%ld,\"cols\":%u,\"rows\":%u,\"row_limit\":%zu,\"byte_limit\":%zu,\"compressed\":%s,\"history_rows\":%zu,\"content_fnv1a64\":\"%016"PRIx64"\",\"checksum\":%"PRIu64",\"feed_ns\":%"PRIu64",\"compression_ns\":%"PRIu64",\"read_history_ns\":%"PRIu64",\"rss_base_kib\":%lu,\"rss_populated_kib\":%lu,\"rss_cold_kib\":%lu,\"rss_read_kib\":%lu,\"rss_released_kib\":%lu,\"page_resident_populated_bytes\":%"PRIu64",\"page_resident_cold_bytes\":%"PRIu64",\"page_resident_after_read_bytes\":%"PRIu64",\"compressed_pages\":%"PRIu64",\"compressed_bytes\":%"PRIu64"}\n",length,cols,rows,row_limit,byte_limit,compress?"true":"false",retained,validation,checksum,feed,compression,read,base,populated_rss,cold_rss,read_rss,released_rss,populated.primary_resident_bytes,cold.primary_resident_bytes,restored.primary_resident_bytes,cold.primary_compressed_pages,cold.primary_compressed_bytes);
}
