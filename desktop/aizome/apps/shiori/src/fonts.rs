//! The fonts the worker gives hayro for those a document names but doesn't
//! carry. The 14 standard fonts, and stand-ins for other Latin fonts, are
//! hayro's own (Foxit's, built in), which it stretches to the widths the
//! document gives. Chinese, Japanese and Korean fonts have no stand-in
//! there: for those fontconfig, which the rest of the desktop asks, lists
//! the fonts it would use for the language, and the first that has its
//! characters (Noto Sans CJK, in Aizome's session) is read, once. hayro
//! finds a character's glyph through its Unicode; a font without the
//! character would give the wrong glyph, so one is only taken if it has
//! them.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::FontResolverFn;
use hayro::hayro_interpret::font::{FallbackFontQuery, FontData, FontQuery};
use hayro::hayro_interpret::hayro_cmap::CidFamily;

/// How far down fontconfig's list for a language to look.
const MOST_CANDIDATES: usize = 64;

/// The resolver to hand hayro.
pub fn resolver() -> FontResolverFn {
    let found: Mutex<HashMap<String, Option<(FontData, u32)>>> = Mutex::new(HashMap::new());
    Arc::new(move |query| match query {
        FontQuery::Standard(s) => Some(s.get_font_data()),
        FontQuery::Fallback(f) => {
            if let Some((pattern, probe)) = cjk_pattern(f) {
                let mut found = found.lock().unwrap_or_else(|e| e.into_inner());
                let font = found.entry(pattern.clone()).or_insert_with(|| read(&pattern, probe)).clone();
                if font.is_some() {
                    return font;
                }
            }
            Some(f.pick_standard_font().get_font_data())
        }
    })
}

/// The fontconfig pattern for a font of a Chinese, Japanese or Korean
/// collection, and a character such a font has; `None` for others.
fn cjk_pattern(f: &FallbackFontQuery) -> Option<(String, char)> {
    let (lang, probe) = match f.character_collection.as_ref()?.family {
        CidFamily::AdobeJapan1 => ("ja", 'あ'),
        CidFamily::AdobeGB1 => ("zh-cn", '中'),
        CidFamily::AdobeCNS1 => ("zh-tw", '中'),
        CidFamily::AdobeKorea1 => ("ko", '한'),
        _ => return None,
    };
    let family = if f.is_serif { "serif" } else { "sans-serif" };
    let weight = if f.is_bold { ":weight=bold" } else { "" };
    Some((format!("{family}:lang={lang}{weight}"), probe))
}

/// The first font fontconfig lists for `pattern` that has `probe`, read.
fn read(pattern: &str, probe: char) -> Option<(FontData, u32)> {
    let (path, index) = fontconfig_find(pattern, probe)?;
    let data = std::fs::read(&path).ok()?;
    Some((Arc::new(data), index))
}

/// fontconfig's list of fonts (each a pattern).
#[repr(C)]
struct FontSet {
    nfont: c_int,
    sfont: c_int,
    fonts: *mut *mut c_void,
}

/// The file and face of the first outline font fontconfig lists for
/// `pattern` that has `probe`, through the `libfontconfig` on the
/// program's run path.
fn fontconfig_find(pattern: &str, probe: char) -> Option<(String, u32)> {
    type Parse = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type Substitute = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> c_int;
    type DefaultSubstitute = unsafe extern "C" fn(*mut c_void);
    type Sort = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut *mut c_void, *mut c_int) -> *mut FontSet;
    type GetString = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *const c_char) -> c_int;
    type GetInteger = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_int) -> c_int;
    type GetCharSet = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void) -> c_int;
    type HasChar = unsafe extern "C" fn(*mut c_void, u32) -> c_int;
    type Destroy = unsafe extern "C" fn(*mut c_void);
    type SetDestroy = unsafe extern "C" fn(*mut FontSet);
    // FcMatchPattern and FcResultMatch.
    const MATCH_PATTERN: c_int = 0;
    const RESULT_MATCH: c_int = 0;
    let pattern = CString::new(pattern).ok()?;
    // SAFETY: fontconfig's documented C API, called as it says: a pattern
    // parsed, filled in and sorted against the default configuration; each
    // listed font's character set, file and index read (borrowed from the
    // set, the file copied before the set is freed); the pattern and the
    // set freed. `FcFontSet` is laid out as fontconfig.h declares it.
    unsafe {
        let lib = libloading::Library::new("libfontconfig.so.1").ok()?;
        let parse: libloading::Symbol<Parse> = lib.get(b"FcNameParse\0").ok()?;
        let substitute: libloading::Symbol<Substitute> = lib.get(b"FcConfigSubstitute\0").ok()?;
        let default: libloading::Symbol<DefaultSubstitute> = lib.get(b"FcDefaultSubstitute\0").ok()?;
        let sort: libloading::Symbol<Sort> = lib.get(b"FcFontSort\0").ok()?;
        let get_string: libloading::Symbol<GetString> = lib.get(b"FcPatternGetString\0").ok()?;
        let get_integer: libloading::Symbol<GetInteger> = lib.get(b"FcPatternGetInteger\0").ok()?;
        let get_charset: libloading::Symbol<GetCharSet> = lib.get(b"FcPatternGetCharSet\0").ok()?;
        let has_char: libloading::Symbol<HasChar> = lib.get(b"FcCharSetHasChar\0").ok()?;
        let destroy: libloading::Symbol<Destroy> = lib.get(b"FcPatternDestroy\0").ok()?;
        let set_destroy: libloading::Symbol<SetDestroy> = lib.get(b"FcFontSetDestroy\0").ok()?;
        let pat = parse(pattern.as_ptr());
        if pat.is_null() {
            return None;
        }
        substitute(std::ptr::null_mut(), pat, MATCH_PATTERN);
        default(pat);
        let mut result = 0;
        let set = sort(std::ptr::null_mut(), pat, 0, std::ptr::null_mut(), &mut result);
        destroy(pat);
        if set.is_null() {
            return None;
        }
        let mut found = None;
        let n = usize::try_from((*set).nfont).unwrap_or(0).min(MOST_CANDIDATES);
        for i in 0..n {
            let font = *(*set).fonts.add(i);
            let mut charset: *mut c_void = std::ptr::null_mut();
            let mut file: *const c_char = std::ptr::null();
            if get_charset(font, c"charset".as_ptr(), 0, &mut charset) != RESULT_MATCH
                || has_char(charset, u32::from(probe)) == 0
                || get_string(font, c"file".as_ptr(), 0, &mut file) != RESULT_MATCH
            {
                continue;
            }
            let path = CStr::from_ptr(file).to_string_lossy().into_owned();
            // hayro reads TrueType and OpenType fonts, not bitmap ones.
            let outline = [".ttf", ".otf", ".ttc", ".otc"].iter().any(|e| path.to_ascii_lowercase().ends_with(e));
            if !outline {
                continue;
            }
            let mut index: c_int = 0;
            if get_integer(font, c"index".as_ptr(), 0, &mut index) != RESULT_MATCH {
                index = 0;
            }
            // The face in a collection; a variable font's named instance is
            // in the upper 16 bits, and hayro draws its default instance.
            found = Some((path, u32::try_from(index & 0xffff).unwrap_or(0)));
            break;
        }
        set_destroy(set);
        found
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
        assert_eq!(cjk_pattern(&q), Some(("sans-serif:lang=ja".into(), 'あ')));
        q.is_serif = true;
        q.is_bold = true;
        assert_eq!(cjk_pattern(&q), Some(("serif:lang=ja:weight=bold".into(), 'あ')));
    }
}
