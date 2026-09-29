//! Minimal Lua table-literal parser.
//!
//! Handles two dialects:
//! - what Balatro's `STR_PACK` writes into `.jkr` files: `{["k"]=v,[1]=v,}`
//! - the source-style tables in `game.lua` (`key = {a = 1, 'x'}`), used by the
//!   data extractor. Function calls (`HEX('fff')`) and bare identifiers
//!   (`G.C.RED`) there parse as `Nil`.
//!
//! The idea of a tiny recursive parser comes from balatro-agent's
//! `tools/balatro_state.py::parse_lua`; this is a rewrite, not a port.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(Table),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Int(i64),
    Str(String),
}

/// A Lua table, entries kept in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub entries: Vec<(Key, Value)>,
}

static NIL: Value = Value::Nil;

impl Table {
    pub fn get(&self, k: &str) -> &Value {
        self.entries
            .iter()
            .find(|(key, _)| matches!(key, Key::Str(s) if s == k))
            .map_or(&NIL, |(_, v)| v)
    }

    pub fn get_int(&self, k: i64) -> &Value {
        self.entries
            .iter()
            .find(|(key, _)| *key == Key::Int(k))
            .map_or(&NIL, |(_, v)| v)
    }

    /// Values under integer keys, sorted by key. Card areas store cards this way.
    pub fn int_values(&self) -> Vec<&Value> {
        let mut v: Vec<(i64, &Value)> = self
            .entries
            .iter()
            .filter_map(|(k, v)| match k {
                Key::Int(i) => Some((*i, v)),
                Key::Str(_) => None,
            })
            .collect();
        v.sort_by_key(|(i, _)| *i);
        v.into_iter().map(|(_, v)| v).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Value {
    /// Path lookup: `v.at("GAME.current_round.hands_left")`. Missing → `Nil`.
    pub fn at(&self, path: &str) -> &Value {
        path.split('.').fold(self, |v, part| v.get(part))
    }

    pub fn get(&self, k: &str) -> &Value {
        match self {
            Value::Table(t) => t.get(k),
            _ => &NIL,
        }
    }

    pub fn table(&self) -> Option<&Table> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn int(&self) -> Option<i64> {
        self.num().filter(|n| n.fract() == 0.0).map(|n| n as i64)
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Lua truthiness: everything except nil and false.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Values under integer keys (e.g. `cards`), in key order.
    pub fn list(&self) -> Vec<&Value> {
        self.table().map(Table::int_values).unwrap_or_default()
    }

    /// Converts to JSON. Tables with keys exactly 1..n become arrays.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Nil => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 9.0e15 {
                    J::from(*n as i64)
                } else {
                    serde_json::Number::from_f64(*n).map_or_else(|| J::String(n.to_string()), J::Number)
                }
            }
            Value::Str(s) => J::String(s.clone()),
            Value::Table(t) => {
                let is_seq = !t.is_empty()
                    && t.entries.iter().enumerate().all(|(i, (k, _))| *k == Key::Int(i as i64 + 1));
                if is_seq {
                    J::Array(t.entries.iter().map(|(_, v)| v.to_json()).collect())
                } else {
                    J::Object(
                        t.entries
                            .iter()
                            .map(|(k, v)| {
                                let key = match k {
                                    Key::Int(i) => i.to_string(),
                                    Key::Str(s) => s.clone(),
                                };
                                (key, v.to_json())
                            })
                            .collect(),
                    )
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub pos: usize,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lua parse error at byte {}: {}", self.pos, self.msg)
    }
}

impl std::error::Error for ParseError {}

/// Parses one value; a leading `return` is allowed. Trailing input is an error.
pub fn parse(src: &str) -> Result<Value, ParseError> {
    let mut p = Parser { s: src.as_bytes(), pos: 0 };
    p.ws();
    if p.s[p.pos..].starts_with(b"return") {
        p.pos += 6;
    }
    let v = p.value()?;
    p.ws();
    if p.pos != p.s.len() {
        return Err(p.err("trailing input"));
    }
    Ok(v)
}

/// Parses one value at the start of `src` and returns it with the number of bytes used.
pub fn parse_prefix(src: &str) -> Result<(Value, usize), ParseError> {
    let mut p = Parser { s: src.as_bytes(), pos: 0 };
    let v = p.value()?;
    Ok((v, p.pos))
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError { pos: self.pos, msg: msg.to_string() }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    /// Skips whitespace and `--` comments.
    fn ws(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                self.pos += 1;
            }
            if self.s[self.pos..].starts_with(b"--") {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.pos += 1;
                }
            } else {
                return;
            }
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        self.ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        self.ws();
        let v = match self.peek().ok_or_else(|| self.err("unexpected end"))? {
            b'{' => self.table()?,
            b'"' | b'\'' => Value::Str(self.string()?),
            c if c == b'-' || c == b'.' || c.is_ascii_digit() => self.number()?,
            c if c.is_ascii_alphabetic() || c == b'_' => self.word()?,
            c => return Err(self.err(&format!("unexpected '{}'", c as char))),
        };
        // Source tables occasionally hold simple arithmetic (`1/3`). Fold it.
        self.ws();
        if let (Value::Num(a), Some(op @ (b'/' | b'*' | b'+'))) = (&v, self.peek()) {
            let a = *a;
            self.pos += 1;
            let b = self.value()?.num().ok_or_else(|| self.err("expected number after operator"))?;
            return Ok(Value::Num(match op {
                b'/' => a / b,
                b'*' => a * b,
                _ => a + b,
            }));
        }
        Ok(v)
    }

