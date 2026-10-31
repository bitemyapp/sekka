use std::{collections::BTreeMap, sync::Arc, time::Duration};
use aizome_hotaru::app::{Hotaru,Msg};
use aizome_kit::{testing::Harness,theme::{Appearance,ThemeChoice},sidestep_ui::offscreen::{RecordedFrame,RecordedOp,GpuPrepare}};
use hotaru_core::{Terminal,GraphemeWidthMethod};

fn inventory(frame:&RecordedFrame, prepare:&mut GpuPrepare, number:usize)->serde_json::Value {
    let scene=frame.prepare_gpu(prepare).expect("whole representative scene supported");
    let software=frame.software_rgba();let lowered=scene.interpret();
    let diffs:Vec<_>=software.iter().zip(&lowered).enumerate().filter(|(_, (a,b))|a!=b).take(20).collect();
    if let Some((at,_))=diffs.first(){let p=(*at/4)as u32;let (x,y)=(p%frame.width,p/frame.width);eprintln!("frame {}x{} scale{} mismatch {x},{y}",frame.width,frame.height,frame.scale);for c in &scene.commands{if c.bounds[0]<=x&&c.bounds[1]<=y&&c.bounds[2]>x&&c.bounds[3]>y{eprintln!("cover {c:?}");}}}
    assert_eq!(software.len(),lowered.len());
    assert!(diffs.is_empty(),"exact scene lowering differs: {diffs:?}");
    if let Ok(dir)=std::env::var("HOTARU_SCENE_FIXTURES") {
        let dir=std::path::Path::new(&dir);std::fs::create_dir_all(dir).unwrap();
        let mut data=b"HTSCN001".to_vec();
        for value in [scene.width,scene.height,scene.commands.len()as u32,scene.assets.len()as u32,scene.tiles.len()as u32,scene.indices.len()as u32] {data.extend(value.to_le_bytes());}
        for c in &scene.commands {
            for value in c.bounds.into_iter().chain([c.kind,c.solid,c.alpha,c.asset.map_or(u32::MAX,|i|i as u32),c.origin[0]as u32,c.origin[1]as u32]) {data.extend(value.to_le_bytes());}
        }
        for a in &scene.assets {data.extend(a.id.to_le_bytes());for value in [a.width,a.height,a.format,a.data.len()as u32]{data.extend(value.to_le_bytes());}data.extend(&*a.data);}
        for row in &scene.tiles{for value in row{data.extend(value.to_le_bytes());}}
        for value in &scene.indices{data.extend(value.to_le_bytes());}
        std::fs::write(dir.join(format!("{number:02}.scene")),data).unwrap();
        std::fs::write(dir.join(format!("{number:02}.rgba")),&software).unwrap();
    }
    let mut counts=BTreeMap::<&str,usize>::new();
    let mut details=BTreeMap::<String,usize>::new();
    for op in frame.ops.iter(){
        let key=match op {
            RecordedOp::Fill{..}=>"Fill",
            RecordedOp::FillWith{blend,..}=>{*details.entry(format!("FillWith {blend:?}")).or_default()+=1;"FillWith"},
            RecordedOp::AlphaMask{mask,..}=>{*details.entry(format!("AlphaMask {}x{}",mask.width(),mask.height())).or_default()+=1;"AlphaMask"},
            RecordedOp::Glyphs(run)=>{*details.entry(format!("Glyphs font {} size {}",run.font,run.size)).or_default()+=run.glyphs.len();"Glyphs"},
            RecordedOp::FillPath{paint,draw,..}=>{*details.entry(format!("FillPath {paint:?} blend={:?} mask={} shadow={} xf={:?}",draw.blend,draw.mask.is_some(),draw.shadow.is_some(),draw.xf)).or_default()+=1;"FillPath"},
            RecordedOp::StrokePath{paint,draw,stroke,..}=>{*details.entry(format!("StrokePath {paint:?} blend={:?} mask={} shadow={} stroke={stroke:?}",draw.blend,draw.mask.is_some(),draw.shadow.is_some())).or_default()+=1;"StrokePath"},
            RecordedOp::Image{image,draw,quality,tiled,src,dst,alpha,tint}=>{*details.entry(format!("Image {}x{} blend={:?} mask={} shadow={} quality={quality:?} tiled={tiled} src={src:?} dst={dst:?} alpha={alpha} tint={tint:?} xf={:?}",image.width,image.height,draw.blend,draw.mask.is_some(),draw.shadow.is_some(),draw.xf)).or_default()+=1;"Image"},
            RecordedOp::BeginGroup{..}=>"BeginGroup",RecordedOp::EndGroup=>"EndGroup",RecordedOp::Composite(_)=>"Composite",
        };
        *counts.entry(key).or_default()+=1;
    }
    serde_json::json!({"width":frame.width,"height":frame.height,"scale":frame.scale,"counts":counts,"details":details,"exact_lowering":true,"commands":scene.commands.len(),"tile_indices":scene.indices.len(),"asset_bytes":scene.assets.iter().map(|a|a.data.len()).sum::<usize>()})
}
fn settled(h:&mut Harness<Hotaru>){assert!(h.settle(Duration::from_secs(5),|a|a.active_pane().is_some_and(|p|p.diagnostic_frame_current())));}
#[test]
fn complete_native_terminal_scene_inventory(){
    let mut output=Vec::new();let mut prepare=GpuPrepare::default();
    for theme in [ThemeChoice::Day,ThemeChoice::Night]{
        let appearance=Appearance{theme,..Appearance::default()};
        let mut h=Harness::with(Hotaru::preview(),appearance,true);
        let w=h.window();h.advance(Duration::ZERO);
        let id=h.program().active_pane().unwrap().id;
        let mut t=Terminal::new(120,40,100);
        t.set_default_grapheme_width(GraphemeWidthMethod::Unicode);
        t.feed("ASCII needle ffi == -> \x1b[31;44mcolor\x1b[0m\r\n日本語 界 é 👩‍💻 🇯🇵 ☀️\r\n┌─────────┬───────┐\r\n│ ▀▄█▓▒░ │ ═╬╔╗ │\r\n└─────────┴───────┘\r\n\x1b]8;;https://example.test/日本\x1b\\linked text\x1b]8;;\x1b\\\r\n\x1b[4:3;58:2::255:0:0mcurly underline\x1b[0m\r\n\x1b[7m inverse \x1b[0m \x1b[2mdim\x1b[0m \x1b[9mstrike\x1b[0m\x1b[2 q".as_bytes());
        h.send(Msg::Frame(id,Arc::new(t.snapshot())));settled(&mut h);
        for scale in [1.,1.25,2.]{
            h.output_scale(w,scale);h.advance(Duration::ZERO);assert!(h.settle(Duration::from_secs(5),|a|a.active_pane().is_some_and(|p|p.diagnostic_frame_current()&&p.font_metrics().is_some_and(|m|m.scale==scale))));
            let pane=h.program().active_pane().unwrap();let metrics=pane.font_metrics().unwrap();
            let origin=pane.rect.origin()+aizome_kit::Vec2::new(10.,10.);
            h.drag(w,origin+aizome_kit::Vec2::new(0.1,metrics.cell_height()*1.4),origin+aizome_kit::Vec2::new(metrics.cell_width()*8.,metrics.cell_height()*1.4),4);settled(&mut h);
            let mut record=inventory(&h.record_ops(w,scale),&mut prepare,output.len());record["case"]=serde_json::json!(format!("{appearance:?}-selected"));output.push(record);
            h.send(Msg::Find);h.send(Msg::Search("needle".into()));
            assert!(h.settle(Duration::from_secs(5),|a|a.search_selected().is_some()));settled(&mut h);
            let mut record=inventory(&h.record_ops(w,scale),&mut prepare,output.len());record["case"]=serde_json::json!(format!("{appearance:?}-find"));output.push(record);
            assert!(h.record_open_menu(w,"View"));h.advance(Duration::ZERO);
            let popups=h.popups(w);assert!(!popups.is_empty());
            for (popup,_) in popups{
                let mut record=inventory(&h.record_popup_ops(popup,scale),&mut prepare,output.len());record["case"]=serde_json::json!(format!("{appearance:?}-view-menu"));output.push(record);
            }
            h.key(w,aizome_kit::sidestep_ui::Key::Named(aizome_kit::sidestep_ui::NamedKey::Escape),aizome_kit::sidestep_ui::Modifiers::NONE);
            h.send(Msg::CloseFind);
        }
    }
    let path=std::env::var("HOTARU_SCENE_INVENTORY").expect("explicit isolated evidence path");
    std::fs::write(path,serde_json::to_vec_pretty(&output).unwrap()).unwrap();
}
