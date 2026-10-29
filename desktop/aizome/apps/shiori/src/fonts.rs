//! The fonts the worker gives hayro for those a document names but doesn't
//! carry. The 14 standard fonts, and stand-ins for other Latin fonts, are
//! hayro's own (Foxit's, built in), which it stretches to the widths the
//! document gives. Chinese, Japanese and Korean fonts have no stand-in
//! there: for those fontconfig, which the rest of the desktop asks, names
//! one for the language (Noto Sans CJK, in Aizome's session), and it's read
//! once.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::FontResolverFn;
use hayro::hayro_interpret::font::{FallbackFontQuery, FontData, FontQuery};
use hayro::hayro_interpret::hayro_cmap::CidFamily;

/// The resolver to hand hayro.
pub fn resolver() -> FontResolverFn {
    let found: Mutex<HashMap<String, Option<(FontData, u32)>>> = Mutex::new(HashMap::new());
    Arc::new(move |query| match query {
        FontQuery::Standard(s) => Some(s.get_font_data()),
        FontQuery::Fallback(f) => {
            if let Some(pattern) = cjk_pattern(f) {
                let mut found = found.lock().unwrap_or_else(|e| e.into_inner());
                let font = found.entry(pattern.clone()).or_insert_with(|| read(&pattern)).clone();
                if font.is_some() {
                    return font;
                }
            }
            Some(f.pick_standard_font().get_font_data())
        }
    })
}

/// The fontconfig pattern for a font of a Chinese, Japanese or Korean
/// collection; `None` for others.
fn cjk_pattern(f: &FallbackFontQuery) -> Option<String> {
    let lang = match f.character_collection.as_ref()?.family {
        CidFamily::AdobeJapan1 => "ja",
        CidFamily::AdobeGB1 => "zh-cn",
        CidFamily::AdobeCNS1 => "zh-tw",
        CidFamily::AdobeKorea1 => "ko",
        _ => return None,
    };
    let family = if f.is_serif { "serif" } else { "sans-serif" };
    let weight = if f.is_bold { ":weight=bold" } else { "" };
    Some(format!("{family}:lang={lang}{weight}"))
}

/// The font fontconfig matches `pattern` with, read.
fn read(pattern: &str) -> Option<(FontData, u32)> {
    let (path, index) = fontconfig_match(pattern)?;
    let data = std::fs::read(&path).ok()?;
    log::info!("{pattern} is {path} ({index})");
    Some((Arc::new(data), index))
}

/// The file and face fontconfig matches `pattern` with, through the
/// `libfontconfig` on the program's run path.
fn fontconfig_match(pattern: &str) -> Option<(String, u32)> {
    type Parse = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type Substitute = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> c_int;
    type Default = unsafe extern "C" fn(*mut c_void);
    type Match = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_int) -> *mut c_void;
    type GetString = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *const c_char) -> c_int;
    type GetInteger = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_int) -> c_int;
    type Destroy = unsafe extern "C" fn(*mut c_void);
    // FcMatchPattern and FcResultMatch.
    const MATCH_PATTERN: c_int = 0;
    const RESULT_MATCH: c_int = 0;
    let pattern = CString::new(pattern).ok()?;
    // SAFETY: fontconfig's documented C API, called as it says: a pattern
    // parsed, filled in and matched against the default configuration, the
    // match's file and index read (the string copied before the match is
    // freed), and both patterns freed.
    unsafe {
        let lib = libloading::Library::new("libfontconfig.so.1").ok()?;
        let parse: libloading::Symbol<Parse> = lib.get(b"FcNameParse\0").ok()?;
        let substitute: libloading::Symbol<Substitute> = lib.get(b"FcConfigSubstitute\0").ok()?;
        let default: libloading::Symbol<Default> = lib.get(b"FcDefaultSubstitute\0").ok()?;
        let matched: libloading::Symbol<Match> = lib.get(b"FcFontMatch\0").ok()?;
        let get_string: libloading::Symbol<GetString> = lib.get(b"FcPatternGetString\0").ok()?;
        let get_integer: libloading::Symbol<GetInteger> = lib.get(b"FcPatternGetInteger\0").ok()?;
        let destroy: libloading::Symbol<Destroy> = lib.get(b"FcPatternDestroy\0").ok()?;
        let pat = parse(pattern.as_ptr());
        if pat.is_null() {
            return None;
        }
        substitute(std::ptr::null_mut(), pat, MATCH_PATTERN);
        default(pat);
        let mut result = 0;
        let found = matched(std::ptr::null_mut(), pat, &mut result);
        destroy(pat);
        if found.is_null() {
            return None;
        }
        let mut file: *const c_char = std::ptr::null();
        let mut index: c_int = 0;
        let path = (result == RESULT_MATCH && get_string(found, c"file".as_ptr(), 0, &mut file) == RESULT_MATCH)
            .then(|| CStr::from_ptr(file).to_string_lossy().into_owned());
        if get_integer(found, c"index".as_ptr(), 0, &mut index) != RESULT_MATCH {
            index = 0;
        }
        destroy(found);
        Some((path?, u32::try_from(index).unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_collections_ask_for_their_language() {
        let mut q = FallbackFontQuery {
            post_script_name: None,
            font_name: None,
            font_family: None,
            font_stretch: hayro::hayro_interpret::font::FontStretch::Normal,
            font_weight: 400,
            is_fixed_pitch: false,
            is_serif: false,
            is_italic: false,
            is_bold: false,
            is_small_cap: false,
            character_collection: None,
        };
        assert_eq!(cjk_pattern(&q), None);
        q.character_collection = Some(hayro::hayro_interpret::hayro_cmap::CharacterCollection {
            family: CidFamily::AdobeJapan1,
            supplement: 6,
        });
        assert_eq!(cjk_pattern(&q).as_deref(), Some("sans-serif:lang=ja"));
        q.is_serif = true;
        q.is_bold = true;
        assert_eq!(cjk_pattern(&q).as_deref(), Some("serif:lang=ja:weight=bold"));
    }
}
