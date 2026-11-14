//! Key-value metadata dictionary (AVDictionary reproduction).

use std::fmt;

/// Dictionary flags matching FFmpeg av_dict flags.
pub const AV_DICT_MATCH_CASE: u32 = 1;
pub const AV_DICT_IGNORE_SUFFIX: u32 = 2;
pub const AV_DICT_DONT_STRDUP_KEY: u32 = 4;
pub const AV_DICT_DONT_STRDUP_VAL: u32 = 8;
pub const AV_DICT_DONT_OVERWRITE: u32 = 16;
pub const AV_DICT_APPEND: u32 = 32;

/// String key-value metadata container (AVDictionary).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dictionary {
    entries: Vec<(String, String)>,
}

impl Dictionary {
    /// Create an empty Dictionary.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Set a key-value entry with flags.
    pub fn set(&mut self, key: &str, value: &str, flags: u32) {
        let match_case = (flags & AV_DICT_MATCH_CASE) != 0;
        let dont_overwrite = (flags & AV_DICT_DONT_OVERWRITE) != 0;
        let append = (flags & AV_DICT_APPEND) != 0;

        for (k, v) in self.entries.iter_mut() {
            let matches = if match_case {
                k == key
            } else {
                k.eq_ignore_ascii_case(key)
            };

            if matches {
                if dont_overwrite {
                    return;
                }
                if append {
                    v.push_str(value);
                } else {
                    *v = value.to_string();
                }
                return;
            }
        }

        self.entries.push((key.to_string(), value.to_string()));
    }

    /// Get value for a key with case sensitivity option.
    pub fn get(&self, key: &str, flags: u32) -> Option<&str> {
        let match_case = (flags & AV_DICT_MATCH_CASE) != 0;
        for (k, v) in &self.entries {
            let matches = if match_case {
                k == key
            } else {
                k.eq_ignore_ascii_case(key)
            };
            if matches {
                return Some(v.as_str());
            }
        }
        None
    }

    /// Check if key exists.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key, 0).is_some()
    }

    /// Remove a key from the dictionary.
    pub fn remove(&mut self, key: &str, flags: u32) -> Option<String> {
        let match_case = (flags & AV_DICT_MATCH_CASE) != 0;
        if let Some(pos) = self.entries.iter().position(|(k, _)| {
            if match_case {
                k == key
            } else {
                k.eq_ignore_ascii_case(key)
            }
        }) {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Iterate over key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Parse dictionary from string with key-value delimiters (e.g. "k1=v1:k2=v2").
    pub fn parse_string(&mut self, str_val: &str, key_val_sep: char, pairs_sep: char) {
        for pair in str_val.split(pairs_sep) {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some((k, v)) = pair.split_once(key_val_sep) {
                self.set(k.trim(), v.trim(), 0);
            }
        }
    }

    /// Copy all entries from another dictionary.
    pub fn copy_from(&mut self, other: &Self, flags: u32) {
        for (k, v) in other.iter() {
            self.set(k, v, flags);
        }
    }
}

impl fmt::Display for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, (k, v)) in self.entries.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}={}", k, v)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_ops() {
        let mut d = Dictionary::new();
        d.set("title", "My Movie", 0);
        d.set("YEAR", "2026", 0);

        assert_eq!(d.get("Title", 0), Some("My Movie"));
        assert_eq!(d.get("year", 0), Some("2026"));
        assert_eq!(d.get("Title", AV_DICT_MATCH_CASE), None);
        assert_eq!(d.len(), 2);

        d.remove("year", 0);
        assert_eq!(d.len(), 1);
    }
}