    fn table(&mut self) -> Result<Value, ParseError> {
        self.expect(b'{')?;
        let mut t = Table::default();
        let mut next_index = 1i64;
        loop {
            self.ws();
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Table(t));
                }
                Some(b',' | b';') => {
                    self.pos += 1;
                    continue;
                }
                Some(b'[') => {
                    self.pos += 1;
                    let key = match self.value()? {
                        Value::Str(s) => Key::Str(s),
                        Value::Num(n) if n.fract() == 0.0 => Key::Int(n as i64),
                        _ => return Err(self.err("unsupported table key")),
                    };
                    self.expect(b']')?;
                    self.expect(b'=')?;
                    let v = self.value()?;
                    t.entries.push((key, v));
                }
                Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                    // `ident = value`, or a positional bare word / call.
                    let start = self.pos;
                    let ident = self.ident();
                    self.ws();
                    if self.peek() == Some(b'=') && self.s.get(self.pos + 1) != Some(&b'=') {
                        self.pos += 1;
                        let v = self.value()?;
                        t.entries.push((Key::Str(ident), v));
                    } else {
                        self.pos = start;
                        let v = self.value()?;
                        t.entries.push((Key::Int(next_index), v));
                        next_index += 1;
                    }
                }
                Some(_) => {
                    let v = self.value()?;
                    t.entries.push((Key::Int(next_index), v));
                    next_index += 1;
                }
                None => return Err(self.err("unterminated table")),
            }
        }
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.pos]).into_owned()
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let quote = self.s[self.pos];
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.pos += 1;
            match c {
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("bad escape"))?;
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'\n' => out.push(b'\n'),
                        d if d.is_ascii_digit() => {
                            // \ddd decimal escape (STR_PACK uses %q for strings)
                            let mut n = u32::from(d - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d) if d.is_ascii_digit() => {
                                        n = n * 10 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(n as u8);
                        }
                        other => out.push(other),
                    }
                }
                c if c == quote => break,
                c => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
            // tostring() of infinities / NaN
            for (word, v) in [("inf", f64::NEG_INFINITY), ("nan", f64::NAN)] {
                if self.s[self.pos..].starts_with(word.as_bytes()) {
                    self.pos += word.len();
                    return Ok(Value::Num(v));
                }
            }
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'.') {
            self.pos += 1;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or("");
        text.parse::<f64>()
            .map(Value::Num)
            .map_err(|_| ParseError { pos: start, msg: format!("bad number '{text}'") })
    }

    fn word(&mut self) -> Result<Value, ParseError> {
        let w = self.ident();
        match w.as_str() {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            "nil" => return Ok(Value::Nil),
            "inf" => return Ok(Value::Num(f64::INFINITY)),
            "nan" => return Ok(Value::Num(f64::NAN)),
            _ => {}
        }
        // Source-only: dotted names and calls, e.g. `G.C.RED`, `HEX('fff')`, `G.C.SET.Tarot`.
        loop {
            match self.peek() {
                Some(b'.' | b':') => {
                    self.pos += 1;
                    self.ident();
                }
                Some(b'(') => self.skip_balanced(b'(', b')')?,
                Some(b'[') => self.skip_balanced(b'[', b']')?,
                _ => break,
            }
        }
        Ok(Value::Nil)
    }

    fn skip_balanced(&mut self, open: u8, close: u8) -> Result<(), ParseError> {
        let mut depth = 0;
        loop {
            match self.peek().ok_or_else(|| self.err("unbalanced brackets"))? {
                b'"' | b'\'' => {
                    self.string()?;
                    continue;
                }
                c if c == open => depth += 1,
                c if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(());
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_dialect() {
        let v = parse(r#"return {["a"]={[1]="x",[2]="y\"z",},["n"]=-1.5e3,["t"]=true,["w"]={[8]=3,[2]=1,},}"#)
            .unwrap();
        assert_eq!(v.at("a").list().len(), 2);
        assert_eq!(v.at("a").list()[1].str(), Some("y\"z"));
        assert_eq!(v.get("n").num(), Some(-1500.0));
        assert_eq!(v.get("t").bool(), Some(true));
        // Integer keys stay integers, not list positions
        assert_eq!(v.at("w").table().unwrap().get_int(8).int(), Some(3));
    }

    #[test]
    fn infinities_and_nan() {
        let v = parse(r#"{["a"]=inf,["b"]=-inf,["c"]=nan,["d"]=-nan,}"#).unwrap();
        assert_eq!(v.get("a").num(), Some(f64::INFINITY));
        assert_eq!(v.get("b").num(), Some(f64::NEG_INFINITY));
        assert!(v.get("c").num().unwrap().is_nan());
        assert!(v.get("d").num().unwrap().is_nan());
    }

    #[test]
    fn source_dialect() {
        let src = r#"{order = 6, name = "Jolly Joker", pos = {x=2,y=0}, colour = HEX('fff'), c = G.C.RED, config = {t_mult = 8, type = 'Pair', extra = 1/4}, list = {'a', 'b'}} -- comment"#;
        let v = parse(src).unwrap();
        assert_eq!(v.get("name").str(), Some("Jolly Joker"));
        assert_eq!(v.at("config.t_mult").int(), Some(8));
        assert_eq!(v.at("config.type").str(), Some("Pair"));
        assert_eq!(v.at("config.extra").num(), Some(0.25));
        assert!(v.get("colour").is_nil());
        assert_eq!(v.get("list").list()[1].str(), Some("b"));
    }

    #[test]
    fn json_arrays_only_for_sequences() {
        let v = parse(r#"{[1]="a",[2]="b",}"#).unwrap();
        assert!(v.to_json().is_array());
        let v = parse(r#"{[8]=1,}"#).unwrap();
        assert!(v.to_json().is_object());
    }
}
