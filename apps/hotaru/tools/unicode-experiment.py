#!/usr/bin/env python3
"""Create isolated correctness-preserving width-cache/write-batch experiments.

The source is the unchanged 1d3c9cf Hotaru workspace archived from Git. This
tool never edits that source or the shared product workspace. Each destination
must be new. Build/fixture/timing checks are separate and mandatory.
"""
import argparse
from pathlib import Path
import shutil


RUN_METHOD = '''    /// Print a bounded positive-width run after preparing its complete range.
    pub(crate) fn put_run(&mut self,start:usize,run:&[(char,u8)],width:usize,style:Style,protected:bool,link:Option<&LinkRef>) {
        self.prepare_write(start,start+width,style);
        let id=self.style_id(style);
        let mut col=start;
        for &(ch,width) in run {
            self.cells[col]=Cell::new(ch,id,width).protected(protected);
            if width==2 {self.cells[col+1]=Cell::new(' ',id,0).protected(protected);}
            col+=usize::from(width);
        }
        if let Some(link)=link {self.set_hyperlink_range(start..start+width,Some(link));}
    }
'''

INNER_LOOP = '''            loop {
                let mut run=[('\\0',0u8);64];
                let mut count=0;
                let mut end=col;
                let mut stop_width=None;
                while count<run.len() {
                    let Some(&ch)=chars.peek() else {break;};
                    WIDTH
                    if width==0 || pending || end+width>s.cols {stop_width=Some(width);break;}
                    chars.next();
                    run[count]=(ch,width as u8);count+=1;end+=width;
                }
                if count!=0 {
                    let mut check_col=col;
                    for &(_,width) in &run[..count] {
                        if check_col>1 {break;}
                        clear_previous_spacer|=row.cells[check_col].width()!=usize::from(width)&&row.cells[check_col].width()!=1;
                        check_col+=usize::from(width);
                    }
                    row.put_run(col,&run[..count],end-col,style,protected,link.as_ref());
                    col=end;
                    if col==s.cols {col-=1;pending=true;}
                }
                if count==run.len() {continue;}
                if stop_width==Some(0) {
                    row.combine(if pending{col}else{col.saturating_sub(1)},chars.next().unwrap());
                    continue;
                }
                break;
            }
'''

WIDTH_CACHE = '''let width=if ch==last_char {last_width} else {
                        let width=char_width(ch);last_char=ch;last_width=width;width
                    };'''

STREAM_LOOP = '''            let mut run:Option<(usize,bool,u16)>=None;
            while let Some(&ch)=chars.peek() {
                WIDTH
                if width!=0 && (pending||col+width>s.cols) {break;}
                chars.next();
                if width==0 {
                    if let Some((start,left,_))=run.take() {
                        row.finish_run(start,if pending{s.cols}else{col},left,style,link.as_ref());
                    }
                    row.combine(if pending{col}else{col.saturating_sub(1)},ch);
                }else{
                    clear_previous_spacer|=col<=1&&row.cells[col].width()!=width&&row.cells[col].width()!=1;
                    let (_,_,id)=*run.get_or_insert_with(||(col,col>0&&row.cells[col].width()==0,row.style_id(style)));
                    row.cells[col]=Cell::new(ch,id,width as u8).protected(protected);
                    if width==2 {row.cells[col+1]=Cell::new(' ',id,0).protected(protected);}
                    col+=width;
                    if col==s.cols {col-=1;pending=true;}
                }
            }
            if let Some((start,left,_))=run {
                row.finish_run(start,if pending{s.cols}else{col},left,style,link.as_ref());
            }
'''

FINISH_METHOD = '''    pub(crate) fn finish_run(&mut self,start:usize,end:usize,left:bool,style:Style,link:Option<&LinkRef>) {
        self.prepare_write_with_left(start,end,left,style);
        if let Some(link)=link {self.set_hyperlink_range(start..end,Some(link));}
    }
'''

