//! Diagnostic GLES compute compositor. All driver calls/objects stay on the
//! existing dedicated context worker; no event-side synchronous driver call.
use super::*;
use crate::scene::{Asset, Scene};
use std::{collections::HashMap, sync::Arc};
const SSBO: u32 = 0x90d2;
const ATLAS: usize = 32 << 20;
pub struct SceneDriver {
    context: Rc<Context>,
    program: u32,
    buffers: [u32; 4],
    extent: i32,
    used: usize,
    cache: HashMap<u64, (usize, Arc<[u8]>, u32, u32, u32)>,
}
impl Drop for SceneDriver {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: fixed array contains only this worker's live buffer names;
            // current context and function pointers outlive deletion via Rc.
            (self.context.api.glDeleteBuffers)(4, self.buffers.as_ptr());
            if self.program != 0 {
                (self.context.api.glDeleteProgram)(self.program);
            }
            println!(
                "{}",
                serde_json::json!({"event":"scene_gpu_cache_destroyed","ok":(self.context.api.glGetError)()==0})
            );
        }
    }
}
impl SceneDriver {
    pub fn new(
        library: &str,
        node: &str,
        shutdown: Option<std::sync::mpsc::Sender<bool>>,
    ) -> Result<Self, String> {
        let driver = EglDriver::new(library, node, shutdown)?;
        let c = driver.context.clone();
        let mut out = Self {
            context: c.clone(),
            program: 0,
            buffers: [0; 4],
            extent: -1,
            used: 0,
            cache: HashMap::new(),
        };
        unsafe {
            // SAFETY: each scalar/array output is correctly sized for this exact
            // Khronos ABI. Capabilities are checked before buffer allocation.
            let (mut major, mut minor, mut max_block, mut units, mut invocations) =
                (0, 0, 0i64, 0, 0);
            (c.api.glGetIntegerv)(0x821b, &mut major);
            (c.api.glGetIntegerv)(0x821c, &mut minor);
            require(major > 3 || (major == 3 && minor >= 1), "GLES3.1")?;
            (c.api.glGetInteger64v)(0x90de, &mut max_block);
            (c.api.glGetIntegerv)(0x90dd, &mut units);
            (c.api.glGetIntegerv)(0x90eb, &mut invocations);
            require(
                max_block >= ATLAS as i64 && units >= 4 && invocations >= 128,
                "compute SSBO limits",
            )?;
            for (dimension, wanted) in [(0, 16), (1, 8)] {
                let mut v = 0;
                (c.api.glGetIntegeri_v)(0x91bf, dimension, &mut v);
                require(v >= wanted, "workgroup dimensions")?;
            }
            for (dimension, wanted) in [(0, 512), (1, 1024)] {
                let mut v = 0;
                (c.api.glGetIntegeri_v)(0x91be, dimension, &mut v);
                require(v >= wanted, "dispatch dimensions")?;
            }
            let shader = (c.api.glCreateShader)(0x91b9);
            require(shader != 0, "compute shader allocation")?;
            let source = include_str!("compose.comp");
            let ptr = source.as_ptr().cast();
            let len = source.len() as i32;
            (c.api.glShaderSource)(shader, 1, &ptr, &len);
            (c.api.glCompileShader)(shader);
            let mut ok = 0;
            (c.api.glGetShaderiv)(shader, 0x8b81, &mut ok);
            if ok == 0 {
                let mut log = [0u8; 4096];
                let mut written = 0;
                (c.api.glGetShaderInfoLog)(shader, 4096, &mut written, log.as_mut_ptr().cast());
                (c.api.glDeleteShader)(shader);
                return Err(format!(
                    "compute compile: {}",
                    String::from_utf8_lossy(&log[..(written as usize).min(log.len())])
                ));
            }
            out.program = (c.api.glCreateProgram)();
            if out.program == 0 {
                (c.api.glDeleteShader)(shader);
                return Err("program allocation".into());
            }
            (c.api.glAttachShader)(out.program, shader);
            (c.api.glLinkProgram)(out.program);
            (c.api.glDeleteShader)(shader);
            (c.api.glGetProgramiv)(out.program, 0x8b82, &mut ok);
            if ok == 0 {
                let mut log = [0u8; 4096];
                let mut written = 0;
                (c.api.glGetProgramInfoLog)(
                    out.program,
                    4096,
                    &mut written,
                    log.as_mut_ptr().cast(),
                );
                return Err(format!(
                    "compute link: {}",
                    String::from_utf8_lossy(&log[..(written as usize).min(log.len())])
                ));
            }
            out.extent = (c.api.glGetUniformLocation)(out.program, c"extent".as_ptr());
            require(out.extent >= 0, "extent uniform")?;
            (c.api.glGenBuffers)(4, out.buffers.as_mut_ptr());
            (c.api.glBindBuffer)(SSBO, out.buffers[3]);
            (c.api.glBufferData)(SSBO, ATLAS as isize, ptr::null(), 0x88e8);
            (c.api.glDisable)(0x0bd0);
            require((c.api.glGetError)() == 0, "compute setup")?;
            println!(
                "{}",
                serde_json::json!({"event":"scene_compute_ready","max_ssbo":max_block,"atlas_bytes":ATLAS,"local_size":[16,8],"integer_blend":true,"filtered_samples_precise":true})
            );
        }
        Ok(out)
    }
    fn asset(&mut self, a: &Asset) -> Result<(u32, usize), String> {
        if let Some((offset, bytes, w, h, format)) = self.cache.get(&a.id) {
            require(
                *w == a.width && *h == a.height && *format == a.format && &**bytes == &*a.data,
                "asset identity collision",
            )?;
            return Ok((*offset as u32, 0));
        }
        let len = a.data.len().checked_add(3).ok_or("asset padding")? & !3;
        require(
            self.used.checked_add(len).is_some_and(|n| n <= ATLAS),
            "atlas admission",
        )?;
        let mut words = Vec::with_capacity(len / 4);
        for bytes in a.data.chunks(4) {
            let mut word = [0u8; 4];
            word[..bytes.len()].copy_from_slice(bytes);
            words.push(u32::from_le_bytes(word));
        }
        unsafe {
            // SAFETY: words owns exactly len initialized bytes; upload copies
            // during the call, range was checked within allocated atlas storage.
            (self.context.api.glBindBuffer)(SSBO, self.buffers[3]);
            (self.context.api.glBufferSubData)(
                SSBO,
                self.used as isize,
                len as isize,
                words.as_ptr().cast(),
            );
        }
        let offset = self.used;
        self.used += len;
        self.cache
            .insert(a.id, (offset, a.data.clone(), a.width, a.height, a.format));
        Ok((offset as u32, len))
    }
    fn commands(&mut self, scene: &Scene) -> Result<(Vec<u32>, usize), String> {
        let required = scene
            .assets()
            .iter()
            .filter(|a| !self.cache.contains_key(&a.id))
            .try_fold(0usize, |n, a| n.checked_add((a.data.len() + 3) & !3))
            .ok_or("asset total")?;
        if self.used.checked_add(required).is_none_or(|n| n > ATLAS) {
            // Every preceding render waited for its producer fence on this worker.
            // Committed exported textures no longer depend on atlas contents.
            self.cache.clear();
            self.used = 0;
        }
        let mut offsets = Vec::with_capacity(scene.assets().len());
        let mut uploaded = 0;
        for a in scene.assets() {
            let (offset, n) = self.asset(a)?;
            offsets.push(offset);
            uploaded += n;
        }
        let mut out = Vec::with_capacity(scene.commands().len() * 12);
        for c in scene.commands() {
            out.extend_from_slice(&c[..7]);
            out.push(if c[7] == u32::MAX {
                0
            } else {
                offsets[c[7] as usize]
            });
            out.push(if c[7] == u32::MAX {
                0
            } else {
                scene.assets()[c[7] as usize].width
            });
            out.extend_from_slice(&c[8..10]);
            out.push(0);
        }
        Ok((out, uploaded))
    }
}
impl Driver<Scene> for SceneDriver {
    type Resource = EglResource;
    fn render(
        &mut self,
        frame: &Frame<Scene>,
        cancelled: &Cancellation,
    ) -> Result<(EglResource, Descriptor), RenderError> {
        let scene = &frame.source;
        let started = Instant::now();
        let (commands, uploaded) = self.commands(scene)?;
        let tiles: Vec<u32> = scene.tiles().iter().flatten().copied().collect();
        let c = self.context.clone();
        let mut r = EglResource {
            context: c.clone(),
            texture: 0,
            framebuffer: 0,
            image: ptr::null_mut(),
            original: None,
        };
        unsafe {
            // SAFETY: complete scene validates dimensions, all asset ranges,
            // metadata lengths and tile coverage before admission. Every pointer
            // refers to a live exact-length array during this synchronous upload.
            for (i, words) in [&commands[..], &tiles[..], &scene.indices()[..]]
                .into_iter()
                .enumerate()
            {
                require(words.len() * 4 <= ATLAS, "metadata SSBO size")?;
                (c.api.glBindBuffer)(SSBO, self.buffers[i]);
                if words.is_empty() {
                    (c.api.glBufferData)(SSBO, 4, [0u32].as_ptr().cast(), 0x88e8);
                } else {
                    (c.api.glBufferData)(
                        SSBO,
                        (words.len() * 4) as isize,
                        words.as_ptr().cast(),
                        0x88e8,
                    );
                }
            }
            for (i, buffer) in self.buffers.iter().enumerate() {
                (c.api.glBindBufferBase)(SSBO, i as u32, *buffer);
            }
            (c.api.glGenTextures)(1, &mut r.texture);
            (c.api.glBindTexture)(TEX, r.texture);
            (c.api.glTexParameteri)(TEX, 0x2801, 0x2600);
            (c.api.glTexParameteri)(TEX, 0x2800, 0x2600);
            (c.api.glTexStorage2D)(TEX, 1, 0x8058, scene.width() as i32, scene.height() as i32);
            (c.api.glGenFramebuffers)(1, &mut r.framebuffer);
            (c.api.glBindFramebuffer)(FB, r.framebuffer);
            (c.api.glFramebufferTexture2D)(FB, 0x8ce0, TEX, r.texture, 0);
            require(
                (c.api.glCheckFramebufferStatus)(FB) == 0x8cd5,
                "scene framebuffer",
            )?;
            (c.api.glUseProgram)(self.program);
            (c.api.glUniform2ui)(self.extent, scene.width(), scene.height());
            (c.api.glBindImageTexture)(0, r.texture, 0, 0, 0, 0x88b9, 0x8058);
            if cancelled.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            let submitted = Instant::now();
            (c.api.glDispatchCompute)(scene.width().div_ceil(16), scene.height().div_ceil(8), 1);
            // GLES3.1: image writes precede subsequent image/texture/framebuffer
            // access. EGL fence then orders producer completion before handoff.
            (c.api.glMemoryBarrier)(0x20 | 0x8 | 0x400);
            require((c.api.glGetError)() == 0, "scene dispatch/barrier")?;
            r.image = (c.api.eglCreateImageKHR)(
                c.display,
                c.context,
                0x30b1,
                r.texture as usize as Handle,
                [0x30bc, 0, 0x30d2, 1, NONE].as_ptr(),
            );
            require(!r.image.is_null(), "scene EGLimage")?;
            let (mut fourcc, mut planes) = (0, 0);
            let mut modifiers = [0u64; 4];
            require(
                (c.api.eglExportDMABUFImageQueryMESA)(
                    c.display,
                    r.image,
                    &mut fourcc,
                    &mut planes,
                    modifiers.as_mut_ptr(),
                ) != 0,
                "scene export query",
            )?;
            require(
                planes == 1 && fourcc as u32 == u32::from_le_bytes(*b"AB24") && modifiers[0] == 0,
                "scene export AB24 linear",
            )?;
            let (mut fd, mut stride, mut offset) = (-1, 0, 0);
            let exported = (c.api.eglExportDMABUFImageMESA)(
                c.display,
                r.image,
                &mut fd,
                &mut stride,
                &mut offset,
            );
            if fd >= 0 {
                r.original = Some(OwnedFd::from_raw_fd(fd));
            }
            require(
                exported != 0 && r.original.is_some() && stride > 0 && offset >= 0,
                "scene descriptor",
            )?;
            let sync = (c.api.eglCreateSyncKHR)(c.display, 0x30f9, ptr::null());
            require(!sync.is_null(), "scene fence")?;
            let fence = Fence {
                context: c.clone(),
                sync,
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if cancelled.is_cancelled() {
                    return Err(RenderError::Cancelled);
                }
                require(Instant::now() < deadline, "scene fence deadline")?;
                let status = (c.api.eglClientWaitSyncKHR)(c.display, sync, 1, 5_000_000);
                if status == 0x30f6 {
                    break;
                }
                require(status == 0x30f5, "scene fence wait")?;
            }
            drop(fence);
            let finished = Instant::now();
            let mut pixels = vec![0u8; scene.width() as usize * scene.height() as usize * 4];
            (c.api.glReadPixels)(
                0,
                0,
                scene.width() as i32,
                scene.height() as i32,
                0x1908,
                0x1401,
                pixels.as_mut_ptr().cast(),
            );
            require((c.api.glGetError)() == 0, "scene readback")?;
            require(
                pixels.len() == scene.expected().len(),
                "exact reference length",
            )?;
            let differences = pixels
                .iter()
                .zip(scene.expected())
                .filter(|(a, b)| a != b)
                .count();
            println!(
                "{}",
                serde_json::json!({"event":"scene_verified","scene":scene.name(),"width":scene.width(),"height":scene.height(),"commands":scene.commands().len(),"tile_indices":scene.indices().len(),"asset_upload_bytes":uploaded,"atlas_used":self.used,"rgba_bytes":pixels.len(),"different_bytes":differences,"readback_after_fence":true,"prepare_upload_us":submitted.duration_since(started).as_micros(),"dispatch_export_fence_us":finished.duration_since(submitted).as_micros(),"readback_verify_us":Instant::now().duration_since(finished).as_micros()})
            );
            if differences != 0 {
                let first = pixels
                    .iter()
                    .zip(scene.expected())
                    .position(|(a, b)| a != b)
                    .unwrap();
                return Err(format!(
                    "scene {} differs at byte {first}: GPU{} CPU{}",
                    scene.name(),
                    pixels[first],
                    scene.expected()[first]
                )
                .into());
            }
            let desc = Descriptor {
                fd: r
                    .original
                    .as_ref()
                    .unwrap()
                    .try_clone()
                    .map_err(|e| e.to_string())?,
                width: scene.width(),
                height: scene.height(),
                stride: stride as u32,
                offset: offset as u32,
                fourcc: fourcc as u32,
                modifier: modifiers[0],
            };
            Ok((r, desc))
        }
    }
}
