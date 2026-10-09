//! Minimal JS lexer, ported one-for-one from the extraction prototype that was
//! validated against the real `base.js` (see docs/gui-benchmarks/yt.md).
//!
//! It is *not* a JS parser: it only recognises enough structure to brace-match
//! machine-generated code — strings, template literals with `${}`
//! interpolation, regex literals, comments and numeric literals — while
//! recording each token's nesting depth. The extraction pass in
//! [`super::extract`] works purely off this token stream.

/// A token. `depth` is the bracket nesting level *after* the token was
/// consumed (matching the prototype), relative to the start of the scan.
#[derive(Clone, Copy, Debug)]
pub struct Token {
    /// Single punctuation char, or `id`/`str`/`re`.
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// Nesting depth recorded at scan time. For openers this is the depth
    /// *inside* the bracket; for everything else the depth around it.
    pub depth: usize,
    /// `true` when the token came from inside a template `${}` interpolation.
    /// Those belong to the string; statement splitting skips them.
    pub in_tpl: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Id,
    Str,
    Re,
    Sep(char),
}

pub fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

pub fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// The keywords after which a `/` begins a regex, and which never count as a
/// use of a same-named top-level variable.
pub const KEYWORDS: &[&str] = &[
    "var", "let", "const", "function", "return", "new", "typeof", "in", "of", "instanceof",
    "if", "else", "for", "while", "do", "switch", "case", "break", "continue", "this",
    "true", "false", "null", "undefined", "delete", "void", "throw", "try", "catch",
    "finally", "class", "extends", "super", "yield", "await", "default", "export",
    "import", "async", "static",
];

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    prev: String,
    out: Vec<Token>,
    in_tpl_depth: Vec<usize>, // stack of interpolation bases
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let mut lx = Lexer {
        src,
        b: src.as_bytes(),
        i: 0,
        prev: String::new(),
        out: Vec::with_capacity(64),
        in_tpl_depth: Vec::new(),
    };
    lx.scan_code(0, false);
    lx.out
}

impl<'a> Lexer<'a> {
    fn is_regex_start(&self) -> bool {
        self.prev.is_empty()
            || matches!(self.prev.as_str(), "(" | "=" | "," | ":" | ";" | "[" | "!" | "&" | "|" | "?" | "{" | "}" | "+" | "-" | "*" | "%" | "<" | ">" | "^" | "~")
            || KEYWORDS.contains(&self.prev.as_str())
    }

