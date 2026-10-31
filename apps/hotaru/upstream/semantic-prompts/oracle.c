/* Development-only adapter to unchanged pinned libghostty-vt. */
#include <ghostty/vt.h>
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
static void check(GhosttyResult r) { if(r!=GHOSTTY_SUCCESS) { fprintf(stderr,"API error %d\n",r); exit(2); } }
static unsigned number(const char *s,unsigned max) { char *e; unsigned long v=strtoul(s,&e,10); if(!*s||*e||!v||v>max)exit(3); return v; }
static uint32_t read_le(FILE *f,unsigned n) { uint32_t v=0; for(unsigned i=0;i<n;i++) { int c=fgetc(f); if(c==EOF)exit(4);v|=(uint32_t)c<<(8*i); } return v; }
int main(int argc,char **argv) {
 if(argc!=6)return 3;
 unsigned cols=number(argv[2],8192),rows=number(argv[3],4096),chunk=number(argv[4],65536); bool unicode=!strcmp(argv[5],"unicode");
 FILE *f=fopen(argv[1],"rb"); if(!f)return 4;
 GhosttyTerminal term;check(ghostty_terminal_new(NULL,&term,cols,rows));
 size_t history=256,bytes=128*1024*1024;
 check(ghostty_terminal_set(term,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,&history));
 check(ghostty_terminal_set(term,GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,&bytes));
 GhosttyTerminalModeConfig config={.mode=2027,.value=unicode};check(ghostty_terminal_set(term,GHOSTTY_TERMINAL_OPT_MODE_DEFAULT,&config));
 int op; while((op=fgetc(f))!=EOF) {
  if(op=='F') { size_t n=read_le(f,4);if(n>1024*1024)exit(5);uint8_t *p=malloc(n?n:1);if(!p)exit(6);if(fread(p,1,n,f)!=n)exit(4);
   for(size_t pos=0;pos<n;) { size_t take=n-pos<chunk?n-pos:chunk;ghostty_terminal_vt_write(term,p+pos,take);pos+=take; } free(p);
  } else if(op=='R') { cols=read_le(f,2);rows=read_le(f,2);check(ghostty_terminal_resize(term,cols,rows,0,0));
  } else if(op=='C') { GhosttyTerminalCompressionResult result;check(ghostty_terminal_compress(term,GHOSTTY_TERMINAL_COMPRESSION_MODE_FULL,&result));
  } else exit(7);
 }
 fclose(f);bool error=false;check(ghostty_terminal_get(term,GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR,&error));if(error)exit(8);
 size_t retained=0;uint16_t x=0,y=0;check(ghostty_terminal_get(term,GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,&retained));
 check(ghostty_terminal_get(term,GHOSTTY_TERMINAL_DATA_CURSOR_X,&x));check(ghostty_terminal_get(term,GHOSTTY_TERMINAL_DATA_CURSOR_Y,&y));
 printf("{\"cols\":%u,\"rows\":%u,\"cursor\":[%u,%zu],\"cells\":[",cols,rows,x,retained+y);
 for(size_t row=0;row<retained+rows;row++) { printf("%s[",row?",":"");
  for(unsigned col=0;col<cols;col++) { GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=col,.y=row}};
   GhosttyGridRef ref={.size=sizeof ref};check(ghostty_terminal_grid_ref(term,p,&ref));GhosttyCell cell;check(ghostty_grid_ref_cell(&ref,&cell));
   uint32_t cp=0;GhosttyCellSemanticContent semantic;GhosttyCellWide wide;bool protected=false;
   check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_CODEPOINT,&cp));check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_SEMANTIC_CONTENT,&semantic));
   check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_WIDE,&wide));check(ghostty_cell_get(cell,GHOSTTY_CELL_DATA_PROTECTED,&protected));
   unsigned width=wide==GHOSTTY_CELL_WIDE_WIDE?2:wide==GHOSTTY_CELL_WIDE_SPACER_TAIL?0:1;
   printf("%s[%u,%d,%u,%u]",col?",":"",cp?cp:32,semantic,width,protected?1:0);
  } putchar(']');
 } printf("],\"row_prompts\":[");
 for(size_t row=0;row<retained+rows;row++) {
  GhosttyPoint p={.tag=GHOSTTY_POINT_TAG_SCREEN,.value.coordinate={.x=0,.y=row}};
  GhosttyGridRef ref={.size=sizeof ref};check(ghostty_terminal_grid_ref(term,p,&ref));
  GhosttyRow value;GhosttyRowSemanticPrompt prompt;check(ghostty_grid_ref_row(&ref,&value));
  check(ghostty_row_get(value,GHOSTTY_ROW_DATA_SEMANTIC_PROMPT,&prompt));printf("%s%d",row?",":"",prompt);
 } puts("]}");ghostty_terminal_free(term);return 0;
}
