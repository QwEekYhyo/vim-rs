use std::str::Chars;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug)]
pub struct Line {
    text: String,
    has_utf8: bool,
    len: usize,   // number of characters
    width: usize, // unicode width of line
}

impl Line {
    #[must_use]
    pub const fn new() -> Self {
        Line {
            text: String::new(),
            has_utf8: false,
            len: 0,
            width: 0,
        }
    }

    #[must_use]
    pub fn with_string(s: String) -> Self {
        let has_utf8 = !s.is_ascii();

        if has_utf8 {
            Line {
                has_utf8: true,
                len: s.chars().count(),
                width: UnicodeWidthStr::width(s.as_str()),
                text: s,
            }
        } else {
            Line {
                has_utf8: false,
                len: s.len(),
                width: s.len(),
                text: s,
            }
        }
    }

    #[must_use]
    pub fn get_unicode_width_at(&self, index: usize) -> usize {
        if !self.has_utf8 {
            return index;
        }

        if index == self.len {
            return self.width;
        }

        self.text
            .chars()
            .take(index)
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
            .sum()
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    pub fn chars(&self) -> Chars<'_> {
        self.text.chars()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.len = 0;
        self.width = 0;
        self.has_utf8 = false;
    }

    pub fn reserve(&mut self, additional: usize) {
        self.text.reserve(additional);
    }

    pub fn push(&mut self, ch: char) {
        self.text.push(ch);
        self.len += 1;
        if !ch.is_ascii()
            && let Some(width) = UnicodeWidthChar::width(ch)
        {
            self.width += width;
            self.has_utf8 = true;
        } else {
            self.width += 1;
        }
    }
}

impl Extend<char> for Line {
    fn extend<T: IntoIterator<Item = char>>(&mut self, iter: T) {
        let iterator = iter.into_iter();
        let (lower_bound, _) = iterator.size_hint();
        self.reserve(lower_bound);
        iterator.for_each(move |c| self.push(c));
    }
}