    fn scan_string(&mut self, depth: usize) -> usize {
        let quote = self.b[self.i];
        let start = self.i;
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'\n' => break, // unterminated; bail like the prototype
                c if c == quote => {
                    self.i += 1;
                    break;
                }
                _ => self.i += 1,
            }
        }
        let end = self.i.min(self.b.len());
        self.push(Kind::Str, start, end, depth);
        end
    }

    /// Template literal: text until a backtick or `${`; interpolations are
    /// scanned as code (their tokens flagged `in_tpl`) until the matching `}`.
    fn scan_template(&mut self, depth: usize) -> usize {
        let start = self.i;
        self.i += 1; // opening `
        loop {
            if self.i >= self.b.len() {
                break;
            }
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'`' => {
                    self.i += 1;
                    break;
                }
                b'$' if self.i + 1 < self.b.len() && self.b[self.i + 1] == b'{' => {
                    self.i += 2;
                    // scan interpolation as code; base = depth + 1 so a `}`
                    // that closes it is recognised inside scan_code
                    let base = depth + 1;
                    self.in_tpl_depth.push(base);
                    self.scan_code(base, true);
                    self.in_tpl_depth.pop();
                }
                _ => self.i += 1,
            }
        }
        let end = self.i.min(self.b.len());
        self.push(Kind::Str, start, end, depth);
        end
    }

    fn scan_regex(&mut self, depth: usize) -> Option<usize> {
        let start = self.i;
        let mut j = self.i + 1;
        let mut in_class = false;
        while j < self.b.len() {
            match self.b[j] {
                b'\\' => j += 2,
                b'\n' => return None, // not a regex after all
                b'[' => {
                    in_class = true;
                    j += 1;
                }
                b']' => {
                    in_class = false;
                    j += 1;
                }
                b'/' if !in_class => {
                    j += 1;
                    while j < self.b.len() && self.b[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    self.i = j;
                    self.push(Kind::Re, start, j, depth);
                    self.prev = "re".into();
                    return Some(j);
                }
                _ => j += 1,
            }
        }
        None
    }

    /// Scan code until EOF (`stop` = false) or until the closing bracket that
    /// takes `depth` below `base` (interpolation mode).
    fn scan_code(&mut self, base: usize, stop: bool) {
        let mut depth = base;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            // whitespace
            if c.is_ascii_whitespace() {
                self.i += 1;
                continue;
            }
            // comments
            if c == b'/' && self.i + 1 < self.b.len() && self.b[self.i + 1] == b'/' {
                match self.b[self.i..].iter().position(|&b| b == b'\n') {
                    Some(nl) => self.i += nl,
                    None => self.i = self.b.len(),
                }
                continue;
            }
            if c == b'/' && self.i + 1 < self.b.len() && self.b[self.i + 1] == b'*' {
                match self.b[self.i..].windows(2).position(|w| w == b"*/") {
                    Some(p) => self.i += p + 2,
                    None => self.i = self.b.len(),
                }
                continue;
            }
            // strings / templates
            if c == b'"' || c == b'\'' {
                let end = self.scan_string(depth);
                self.i = end;
                self.prev = "str".into();
                continue;
            }
            if c == b'`' {
                let end = self.scan_template(depth);
                self.i = end;
                self.prev = "str".into();
                continue;
            }
            // regex vs division
            if c == b'/' && self.is_regex_start() {
                if let Some(end) = self.scan_regex(depth) {
                    self.i = end;
                    continue;
                }
                // fall through: division
            }
            // brackets
            if matches!(c, b'(' | b'[' | b'{') {
                depth += 1;
                self.push(Kind::Sep(c as char), self.i, self.i + 1, depth);
                self.i += 1;
                self.prev = (c as char).to_string();
                continue;
            }
            if matches!(c, b')' | b']' | b'}') {
                if stop && depth == base && c == b'}' {
                    self.i += 1; // consume the closing interpolation brace
                    return;
                }
                depth = depth.saturating_sub(1);
                self.push(Kind::Sep(c as char), self.i, self.i + 1, depth);
                self.i += 1;
                self.prev = (c as char).to_string();
                continue;
            }
            // punctuation (single chars suffice for structural work)
            if matches!(
                c,
                b';' | b',' | b'.' | b':' | b'?' | b'=' | b'+' | b'-' | b'*' | b'%' | b'<' | b'>' | b'&' | b'|' | b'!' | b'~' | b'^' | b'/'
            ) {
                self.push(Kind::Sep(c as char), self.i, self.i + 1, depth);
                self.i += 1;
                self.prev = (c as char).to_string();
                continue;
            }
            // numbers: 0x/0o/0b, digits, one '.', exponent, BigInt suffix
            if c.is_ascii_digit() || (c == b'.' && self.b.get(self.i + 1).is_some_and(|n| n.is_ascii_digit()))
            {
                let start = self.i;
                if c == b'0' && self.b.get(self.i + 1).is_some_and(|n| matches!(n, b'x' | b'X' | b'o' | b'O' | b'b' | b'B')) {
                    self.i += 2;
                    while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_') {
                        self.i += 1;
                    }
                } else {
                    while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'_') {
                        self.i += 1;
                    }
                    if self.i < self.b.len() && self.b[self.i] == b'.' {
                        self.i += 1;
                        while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'_') {
                            self.i += 1;
                        }
                    }
                    if self.i < self.b.len()
                        && matches!(self.b[self.i], b'e' | b'E')
                        && self.b.get(self.i + 1).is_some_and(|n| n.is_ascii_digit() || matches!(n, b'+' | b'-'))
                    {
                        self.i += 1;
                        if matches!(self.b[self.i], b'+' | b'-') {
                            self.i += 1;
                        }
                        while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || self.b[self.i] == b'_') {
                            self.i += 1;
                        }
                    }
                    if self.i < self.b.len() && self.b[self.i] == b'n' {
                        self.i += 1; // BigInt
                    }
                }
                let end = self.i;
                self.push(Kind::Id, start, end, depth);
                self.prev = self.src[start..end].to_string();
                continue;
            }
            // identifiers / keywords
            if is_ident_start(c as char) {
                let start = self.i;
                self.i += 1;
                while self.i < self.b.len() && is_ident_char(self.b[self.i] as char) {
                    self.i += 1;
                }
                let end = self.i;
                self.push(Kind::Id, start, end, depth);
                self.prev = self.src[start..end].to_string();
                continue;
            }
            self.i += 1; // unknown char
        }
    }

    fn push(&mut self, kind: Kind, start: usize, end: usize, depth: usize) {
        // Every token emitted while an interpolation base is on the stack
        // belongs to that `${}` (or a template nested inside it); everything
        // else — including the enclosing template's own string token, pushed
        // after the stack is popped — is statement-level material.
        let in_tpl = !self.in_tpl_depth.is_empty();
        self.out.push(Token { kind, start, end, depth, in_tpl });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_balance_on_tricky_input() {
        // regex with brackets, template interpolation with an object literal,
        // string with quotes, exponent number, spread
        let src = r#"var a=/([\-w]+)\(/g,b=`x${ {k:1} }y`+ "q\"t", c=[...d],e=1.5e-3;"#;
        let toks = tokenize(src);
        let mut count = std::collections::HashMap::new();
        for t in &toks {
            if let Kind::Sep(c) = t.kind {
                *count.entry(c).or_insert(0i32) += 1;
            }
        }
        assert_eq!(count.get(&'('), count.get(&')'), "parens balance");
        assert_eq!(count.get(&'{'), count.get(&'}'), "braces balance");
        assert_eq!(count.get(&'['), count.get(&']'), "brackets balance");
        // the regex is one token
        assert!(toks.iter().any(|t| t.kind == Kind::Re));
    }

    #[test]
    fn interpolation_tokens_are_flagged() {
        let src = "a=`x${f(1)}y`;";
        let toks = tokenize(src);
        let f_tok = toks.iter().find(|t| t.kind == Kind::Id && t.start == 6).expect("f tokenized");
        assert!(f_tok.in_tpl, "interpolation tokens carry in_tpl");
        let a_tok = toks.iter().find(|t| t.kind == Kind::Id && t.start == 0).unwrap();
        assert!(!a_tok.in_tpl);
    }

    #[test]
    fn numbers_do_not_eat_operators() {
        let src = "t+12+e),t+12+e);";
        let toks = tokenize(src);
        let text: Vec<&str> = toks.iter().map(|t| &src[t.start..t.end]).collect();
        assert_eq!(
            text,
            vec!["t", "+", "12", "+", "e", ")", ",", "t", "+", "12", "+", "e", ")", ";"],
            "12+e must lex as 12, +, e"
        );
    }
}
