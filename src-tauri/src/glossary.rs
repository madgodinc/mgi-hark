//! The gaming glossary, the same one the service on madgodinc.net uses.
//!
//! A translation model reads "push bot" as "толкать ботов". Four layers fix
//! that around the model, each applied in one pass so a replacement can never
//! be rewritten by a shorter rule:
//!
//! - **calls** - a whole short phrase we know by heart, no model needed;
//! - **terms** - jargon hidden behind a placeholder the model copies as it is;
//! - **pre** - plain English for slang the model would read wrong;
//! - **fixes** - the literal Russian that comes back, turned into player speech.
//!
//! The file is the one in `server/glossary.json`, built into the binary so the
//! two never drift apart.

use regex::{Regex, RegexBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const SOURCE: &str = include_str!("../../server/glossary.json");

#[derive(Deserialize)]
struct Groups {
    groups: HashMap<String, Layers>,
}

#[derive(Deserialize, Default, Clone)]
struct Layers {
    #[serde(default)]
    calls: HashMap<String, String>,
    #[serde(default)]
    terms: HashMap<String, String>,
    #[serde(default)]
    pre: HashMap<String, String>,
    #[serde(default)]
    fixes: HashMap<String, String>,
}

/// One language and one game, compiled into what the passes need.
pub struct Section {
    calls: HashMap<String, String>,
    terms: Pass,
    pre: Pass,
    fixes: Pass,
}

struct Pass {
    /// None when the layer is empty: nothing to match.
    pattern: Option<Regex>,
    table: HashMap<String, String>,
}

impl Pass {
    fn build(pairs: &HashMap<String, String>) -> Pass {
        let mut keys: Vec<&String> = pairs.keys().collect();
        // Longest first, so "smoke mid" wins over "mid".
        keys.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        let table = pairs.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
        let pattern = (!keys.is_empty()).then(|| {
            let body = keys.iter().map(|k| regex::escape(k)).collect::<Vec<_>>().join("|");
            RegexBuilder::new(&format!(r"(?:^|\b)(?:{body})(?:\b|$)"))
                .case_insensitive(true)
                .build()
                .expect("glossary pattern")
        });
        Pass { pattern, table }
    }

    /// One pass over the text; `each` decides what a hit turns into.
    fn apply(&self, text: &str, mut each: impl FnMut(&str) -> String) -> String {
        let Some(pattern) = &self.pattern else { return text.to_string() };
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for m in pattern.find_iter(text) {
            let Some(value) = self.table.get(&m.as_str().to_lowercase()) else { continue };
            out.push_str(&text[at..m.start()]);
            out.push_str(&each(value));
            at = m.end();
        }
        out.push_str(&text[at..]);
        out
    }
}

fn data() -> &'static HashMap<String, HashMap<String, Layers>> {
    static DATA: OnceLock<HashMap<String, HashMap<String, Layers>>> = OnceLock::new();
    DATA.get_or_init(|| {
        // The file starts with a "_comment" for whoever edits it by hand, so the
        // languages are picked out one by one rather than parsed in one go.
        let parsed: HashMap<String, serde_json::Value> = serde_json::from_str(SOURCE).unwrap_or_default();
        parsed
            .into_iter()
            .filter_map(|(lang, value)| {
                let groups: Groups = serde_json::from_value(value).ok()?;
                Some((lang, groups.groups))
            })
            .collect()
    })
}

impl Section {
    /// Common words plus the chosen game; the game's own entries win. `lang` is
    /// the two-letter code of the speech, "en" or "ru".
    pub fn build(lang: &str, game: &str) -> Option<Section> {
        let groups = data().get(lang)?;
        let mut merged = Layers::default();
        let common = groups.get("common").cloned().unwrap_or_default();
        let chosen: Vec<Layers> = match game {
            "all" | "" => {
                let mut all = vec![common];
                all.extend(groups.iter().filter(|(name, _)| *name != "common").map(|(_, g)| g.clone()));
                all
            }
            name => vec![common, groups.get(name).cloned().unwrap_or_default()],
        };
        for layer in chosen {
            merged.calls.extend(layer.calls);
            merged.terms.extend(layer.terms);
            merged.pre.extend(layer.pre);
            merged.fixes.extend(layer.fixes);
        }
        Some(Section {
            calls: merged.calls.iter().map(|(k, v)| (key(k), v.clone())).collect(),
            terms: Pass::build(&merged.terms),
            pre: Pass::build(&merged.pre),
            fixes: Pass::build(&merged.fixes),
        })
    }

    /// What was loaded, for the example binary and the log.
    pub fn summary(&self) -> String {
        format!(
            "{} calls, {} terms, {} rewrites, {} fixes",
            self.calls.len(),
            self.terms.table.len(),
            self.pre.table.len(),
            self.fixes.table.len()
        )
    }

    /// A whole short phrase we know by heart, ready to show without the model.
    pub fn call(&self, text: &str) -> Option<&str> {
        self.calls.get(&key(text)).map(|s| s.as_str())
    }

    /// Hides jargon behind placeholders and rewrites slang into plain language.
    /// Returns the prepared text and what each placeholder stands for.
    pub fn prepare(&self, text: &str) -> (String, Vec<(String, String)>) {
        let mut marks: Vec<(String, String)> = Vec::new();
        let hidden = self.terms.apply(text, |word| {
            let token = format!("ZQ{}", marks.len() + 1);
            marks.push((token.clone(), word.to_string()));
            token
        });
        (self.pre.apply(&hidden, |plain| plain.to_string()), marks)
    }

    /// Turns the literal translation back into what players say.
    pub fn repair(&self, text: &str) -> String {
        self.fixes.apply(text, |said| said.to_string())
    }
}

/// Placeholders come back as they were sent, or transliterated into Cyrillic:
/// the model wrote "ЗК1" for "ZQ1" often enough to matter.
pub fn restore(text: &str, marks: &[(String, String)]) -> String {
    if marks.is_empty() {
        return text.to_string();
    }
    static MARK: OnceLock<Regex> = OnceLock::new();
    let mark = MARK.get_or_init(|| {
        RegexBuilder::new(r"[ZЗ3]\s?[QКK]\s?(\d+)").case_insensitive(true).build().expect("mark pattern")
    });
    let out = mark
        .replace_all(text, |caps: &regex::Captures| {
            let token = format!("ZQ{}", &caps[1]);
            marks
                .iter()
                .find(|(name, _)| *name == token)
                .map(|(_, word)| word.clone())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();
    // A placeholder is an unknown word to the model, and it likes to put a dash
    // after unknown words: "Иду на мид - прикрывай меня".
    marks.iter().fold(out, |text, (_, word)| text.replace(&format!("{word} - "), &format!("{word}, ")))
}

/// Lower case, no punctuation: how a spoken phrase is looked up.
fn key(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}
