//! Array-of-bytes signatures: `"48 8B 05 ?? ?? ?? ?? 48 85 C0"`, `??` (or `?`) matching any byte.
//!
//! Pure and platform-independent, so signatures can be unit-tested against dumped byte ranges.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<Option<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad AOB pattern {:?}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl Pattern {
    pub fn parse(s: &str) -> Result<Pattern, ParseError> {
        let bytes = s
            .split_whitespace()
            .map(|t| match t {
                "?" | "??" => Ok(None),
                _ if t.len() == 2 => u8::from_str_radix(t, 16)
                    .map(Some)
                    .map_err(|_| ParseError(t.into())),
                _ => Err(ParseError(t.into())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // A pattern of only wildcards matches everywhere: always a mistake.
        if bytes.iter().all(Option::is_none) {
            return Err(ParseError(s.into()));
        }
        Ok(Pattern { bytes })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    fn matches_at(&self, hay: &[u8], at: usize) -> bool {
        self.bytes
            .iter()
            .zip(&hay[at..])
            .all(|(p, b)| p.is_none_or(|p| p == *b))
    }

    /// Every offset in `hay` where the pattern matches.
    pub fn find_all<'a>(&'a self, hay: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
        let end = (hay.len() + 1).saturating_sub(self.len());
        (0..end).filter(move |&i| self.matches_at(hay, i))
    }

    /// The single match, or `None` if there are zero or several (an ambiguous signature is a miss).
    pub fn find_unique(&self, hay: &[u8]) -> Option<usize> {
        let mut it = self.find_all(hay);
        let first = it.next()?;
        it.next().is_none().then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rejects() {
        assert_eq!(Pattern::parse("48 ?? 05").unwrap().len(), 3);
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("?? ??").is_err());
        assert!(Pattern::parse("4G").is_err());
        assert!(Pattern::parse("488B").is_err());
    }

    #[test]
    fn finds_with_wildcards() {
        let hay = [0x90, 0x48, 0x8B, 0x05, 0x11, 0x22, 0x48, 0x8B, 0x05, 0x33];
        let p = Pattern::parse("48 8B 05 ??").unwrap();
        assert_eq!(p.find_all(&hay).collect::<Vec<_>>(), vec![1, 6]);
        assert_eq!(p.find_unique(&hay), None);
        assert_eq!(Pattern::parse("05 11").unwrap().find_unique(&hay), Some(3));
        assert_eq!(Pattern::parse("05 33").unwrap().find_unique(&hay), Some(8));
        let too_long = Pattern::parse("90 48 8B 05 11 22 48 8B 05 33 00").unwrap();
        assert_eq!(too_long.find_unique(&hay), None);
    }
}