DIRECT_LOOP = '''            while let Some(&first)=chars.peek() {
                let mut ch=first;
                let mut width=char_width(ch);
                if width==0 {
                    chars.next();row.combine(if pending{col}else{col.saturating_sub(1)},ch);continue;
                }
                if pending||col+width>s.cols {break;}
                let start=col;
                let left=col>0&&row.cells[col].width()==0;
                let id=row.style_id(style);
                loop {
                    chars.next();
                    clear_previous_spacer|=col<=1&&row.cells[col].width()!=width&&row.cells[col].width()!=1;
                    row.cells[col]=Cell::new(ch,id,width as u8).protected(protected);
                    if width==2 {row.cells[col+1]=Cell::new(' ',id,0).protected(protected);}
                    col+=width;
                    if col==s.cols {col-=1;pending=true;break;}
                    let Some(&next)=chars.peek() else {break;};
                    ch=next;width=char_width(ch);
                    if width==0||col+width>s.cols {break;}
                }
                row.finish_run(start,if pending{s.cols}else{col},left,style,link.as_ref());
            }
'''

HAN_TEST = '''
#[cfg(test)]
mod width_fastpath_test {
    use super::*;
    #[test]
    fn fast_width_matches_existing_policy_for_every_unicode_scalar() {
        for value in 0..=0x10ffff {
            if let Some(ch)=char::from_u32(value) {
                let expected=if ch.is_ascii(){1}else if matches!(ch,'\\u{1f1e6}'..='\\u{1f1ff}'){2}else{ch.width().unwrap_or(0).min(2)};
                assert_eq!(char_width(ch),expected,"U+{value:04X}");
            }
        }
    }
}
'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,required=True)
    parser.add_argument('--destination',type=Path,required=True)
    parser.add_argument('--variant',choices=['width-cache','write-batch','batch-cache','stream-write','stream-cache','direct-stream','direct-han','stream-han'],required=True)
    args=parser.parse_args()
    if args.destination.exists():parser.error('destination must not exist')
    if args.destination.resolve().is_relative_to(args.source.resolve()):parser.error('destination must be outside source')
    shutil.copytree(args.source,args.destination,ignore=shutil.ignore_patterns('target','.git','__pycache__'))
    path=args.destination/'crates/hotaru-core/src/screen.rs'
    source=path.read_text();start=source.index('    pub fn print_utf8(');end=source.index('    fn move_to(',start)
    method=source[start:end]
    cache=args.variant in ('width-cache','batch-cache','stream-cache')
    if cache:method=method.replace('let mut chars = text.chars().peekable();','let mut chars = text.chars().peekable();\n        let mut last_char=\'\\0\';let mut last_width=0;')
    if args.variant=='width-cache':method=method.replace('let width = char_width(ch);',WIDTH_CACHE)
    else:
        first=method.index('            while let Some(&ch) = chars.peek() {')
        last=method.index('            s.cursor.col = col;',first)
        stream=args.variant.startswith(('stream','direct'))
        loop=DIRECT_LOOP if args.variant.startswith('direct') else STREAM_LOOP if stream else INNER_LOOP
        method=method[:first]+loop.replace('WIDTH',WIDTH_CACHE if cache else 'let width=char_width(ch);')+method[last:]
        row=args.destination/'crates/hotaru-core/src/row.rs';text=row.read_text()
        if stream:
            old='''    pub(crate) fn prepare_write(&mut self, start: usize, end: usize, style: Style) {
        let left = start > 0 && self.cells[start].width == 0;
'''
            new=old+'''        self.prepare_write_with_left(start,end,left,style);
    }
    fn prepare_write_with_left(&mut self,start:usize,end:usize,left:bool,style:Style) {
'''
            assert old in text;text=text.replace(old,new,1)
        text=text.replace('    pub(crate) fn combine(',(FINISH_METHOD if stream else RUN_METHOD)+'    pub(crate) fn combine(',1);row.write_text(text)
    source=source[:start]+method+source[end:]
    if args.variant.endswith('han'):
        source=source.replace("} else if matches!(ch, '\\u{1f1e6}'..='\\u{1f1ff}') {","} else if matches!(ch, '\\u{1f1e6}'..='\\u{1f1ff}' | '\\u{3400}'..='\\u{4dbf}' | '\\u{4e00}'..='\\u{9fff}' | '\\u{f900}'..='\\u{faff}') {")
        source+=HAN_TEST
    path.write_text(source)
    print(args.destination)


if __name__=='__main__':main()
