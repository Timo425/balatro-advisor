//! Joker descriptions, read at runtime from the player's own `Balatro.exe`.
//!
//! The game text (`localization/en-us.lua`) and the values it shows (`card.lua`,
//! `generate_UIBox_ability_table`: `loc_vars = {...}` per joker) stay in the player's
//! install; nothing from them is committed here. A small evaluator fills the `#1#`
//! placeholders from the joker's own ability table, so counters show their current value.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::lua;

pub struct Texts {
    /// joker key → (name, text lines with style codes still in)
    text: HashMap<String, (String, Vec<String>)>,
    /// joker *name* → loc_vars expressions
    vars: HashMap<String, Vec<String>>,
}

/// Game state the descriptions can refer to.
#[derive(Debug, Clone, Default)]
pub struct DescCtx {
    pub probability: f64,
    pub dollars: f64,
    pub starting_deck_size: i64,
    pub playing_cards: i64,
    pub deck_cards: i64,
    pub jokers: i64,
    pub tarots_used: i64,
    pub skips: i64,
    /// `current_round` targets: idol rank/suit, castle suit, ancient suit, mail rank
    pub idol_rank: String,
    pub idol_suit: String,
    pub castle_suit: String,
    pub ancient_suit: String,
    pub mail_rank: String,
}

/// Loaded once from the local install; `None` if the game isn't found.
pub fn texts() -> Option<&'static Texts> {
    static T: OnceLock<Option<Texts>> = OnceLock::new();
    T.get_or_init(|| game_exe().and_then(|p| load(&p).ok())).as_ref()
}

