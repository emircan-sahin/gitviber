//! Reading what an agent CLI answers a review prompt with (guide, risks), and writing the parts of
//! the prompt they share: the schema's example and the language name.

use serde_json::{Map, Value, json};

/// A JSON Schema node, kept in the order it's written: the prompt's example follows it.
#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub ty: &'static str,
    pub description: Option<String>,
    pub choices: Vec<&'static str>,
    pub example: Option<Value>,
    pub properties: Vec<(&'static str, Schema)>,
    pub items: Option<Box<Schema>>,
    pub required: Vec<&'static str>,
}

impl Schema {
    pub fn string(description: &str) -> Self {
        Schema {
            ty: "string",
            description: Some(description.into()),
            ..Default::default()
        }
    }

    pub fn choice(choices: &[&'static str]) -> Self {
        Schema {
            ty: "string",
            choices: choices.to_vec(),
            ..Default::default()
        }
    }

    pub fn object(required: &[&'static str], properties: Vec<(&'static str, Schema)>) -> Self {
        Schema {
            ty: "object",
            required: required.to_vec(),
            properties,
            ..Default::default()
        }
    }

    pub fn array(items: Schema) -> Self {
        Schema {
            ty: "array",
            items: Some(Box::new(items)),
            ..Default::default()
        }
    }

    pub fn described(mut self, description: &str) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn example(mut self, example: Value) -> Self {
        self.example = Some(example);
        self
    }

    pub fn property(&self, name: &str) -> Option<&Schema> {
        self.properties
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, s)| s)
    }

    /// As the CLI takes it (Claude Code's --json-schema).
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("type".into(), json!(self.ty));
        if let Some(d) = &self.description {
            out.insert("description".into(), json!(d));
        }
        if !self.choices.is_empty() {
            out.insert("enum".into(), json!(self.choices));
        }
        if let Some(e) = &self.example {
            out.insert("examples".into(), json!([e]));
        }
        if !self.required.is_empty() {
            out.insert("required".into(), json!(self.required));
        }
        if !self.properties.is_empty() {
            let props: Map<String, Value> = self
                .properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_json()))
                .collect();
            out.insert("properties".into(), Value::Object(props));
        }
        if let Some(items) = &self.items {
            out.insert("items".into(), items.to_json());
        }
        Value::Object(out)
    }

    /// The schema as an example of itself: each value its description (or example, or choices).
    pub fn example_of(&self) -> String {
        if !self.properties.is_empty() {
            let parts: Vec<String> = self
                .properties
                .iter()
                .map(|(k, v)| format!("\"{k}\": {}", v.example_of()))
                .collect();
            return format!("{{{}}}", parts.join(", "));
        }
        if let Some(items) = &self.items {
            return format!("[{}]", items.example_of());
        }
        if let Some(e) = &self.example {
            return e.to_string();
        }
        let said = self
            .description
            .clone()
            .unwrap_or_else(|| self.choices.join(" or "));
        Value::String(said).to_string()
    }
}

/// Combining marks of the scripts people write a language's name in: Rust's `is_alphabetic`
/// leaves some out (a virama), which would split a name like हिन्दी.
const MARKS: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06ED),
    (0x0711, 0x0711),
    (0x0730, 0x074A),
    (0x0900, 0x0903),
    (0x093A, 0x094F),
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    (0x0981, 0x0983),
    (0x09BC, 0x09D7),
    (0x09E2, 0x09E3),
    (0x0A01, 0x0A03),
    (0x0A3C, 0x0A51),
    (0x0A70, 0x0A75),
    (0x0A81, 0x0A83),
    (0x0ABC, 0x0ACD),
    (0x0AE2, 0x0AE3),
    (0x0B01, 0x0B03),
    (0x0B3C, 0x0B57),
    (0x0B62, 0x0B63),
    (0x0B82, 0x0B82),
    (0x0BBE, 0x0BD7),
    (0x0C00, 0x0C04),
    (0x0C3E, 0x0C56),
    (0x0C62, 0x0C63),
    (0x0C81, 0x0C83),
    (0x0CBC, 0x0CD6),
    (0x0CE2, 0x0CE3),
    (0x0D00, 0x0D03),
    (0x0D3B, 0x0D4D),
    (0x0D57, 0x0D57),
    (0x0D62, 0x0D63),
    (0x0D81, 0x0D83),
    (0x0DCA, 0x0DDF),
    (0x0DF2, 0x0DF3),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x0EB1, 0x0EB1),
    (0x0EB4, 0x0EBC),
    (0x0EC8, 0x0ECD),
    (0x0F18, 0x0F19),
    (0x0F35, 0x0F39),
    (0x0F3E, 0x0F3F),
    (0x0F71, 0x0F87),
    (0x0F8D, 0x0FBC),
    (0x102B, 0x103E),
    (0x1056, 0x1059),
    (0x17B4, 0x17D3),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x20D0, 0x20FF),
    (0x302A, 0x302F),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
];

