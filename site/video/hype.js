// The video's length comes from its soundtrack's (`Duration::Auto`): the
// page decodes it and sets its length here, in seconds, before `prepare`.
export const durations = new Map();
async function load_audio_wasm_callback(name) {
  return durations.get(name);
}
export const images = ["01-desktop-night.png", "03-palette.png", "07-day.png", "13-full-effects.png"];
export const audio = "score.wav";
export const defaultFont = "Archivo";


export class AudioData {
    static __wrap(ptr) {
        const obj = Object.create(AudioData.prototype);
        obj.__wbg_ptr = ptr;
        AudioDataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        AudioDataFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_audiodata_free(ptr, 0);
    }
    /**
     * An expensive cloning of the whole audio buffer, should be called once per
     * the app lifetime.
     * @returns {Float32Array}
     */
    get fltp_data() {
        const ret = wasm.audiodata_fltp_data(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {string}
     */
    get name() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.audiodata_name(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    get sample_rate() {
        const ret = wasm.__wbg_get_audiodata_sample_rate(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} arg0
     */
    set sample_rate(arg0) {
        wasm.__wbg_set_audiodata_sample_rate(this.__wbg_ptr, arg0);
    }
}
if (Symbol.dispose) AudioData.prototype[Symbol.dispose] = AudioData.prototype.free;

export class GeneralVideoFileMetadata {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        GeneralVideoFileMetadataFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_generalvideofilemetadata_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    get duration() {
        const ret = wasm.__wbg_get_generalvideofilemetadata_duration(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get fps() {
        const ret = wasm.__wbg_get_generalvideofilemetadata_fps(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get height() {
        const ret = wasm.__wbg_get_generalvideofilemetadata_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    get width() {
        const ret = wasm.__wbg_get_generalvideofilemetadata_width(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} arg0
     */
    set duration(arg0) {
        wasm.__wbg_set_generalvideofilemetadata_duration(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set fps(arg0) {
        wasm.__wbg_set_generalvideofilemetadata_fps(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set height(arg0) {
        wasm.__wbg_set_generalvideofilemetadata_height(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set width(arg0) {
        wasm.__wbg_set_generalvideofilemetadata_width(this.__wbg_ptr, arg0);
    }
}
if (Symbol.dispose) GeneralVideoFileMetadata.prototype[Symbol.dispose] = GeneralVideoFileMetadata.prototype.free;

export class StaticFontFace {
    static __wrap(ptr) {
        const obj = Object.create(StaticFontFace.prototype);
        obj.__wbg_ptr = ptr;
        StaticFontFaceFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        StaticFontFaceFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_staticfontface_free(ptr, 0);
    }
    /**
     * @returns {Uint8Array}
     */
    get data() {
        const ret = wasm.staticfontface_data(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {any}
     */
    get info() {
        const ret = wasm.staticfontface_info(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {string}
     */
    get name() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.staticfontface_name(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
}
if (Symbol.dispose) StaticFontFace.prototype[Symbol.dispose] = StaticFontFace.prototype.free;

export class VideoMetadata {
    static __wrap(ptr) {
        const obj = Object.create(VideoMetadata.prototype);
        obj.__wbg_ptr = ptr;
        VideoMetadataFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        VideoMetadataFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_videometadata_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    get durationInFrames() {
        const ret = wasm.__wbg_get_videometadata_durationInFrames(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get fps() {
        const ret = wasm.__wbg_get_videometadata_fps(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    get height() {
        const ret = wasm.__wbg_get_videometadata_height(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number | undefined}
     */
    get originalFps() {
        const ret = wasm.__wbg_get_videometadata_originalFps(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * @returns {number}
     */
    get width() {
        const ret = wasm.__wbg_get_videometadata_width(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} arg0
     */
    set durationInFrames(arg0) {
        wasm.__wbg_set_videometadata_durationInFrames(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set fps(arg0) {
        wasm.__wbg_set_videometadata_fps(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number} arg0
     */
    set height(arg0) {
        wasm.__wbg_set_videometadata_height(this.__wbg_ptr, arg0);
    }
    /**
     * @param {number | null} [arg0]
     */
    set originalFps(arg0) {
        wasm.__wbg_set_videometadata_originalFps(this.__wbg_ptr, !isLikeNone(arg0), isLikeNone(arg0) ? 0 : arg0);
    }
    /**
     * @param {number} arg0
     */
    set width(arg0) {
        wasm.__wbg_set_videometadata_width(this.__wbg_ptr, arg0);
    }
    /**
     * Returns a resolved audio_map with audio timestamps converted to frames.
     * The audio timestamp fallbacks to 0 if audio not loaded yet
     * @returns {Record<string, [number, number]>}
     * @returns {any}
     */
    get audioMap() {
        const ret = wasm.videometadata_audioMap(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    get hasAudio() {
        const ret = wasm.videometadata_hasAudio(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {string}
     */
    get name() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.videometadata_name(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Returns a resolved audio_map with audio timestamps converted to frames.
     * The audio timestamp fallbacks to 0 if audio not loaded yet
     * @returns {Record<string, [number, number]>}
     * @returns {any}
     */
    get scenesTimeline() {
        const ret = wasm.videometadata_scenesTimeline(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) VideoMetadata.prototype[Symbol.dispose] = VideoMetadata.prototype.free;

export class WasmBridge {
    static __wrap(ptr) {
        const obj = Object.create(WasmBridge.prototype);
        obj.__wbg_ptr = ptr;
        WasmBridgeFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        WasmBridgeFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_wasmbridge_free(ptr, 0);
    }
    /**
     * @param {string} file
     * @param {number} sample_rate
     * @param {Float32Array} input
     */
    add_audio_source(file, sample_rate, input) {
        const ptr0 = passStringToWasm0(file, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(input, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        wasm.wasmbridge_add_audio_source(this.__wbg_ptr, ptr0, len0, sample_rate, ptr1, len1);
    }
    /**
     * @param {string} file
     * @param {string} url
     * @param {number} width
     * @param {number} height
     * @param {string | null} [base64_data]
     */
    add_image_source(file, url, width, height, base64_data) {
        const ptr0 = passStringToWasm0(file, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        var ptr2 = isLikeNone(base64_data) ? 0 : passStringToWasm0(base64_data, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len2 = WASM_VECTOR_LEN;
        wasm.wasmbridge_add_image_source(this.__wbg_ptr, ptr0, len0, ptr1, len1, width, height, ptr2, len2);
    }
    /**
     * @param {string} file
     * @param {string} content
     * @returns {number | undefined}
     */
    add_lottie_source(file, content) {
        const ptr0 = passStringToWasm0(file, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.wasmbridge_add_lottie_source(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret === Number.MAX_SAFE_INTEGER ? undefined : ret;
    }
    /**
     * @param {string} file
     * @param {string} content
     * @returns {number}
     */
    add_subtitles_source(file, content) {
        const ptr0 = passStringToWasm0(file, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(content, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.wasmbridge_add_subtitles_source(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret >>> 0;
    }
    /**
     * @param {string} file
     * @param {string} url
     * @param {number} width
     * @param {number} height
     * @param {number} duration
     */
    add_video_source_placeholder(file, url, width, height, duration) {
        const ptr0 = passStringToWasm0(file, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        wasm.wasmbridge_add_video_source_placeholder(this.__wbg_ptr, ptr0, len0, ptr1, len1, width, height, duration);
    }
    /**
     * @param {number} index
     * @returns {AudioData | undefined}
     */
    get_static_audio_data_by_index(index) {
        const ret = wasm.wasmbridge_get_static_audio_data_by_index(this.__wbg_ptr, index);
        return ret === 0 ? undefined : AudioData.__wrap(ret);
    }
    /**
     * @param {number} index
     * @returns {StaticFontFace | undefined}
     */
    get_static_font_data_by_index(index) {
        const ret = wasm.wasmbridge_get_static_font_data_by_index(this.__wbg_ptr, index);
        return ret === 0 ? undefined : StaticFontFace.__wrap(ret);
    }
    /**
     * @param {Uint8Array} slice
     * @returns {any}
     */
    ingest_font(slice) {
        const ptr0 = passArray8ToWasm0(slice, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.wasmbridge_ingest_font(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    populate_static_fonts_db_with_static_fonts() {
        wasm.wasmbridge_populate_static_fonts_db_with_static_fonts(this.__wbg_ptr);
    }
    /**
     * @param {number | null} [fps]
     * @returns {Promise<VideoMetadata>}
     */
    prepare(fps) {
        const ret = wasm.wasmbridge_prepare(this.__wbg_ptr, isLikeNone(fps) ? Number.MAX_SAFE_INTEGER : (fps) >> 0);
        return ret;
    }
    /**
     * @param {bigint} frame
     * @returns {string}
     */
    render_frame(frame) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmbridge_render_frame(this.__wbg_ptr, frame);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {bigint} frame
     * @returns {string}
     */
    render_preview_frame(frame) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.wasmbridge_render_preview_frame(this.__wbg_ptr, frame);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
}
if (Symbol.dispose) WasmBridge.prototype[Symbol.dispose] = WasmBridge.prototype.free;

/**
 * @returns {WasmBridge}
 */
export function create_wasm_bridge() {
    const ret = wasm.create_wasm_bridge();
    return WasmBridge.__wrap(ret);
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_30c8987f7c2ed4e2: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___fframes_get_video_frame_d133657f45ceb4c3: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = __fframes_get_video_frame(getStringFromWasm0(arg0, arg1), arg2, arg3 !== 0);
            return ret;
        }, arguments); },
        __wbg___wbindgen_debug_string_4687d8d8c2017d52: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_1f9d30630b8b1d3d: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_e343b7d08827ba72: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_2e0e7dee9f701a71: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_dcc1a90847f04c41: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_call_187d372bd5fdd4aa: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_error_757e9472f8410341: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_get_31af05bd4842a84f: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_load_audio_wasm_callback_521fa72899dd1f93: function() { return handleError(function (arg0, arg1) {
            const ret = load_audio_wasm_callback(getStringFromWasm0(arg0, arg1));
            return ret;
        }, arguments); },
        __wbg_log_cebe8fd7bdd4483d: function(arg0, arg1) {
            console.log(getStringFromWasm0(arg0, arg1));
        },
        __wbg_new_227d7c05414eb861: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_617a8cdb8bb1130e: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_ee2291f50781bf1d: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_from_slice_9a868026ffa4208a: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_typed_b01cb72a8af741a3: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined_______true_(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = 0;
            }
        },
        __wbg_queueMicrotask_9833f9a49df95a49: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queueMicrotask_a72f977e97f23c5f: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_resolve_0076e10020304ede: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_set_6be42768c690e380: function(arg0, arg1, arg2) {
            arg0[arg1] = arg2;
        },
        __wbg_set_bea140a88be9b277: function(arg0, arg1, arg2) {
            arg0[arg1 >>> 0] = arg2;
        },
        __wbg_stack_3b0d974bbf31e44f: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_266715b9d96ba635: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_10fb7dc1ae063179: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_0b583911f537483a: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_d7f903d1508cbdc4: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_then_c949d5a25a4e78f8: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_e71170d78fcf8954: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbg_videometadata_new: function(arg0) {
            const ret = VideoMetadata.__wrap(arg0);
            return ret;
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 247, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___wasm_bindgen_969a18ef070f1670___JsValue__core_37ee0ce0c4cd57d5___result__Result_____wasm_bindgen_969a18ef070f1670___JsError___true_);
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_generic_0000000000000004: function(arg0) {
            // Cast intrinsic for `U64 -> Externref`.
            const ret = BigInt.asUintN(64, arg0);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./hype_bg.js": import0,
    };
}

function wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___wasm_bindgen_969a18ef070f1670___JsValue__core_37ee0ce0c4cd57d5___result__Result_____wasm_bindgen_969a18ef070f1670___JsError___true_(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___wasm_bindgen_969a18ef070f1670___JsValue__core_37ee0ce0c4cd57d5___result__Result_____wasm_bindgen_969a18ef070f1670___JsError___true_(arg0, arg1, arg2);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

function wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined_______true_(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen_969a18ef070f1670___convert__closures_____invoke___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined___js_sys_4f5d0b392909503___Function_fn_wasm_bindgen_969a18ef070f1670___JsValue_____wasm_bindgen_969a18ef070f1670___sys__Undefined_______true_(arg0, arg1, arg2, arg3);
}

const AudioDataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_audiodata_free(ptr, 1));
const GeneralVideoFileMetadataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_generalvideofilemetadata_free(ptr, 1));
const StaticFontFaceFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_staticfontface_free(ptr, 1));
const VideoMetadataFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_videometadata_free(ptr, 1));
const WasmBridgeFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_wasmbridge_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_destroy_closure(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('hype_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
// The characters each family falls back for (web/fallbacks.mjs).
export const fallbacks = [{"family":"IBM Plex Mono","from":13,"unicodeRange":"U+2756"},{"family":"Archivo","from":11,"unicodeRange":"U+276F"},{"family":"Noto Sans Symbols 2","from":0,"unicodeRange":"U+2191"},{"family":"Noto Sans Symbols 2","from":11,"unicodeRange":"U+2303"}];
