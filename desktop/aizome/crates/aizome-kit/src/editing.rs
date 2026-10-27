//! Moving through text as editors do: by character (grapheme), by word,
//! by line; shared by `text_field` and `text_editor`.

use unicode_segmentation::UnicodeSegmentation;

/// The start of the character before byte `at`.
pub fn prev_char(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    s[..at].grapheme_indices(true).next_back().map_or(0, |(i, _)| i)
}

/// The end of the character after byte `at`.
pub fn next_char(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    s[at..].graphemes(true).next().map_or(s.len(), |g| at + g.len())
}

fn is_word(w: &str) -> bool {
    w.chars().any(|c| c.is_alphanumeric() || c == '_')
}

/// The start of the word before `at` (skipping what isn't a word).
pub fn prev_word(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    let mut last = 0;
    let mut seen_word = false;
    let words: Vec<(usize, &str)> = s[..at].split_word_bound_indices().collect();
    for &(i, w) in words.iter().rev() {
        if is_word(w) {
            last = i;
            seen_word = true;
        } else if seen_word {
            return last;
        }
    }
    if seen_word { last } else { 0 }
}

/// The end of the word after `at` (skipping what isn't a word).
pub fn next_word(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    let mut seen_word = false;
    let mut end = s.len();
    for (i, w) in s[at..].split_word_bound_indices() {
        if is_word(w) {
            seen_word = true;
            end = at + i + w.len();
        } else if seen_word {
            return end;
        }
    }
    end
}

/// The word around byte `at` (for a double click): its range.
pub fn word_at(s: &str, at: usize) -> std::ops::Range<usize> {
    let at = at.min(s.len());
    for (i, w) in s.split_word_bound_indices() {
        if at >= i && at < i + w.len() || (at == s.len() && i + w.len() == at) {
            return i..i + w.len();
        }
    }
    at..at
}

/// The nearest character boundary at or before `at`.
pub fn floor(s: &str, at: usize) -> usize {
    let mut at = at.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_are_graphemes() {
        let s = "ae\u{301}b👍🏽c";
        assert_eq!(next_char(s, 1), 4);
        assert_eq!(prev_char(s, 4), 1);
        let thumbs = s.find('👍').unwrap();
        assert_eq!(next_char(s, thumbs), thumbs + "👍🏽".len());
        assert_eq!(prev_char(s, 0), 0);
        assert_eq!(next_char(s, s.len()), s.len());
    }

    #[test]
    fn words_skip_spaces_and_punctuation() {
        let s = "hello, big world";
        assert_eq!(next_word(s, 0), 5);
        assert_eq!(next_word(s, 5), 10);
        assert_eq!(prev_word(s, s.len()), 11);
        assert_eq!(prev_word(s, 11), 7);
        assert_eq!(prev_word(s, 3), 0);
        assert_eq!(word_at(s, 8), 7..10);
    }
}