fn is_mark(c: char) -> bool {
    let c = c as u32;
    MARKS.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

/// The language reviews are written in (Settings → Guided Review), as a name and not an
/// instruction: letters of any script (with their marks), spaces and '()- only, on one line, so
/// ". Ignore the schema" can't become a sentence of a prompt. Empty reads as English.
pub fn language_name(language: &str) -> String {
    let kept: String = language
        .chars()
        .filter(|&c| c.is_alphabetic() || is_mark(c) || c.is_whitespace() || "'()-".contains(c))
        .collect();
    let one = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one.chars().take(40).collect();
    let cut = cut.trim();
    if cut.is_empty() {
        "English".into()
    } else {
        cut.into()
    }
}

/// JavaScript's trim, which also takes a byte-order mark.
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// A string as written, trimmed; "" for anything else.
pub fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => trim(s).to_string(),
        _ => String::new(),
    }
}

/// At most `max` characters, the cut marked with an ellipsis.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// Runs of white space as one space.
pub fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The objects of an array; none for anything else.
pub fn records(v: Option<&Value>) -> Vec<&Map<String, Value>> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_object).collect(),
        _ => vec![],
    }
}

/// A number as written, or in a string of digits; None for anything else.
pub fn whole(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
                .then(|| t.parse::<f64>().ok())
                .flatten()
        }
        _ => None,
    }
}

/// A whole number above zero that a double holds exactly, as JavaScript's isSafeInteger.
pub fn positive_line(v: Option<&Value>) -> Option<u64> {
    let n = whole(v)?;
    (n.fract() == 0. && n > 0. && n <= 9_007_199_254_740_991.).then_some(n as u64)
}

pub fn flag(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true))) || text(v).eq_ignore_ascii_case("true")
}

/// `,` before a closing bracket, which models write and JSON doesn't take, gone.
fn without_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        if c == ',' {
            let rest = s[i + 1..].trim_start();
            if rest.starts_with('}') || rest.starts_with(']') {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// `answer` read as JSON, then with the trailing commas taken out.
fn parse(answer: &str) -> Option<Value> {
    serde_json::from_str(answer)
        .ok()
        .or_else(|| serde_json::from_str(&without_trailing_commas(answer)).ok())
}

/// The JSON in an answer: all of it, or the first `{…}` with one of `keys`, wherever it is (a
/// fence, a "Here's the review:" or a `{ retries }` in the words around it). One pass that pairs
/// the braces outside strings, so a brace in a string or a long answer costs no more.
pub fn json_of(answer: &str, keys: &[&str]) -> Option<Value> {
    if let Some(all) = parse(answer) {
        return Some(all);
    }
    let bytes = answer.as_bytes();
    let (mut depth, mut start, mut quoted) = (0usize, 0usize, false);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Strings count only inside an object: the words around it have quotes of their own.
        if quoted {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                quoted = false;
            }
        } else if c == b'"' && depth > 0 {
            quoted = true;
        } else if c == b'{' {
            if depth == 0 {
                start = i;
            }
            depth += 1;
        } else if c == b'}' && depth > 0 {
            depth -= 1;
            if depth == 0
                && let Some(v) = parse(&answer[start..=i])
                && v.as_object()
                    .is_some_and(|o| keys.iter().any(|k| o.contains_key(*k)))
            {
                return Some(v);
            }
        }
        i += 1;
    }
    None
}

/// An answer as the parsers read it: line ends made `\n`, trimmed.
pub fn normalized(output: &str) -> String {
    trim(&output.replace("\r\n", "\n").replace('\r', "\n")).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_marks_the_cut() {
        assert_eq!(clip("abcdef", 6), "abcdef");
        assert_eq!(clip("abc  def", 6), "abc…");
        assert_eq!(clip("üüüüüüü", 3), "üü…");
    }

    #[test]
    fn whole_numbers_as_written_or_in_digits() {
        assert_eq!(whole(Some(&json!(3))), Some(3.));
        assert_eq!(whole(Some(&json!(" 12 "))), Some(12.));
        assert_eq!(whole(Some(&json!("1e3"))), None);
        assert_eq!(whole(Some(&json!([4]))), None);
        assert_eq!(positive_line(Some(&json!(1.5))), None);
        assert_eq!(positive_line(Some(&json!(0))), None);
    }

    #[test]
    fn json_in_the_words_around_it() {
        let v = json_of(
            "I tried `{ retries }`. Here:\n{\"risks\": []} done",
            &["risks"],
        );
        assert_eq!(v, Some(json!({"risks": []})));
        assert_eq!(json_of("{\"a\": [1,],}", &["a"]), Some(json!({"a": [1]})));
        assert_eq!(json_of("no json", &["a"]), None);
    }
}