/// Where Balatro.exe is: `game_exe` in the config, then the usual Steam libraries.
pub fn game_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("BALATRO_EXE") {
        return Some(PathBuf::from(p));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(h.join(".steam/debian-installation"));
        roots.push(h.join(".steam/steam"));
        roots.push(h.join(".local/share/Steam"));
        roots.push(h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    roots.push(PathBuf::from("C:/Program Files (x86)/Steam"));
    let mut libs = roots.clone();
    for r in &roots {
        if let Ok(t) = std::fs::read_to_string(r.join("steamapps/libraryfolders.vdf")) {
            for l in t.lines() {
                let mut parts = l.split('"').filter(|s| !s.trim().is_empty());
                if parts.next() == Some("path") {
                    if let Some(p) = parts.next() {
                        libs.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    libs.into_iter().map(|l| l.join("steamapps/common/Balatro/Balatro.exe")).find(|p| p.is_file())
}

fn read_member(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String, String> {
    let mut f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

pub fn load(exe: &Path) -> Result<Texts, String> {
    let file = std::fs::File::open(exe).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let loc = read_member(&mut zip, "localization/en-us.lua")?;
    let card = read_member(&mut zip, "card.lua")?;

    let v = lua::parse(&loc).map_err(|e| e.to_string())?;
    let mut text = HashMap::new();
    if let Some(t) = v.at("descriptions.Joker").table() {
        for (k, d) in &t.entries {
            if let lua::Key::Str(key) = k {
                let lines = d.get("text").list().iter().filter_map(|l| l.str().map(str::to_string)).collect();
                text.insert(key.clone(), (d.get("name").str().unwrap_or(key).to_string(), lines));
            }
        }
    }
    Ok(Texts { text, vars: parse_loc_vars(&card) })
}

/// `self.ability.name == 'X' [or ... == 'Y'] then loc_vars = {a, b}` → X/Y → [a, b]
fn parse_loc_vars(card: &str) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    let start = card.find("elseif self.ability.set == 'Joker' then").unwrap_or(0);
    let body = &card[start..];
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    let b = body.as_bytes();
    while i < b.len() {
        if body[i..].starts_with("self.ability.name == '") {
            let s = i + "self.ability.name == '".len();
            if let Some(e) = body[s..].find('\'') {
                names.push(body[s..s + e].to_string());
                i = s + e;
            }
        } else if body[i..].starts_with("loc_vars = {") {
            let s = i + "loc_vars = {".len();
            let (mut depth, mut j) = (1, s);
            while j < b.len() && depth > 0 {
                match b[j] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let inner = &body[s..j.saturating_sub(1)];
            let args = split_top(inner);
            for n in names.drain(..) {
                out.entry(n).or_insert_with(|| args.clone());
            }
            i = j;
        } else if body[i..].starts_with("\n        elseif") || body[i..].starts_with("\n        if") {
            // a new branch without loc_vars on the way: forget stale names
            names.clear();
            i += 1;
        } else {
            i += 1;
        }
        if body[i.min(body.len())..].starts_with("\n    end") {
            break;
        }
    }
    out
}

fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut quote) = (Vec::new(), String::new(), 0i32, None::<char>);
    for c in s.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '(' | '{' | '[' => {
                    depth += 1;
                    cur.push(c);
                }
                ')' | '}' | ']' => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
                _ => cur.push(c),
            },
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

impl Texts {
    /// The joker's text with `#n#` filled in and style codes removed, one line per text line.
    pub fn describe(&self, key: &str, ability: &serde_json::Value, ctx: &DescCtx) -> Option<String> {
        let (name, lines) = self.text.get(key)?;
        let vars = self.vars.get(name.as_str()).cloned().unwrap_or_default();
        let values: Vec<String> = vars.iter().map(|e| Eval::new(e, ability, ctx).run().map_or("?".into(), |v| v.show())).collect();
        let filled: Vec<String> = lines
            .iter()
            .map(|l| {
                let mut s = strip_codes(l);
                for (i, v) in values.iter().enumerate() {
                    s = s.replace(&format!("#{}#", i + 1), v);
                }
                s
            })
            .collect();
        Some(filled.join(" "))
    }
}

/// Removes `{C:red}` / `{X:mult,C:white}` / `{}` style markers.
fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut skip = false;
    for c in s.chars() {
        match c {
            '{' => skip = true,
            '}' => skip = false,
            _ if !skip => out.push(c),
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone)]
enum V {
    Num(f64),
    Str(String),
    Nil,
    Bool(bool),
}

impl V {
    fn show(&self) -> String {
        match self {
            V::Num(n) if n.fract() == 0.0 => format!("{}", *n as i64),
            V::Num(n) => {
                let s = format!("{n:.2}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
            V::Str(s) => s.clone(),
            V::Bool(b) => b.to_string(),
            V::Nil => "?".into(),
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            V::Num(n) => Some(*n),
            V::Str(s) => s.parse().ok(),
            _ => None,
        }
    }
    fn truthy(&self) -> bool {
        !matches!(self, V::Nil | V::Bool(false))
    }
}

/// A tiny evaluator for the Lua expressions inside `loc_vars = {...}`.
struct Eval<'a> {
    toks: Vec<String>,
    pos: usize,
    ability: &'a serde_json::Value,
    ctx: &'a DescCtx,
}

impl<'a> Eval<'a> {
    fn new(src: &str, ability: &'a serde_json::Value, ctx: &'a DescCtx) -> Eval<'a> {
        Eval { toks: tokenize(src), pos: 0, ability, ctx }
    }

    fn run(mut self) -> Option<V> {
        let v = self.or()?;
        (self.pos == self.toks.len()).then_some(v)
    }

    fn peek(&self) -> Option<&str> {
        self.toks.get(self.pos).map(String::as_str)
    }

    fn eat(&mut self, t: &str) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Option<V> {
        let mut l = self.and()?;
        while self.eat("or") {
            let r = self.and()?;
            if !l.truthy() {
                l = r;
            }
        }
        Some(l)
    }

    fn and(&mut self) -> Option<V> {
        let mut l = self.concat()?;
        while self.eat("and") {
            let r = self.concat()?;
            l = if l.truthy() { r } else { l };
        }
        Some(l)
    }

    fn concat(&mut self) -> Option<V> {
        let mut l = self.add()?;
        while self.eat("..") {
            let r = self.add()?;
            l = V::Str(format!("{}{}", if matches!(l, V::Str(ref s) if s.is_empty()) { String::new() } else { l.show() }, r.show()));
        }
        Some(l)
    }

    fn add(&mut self) -> Option<V> {
        let mut l = self.mul()?;
        loop {
            if self.eat("+") {
                l = V::Num(l.num()? + self.mul()?.num()?);
            } else if self.eat("-") {
                l = V::Num(l.num()? - self.mul()?.num()?);
            } else {
                return Some(l);
            }
        }
    }

    fn mul(&mut self) -> Option<V> {
        let mut l = self.unary()?;
        loop {
            if self.eat("*") {
                l = V::Num(l.num()? * self.unary()?.num()?);
            } else if self.eat("/") {
                l = V::Num(l.num()? / self.unary()?.num()?);
            } else {
                return Some(l);
            }
        }
    }

    fn unary(&mut self) -> Option<V> {
        if self.eat("-") {
            return Some(V::Num(-self.unary()?.num()?));
        }
        if self.eat("not") {
            return Some(V::Bool(!self.unary()?.truthy()));
        }
        if self.eat("#") {
            let path = self.toks.get(self.pos)?.clone();
            self.pos += 1;
            return Some(V::Num(match path.as_str() {
                "G.jokers.cards" => self.ctx.jokers as f64,
                "G.playing_cards" => self.ctx.playing_cards as f64,
                "G.deck.cards" => self.ctx.deck_cards as f64,
                _ => return None,
            }));
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<V> {
        let t = self.toks.get(self.pos)?.clone();
        self.pos += 1;
        if t == "(" {
            let v = self.or()?;
            self.eat(")");
            return Some(v);
        }
        if let Ok(n) = t.parse::<f64>() {
            return Some(V::Num(n));
        }
        if (t.starts_with('\'') || t.starts_with('"')) && t.len() >= 2 {
            return Some(V::Str(t[1..t.len() - 1].to_string()));
        }
        match t.as_str() {
            "true" => return Some(V::Bool(true)),
            "false" => return Some(V::Bool(false)),
            "nil" => return Some(V::Nil),
            _ => {}
        }
        if self.peek() == Some("(") {
            self.pos += 1;
            let mut args = Vec::new();
            if !self.eat(")") {
                loop {
                    args.push(self.or()?);
                    if self.eat(")") {
                        break;
                    }
                    if !self.eat(",") {
                        return None;
                    }
                }
            }
            return self.call(&t, args);
        }
        if self.peek() == Some("{") {
            return None; // localize{...} and other table calls: not supported
        }
        Some(self.path(&t))
    }

    fn call(&self, f: &str, args: Vec<V>) -> Option<V> {
        let n = |i: usize| args.get(i).and_then(V::num);
        match f {
            "math.max" => Some(V::Num(n(0)?.max(n(1)?))),
            "math.min" => Some(V::Num(n(0)?.min(n(1)?))),
            "math.floor" => Some(V::Num(n(0)?.floor())),
            "localize" => {
                let s = args.first()?.show();
                let kind = args.get(1).map(V::show).unwrap_or_default();
                Some(V::Str(if kind == "suits_singular" { s.trim_end_matches('s').to_string() } else { s }))
            }
            "tostring" => Some(V::Str(args.first()?.show())),
            _ => None,
        }
    }

    fn path(&self, p: &str) -> V {
        let c = self.ctx;
        if let Some(rest) = p.strip_prefix("self.ability.") {
            let v = rest.split('.').fold(Some(self.ability), |v, k| v.and_then(|v| v.get(k)));
            return match v {
                Some(serde_json::Value::Number(n)) => V::Num(n.as_f64().unwrap_or(0.0)),
                Some(serde_json::Value::String(s)) => V::Str(s.clone()),
                Some(serde_json::Value::Bool(b)) => V::Bool(*b),
                _ => V::Nil,
            };
        }
        match p {
            "G.GAME" | "G.jokers" | "G.deck" | "G.GAME.consumeable_usage_total" => V::Bool(true),
            "G.GAME.probabilities.normal" => V::Num(c.probability),
            "G.GAME.dollars" => V::Num(c.dollars),
            "G.GAME.skips" => V::Num(c.skips as f64),
            "G.GAME.starting_deck_size" => V::Num(c.starting_deck_size as f64),
            "G.GAME.consumeable_usage_total.tarot" => V::Num(c.tarots_used as f64),
            "G.GAME.current_round.idol_card.rank" => V::Str(c.idol_rank.clone()),
            "G.GAME.current_round.idol_card.suit" => V::Str(c.idol_suit.clone()),
            "G.GAME.current_round.castle_card.suit" => V::Str(c.castle_suit.clone()),
            "G.GAME.current_round.ancient_card.suit" => V::Str(c.ancient_suit.clone()),
            "G.GAME.current_round.mail_card.rank" => V::Str(c.mail_rank.clone()),
            _ => V::Nil,
        }
    }
}

fn tokenize(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '\'' || c == '"' {
            let mut j = i + 1;
            while j < cs.len() && cs[j] != c {
                j += 1;
            }
            out.push(cs[i..=j.min(cs.len() - 1)].iter().collect());
            i = j + 1;
        } else if c == '.' && cs.get(i + 1) == Some(&'.') {
            out.push("..".into());
            i += 2;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let mut j = i;
            while j < cs.len() && (cs[j].is_ascii_digit() || (cs[j] == '.' && cs.get(j + 1) != Some(&'.'))) {
                j += 1;
            }
            out.push(cs[i..j].iter().collect());
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < cs.len() && (cs[j].is_alphanumeric() || cs[j] == '_' || (cs[j] == '.' && cs.get(j + 1) != Some(&'.'))) {
                j += 1;
            }
            out.push(cs[i..j].iter().collect());
            i = j;
        } else {
            out.push(c.to_string());
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_loc_var_expressions() {
        let ab = serde_json::json!({"extra": {"mult": 20, "size": 3}, "t_mult": 10, "type": "Flush"});
        let ctx = DescCtx { probability: 1.0, jokers: 4, ..Default::default() };
        let e = |s: &str| Eval::new(s, &ab, &ctx).run().map(|v| v.show());
        assert_eq!(e("self.ability.extra.mult").as_deref(), Some("20"));
        assert_eq!(e("localize(self.ability.type, 'poker_hands')").as_deref(), Some("Flush"));
        assert_eq!(e("''..(G.GAME and G.GAME.probabilities.normal or 1)").as_deref(), Some("1"));
        assert_eq!(e("self.ability.extra.size * 2 + 1").as_deref(), Some("7"));
        assert_eq!(e("#G.jokers.cards * 3").as_deref(), Some("12"));
        assert_eq!(e("math.max(0, self.ability.t_mult - 12)").as_deref(), Some("0"));
    }

    #[test]
    fn strips_style_codes() {
        assert_eq!(strip_codes("{C:red}+#1#{} Mult if played"), "+#1# Mult if played");
    }

    #[test]
    fn describes_from_the_local_install() {
        let Some(t) = texts() else { return }; // skipped without a local Balatro install
        let ab = serde_json::json!({"extra": {"mult": 20, "size": 3}});
        let d = t.describe("j_half", &ab, &DescCtx::default()).unwrap();
        assert!(d.contains("+20 Mult") && d.contains("3 or fewer"), "{d}");
    }
}
