//! Dependency-aware extraction of the decipher function out of the player
//! IIFE — the textual port of what youtubei.js v17's `JsAnalyzer`/`JsExtractor`
//! do with an AST (see docs/gui-benchmarks/yt.md for why and what differs).
//!
//! [`build_script`] finds the combined n/sig decipher function by the same
//! structural criteria as youtubei.js's `nsigMatcher` (a ≥3-param function
//! whose body reassigns its first argument from `new X.Y(…)` and calls
//! `.set("alr","yes")`), then pulls the transitive closure of statements it
//! depends on and emits a self-contained script.

use super::lex::{tokenize, Kind, Token};
use std::collections::{HashMap, HashSet};

/// Byte span of one top-level statement inside the player IIFE.
pub struct Stmt {
    pub start: usize,
    pub end: usize,
    pub first: usize,
    pub last: usize,
}

fn is_ident(s: &str) -> bool {
    let mut ch = s.chars();
    match ch.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    ch.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn is_keyword(s: &str) -> bool {
    super::lex::KEYWORDS.contains(&s)
}

/// The biggest `{…}` pair in the file is the player IIFE body.
pub fn find_iife(toks: &[Token]) -> Option<(usize, usize)> {
    let mut stack = Vec::new();
    let mut best: Option<(usize, usize, i64)> = None;
    for (k, t) in toks.iter().enumerate() {
        match t.kind {
            Kind::Sep('{') => stack.push(k),
            Kind::Sep('}') => {
                if let Some(open) = stack.pop() {
                    let span = (t.start - toks[open].end) as i64;
                    if best.is_none_or(|(_, _, s)| span > s) {
                        best = Some((open, k, span));
                    }
                }
            }
            _ => {}
        }
    }
    best.map(|(open, close, _)| (open, close))
}

/// Split the IIFE body into statements: a boundary is a `;` recorded at the
/// same depth as the IIFE's own `{`. Interpolation tokens are skipped.
pub fn split_statements(toks: &[Token], open: usize, close: usize) -> Vec<Stmt> {
    let ref_depth = toks[open].depth;
    let mut stmts = Vec::new();
    let mut start_tok = open + 1;
    let mut started = false;
    for k in (open + 1)..close {
        let t = &toks[k];
        if t.in_tpl {
            continue;
        }
        if t.kind == Kind::Sep(';') && t.depth == ref_depth {
            stmts.push(Stmt {
                start: toks[start_tok].start,
                end: t.end,
                first: start_tok,
                last: k,
            });
            start_tok = k + 1;
            started = false;
            continue;
        }
        if !started {
            start_tok = k;
            started = true;
        }
    }
    stmts
}

/// Names (`nQ`) and member key chains (`g.vL`, `g.vL.prototype.m`) a statement
/// declares at statement level. Chains are indexed at every dotted length ≥ 2;
/// the bare base is the IIFE parameter, never a statement's declaration.
#[derive(Default)]
pub struct Decls {
    pub names: HashSet<String>,
    pub keys: HashSet<String>,
}

pub fn declared_names(toks: &[Token], stmt: &Stmt, src: &str) -> Decls {
    let mut out = Decls::default();
    let stmt_depth = toks[stmt.first].depth;
    for k in stmt.first..stmt.last {
        let t = &toks[k];
        if t.in_tpl || t.kind != Kind::Id || t.depth != stmt_depth {
            continue;
        }
        let text = &src[t.start..t.end];
        let prev = &toks[k.saturating_sub(1)];
        let next = toks.get(k + 1);
        if text == "function" {
            if let Some(n) = next.filter(|n| n.kind == Kind::Id && is_ident(&src[n.start..n.end])) {
                out.names.insert(src[n.start..n.end].to_string());
            }
            continue;
        }
        if text == "var" || text == "let" || text == "const" {
            let mut d = 0i32;
            for j in (k + 1)..stmt.last {
                let t2 = &toks[j];
                if t2.in_tpl {
                    continue;
                }
                match t2.kind {
                    Kind::Sep('(') | Kind::Sep('[') | Kind::Sep('{') => d += 1,
                    Kind::Sep(')') | Kind::Sep(']') | Kind::Sep('}') => {
                        if d == 0 {
                            break;
                        }
                        d -= 1;
                    }
                    Kind::Sep(';') if d == 0 => break,
                    Kind::Id if d == 0 && is_ident(&src[t2.start..t2.end]) => {
                        let pv = &toks[j - 1];
                        if j == k + 1 || pv.kind == Kind::Sep(',') {
                            out.names.insert(src[t2.start..t2.end].to_string());
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        let Some(n) = next else { continue };
        if n.kind == Kind::Sep('=') {
            if prev.kind != Kind::Sep('.') && prev.kind != Kind::Sep('?') {
                out.names.insert(text.to_string());
            }
            // member chain write `a.b.c=`: walk back through `.`-joined ids
            if prev.kind == Kind::Sep('.') {
                let mut chain = vec![text.to_string()];
                let mut j = k as i64 - 1;
                while j > stmt.first as i64 {
                    let t2 = &toks[j as usize];
                    let t3 = &toks[j as usize - 1];
                    if t2.kind == Kind::Sep('.') && t3.kind == Kind::Id && is_ident(&src[t3.start..t3.end]) {
                        chain.insert(0, src[t3.start..t3.end].to_string());
                        j -= 2;
                    } else {
                        break;
                    }
                }
                if chain.len() >= 2 {
                    for i in 2..=chain.len() {
                        out.keys.insert(chain[..i].join("."));
                    }
                }
            }
        }
    }
    out
}

/// Param/declarator names anywhere inside a statement — the over-approximated
/// local scope that keeps minified one-letter locals (`d`, `Q`, `t`) from
/// pulling every same-named top-level declaration in the file.
#[allow(clippy::needless_range_loop)]
pub fn local_names(toks: &[Token], stmt: &Stmt, src: &str) -> HashSet<String> {
    let mut locals = HashSet::new();
    let match_paren = |start: usize| -> Option<usize> {
        let mut d = 0i32;
        for j in start..stmt.last {
            match toks[j].kind {
                Kind::Sep('(') => d += 1,
                Kind::Sep(')') => {
                    d -= 1;
                    if d == 0 {
                        return Some(j);
                    }
                }
                _ => {}
            }
        }
        None
    };
    for k in stmt.first..stmt.last {
        let t = &toks[k];
        if t.in_tpl || t.kind != Kind::Id {
            continue;
        }
        let text = &src[t.start..t.end];
        match text {
            "var" | "let" | "const" => {
                let ref_d = t.depth;
                let mut d = 0i32;
                for j in (k + 1)..stmt.last {
                    let t2 = &toks[j];
                    if t2.in_tpl {
                        continue;
                    }
                    match t2.kind {
                        Kind::Sep('(') | Kind::Sep('[') | Kind::Sep('{') => d += 1,
                        Kind::Sep(')') | Kind::Sep(']') | Kind::Sep('}') => {
                            if d == 0 {
                                break;
                            }
                            d -= 1;
                        }
                        Kind::Sep(';') if d == 0 => break,
                        Kind::Id if d == 0 && t2.depth == ref_d && is_ident(&src[t2.start..t2.end]) => {
                            let pv = &toks[j - 1];
                            if j == k + 1 || pv.kind == Kind::Sep(',') {
                                locals.insert(src[t2.start..t2.end].to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            "function" => {
                let mut j = k + 1;
                if let Some(n) = toks.get(j) {
                    if n.kind == Kind::Id && is_ident(&src[n.start..n.end]) {
                        locals.insert(src[n.start..n.end].to_string());
                        j += 1;
                    }
                }
                if toks.get(j).is_some_and(|p| p.kind == Kind::Sep('(')) {
                    if let Some(close) = match_paren(j) {
                        collect_params(toks, j + 1, close, src, &mut locals);
                    }
                }
            }
            "constructor" => {
                if toks.get(k + 1).is_some_and(|p| p.kind == Kind::Sep('(')) {
                    if let Some(close) = match_paren(k + 1) {
                        collect_params(toks, k + 2, close, src, &mut locals);
                    }
                }
            }
            "class" => {
                if let (Some(n), Some(b)) = (toks.get(k + 1), toks.get(k + 2)) {
                    if n.kind == Kind::Id && is_ident(&src[n.start..n.end]) && b.kind == Kind::Sep('{') {
                        locals.insert(src[n.start..n.end].to_string());
                    }
                }
            }
            _ => {
                // arrow params: `(a,b) =>` — matching `)` then `=>`
                if toks.get(k + 1).is_some_and(|p| p.kind == Kind::Sep('(')) {
                    if let Some(close) = match_paren(k + 1) {
                        if let (Some(eq), Some(gt)) = (toks.get(close + 1), toks.get(close + 2)) {
                            if eq.kind == Kind::Sep('=') && gt.kind == Kind::Sep('>') {
                                collect_params(toks, k + 2, close, src, &mut locals);
                            }
                        }
                    }
                }
            }
        }
    }
    // single-param arrow `x =>` (the lexer never merges `=>`)
    #[allow(clippy::needless_range_loop)]
    if stmt.first + 2 < stmt.last {
        for k in (stmt.first + 2)..stmt.last {
            let t = &toks[k];
            if t.in_tpl || t.kind != Kind::Id {
                continue;
            }
            let (pv, pp) = (&toks[k - 1], &toks[k - 2]);
            if pv.kind == Kind::Sep('>') && pp.kind == Kind::Sep('=') {
                let ppp = &toks[k - 2];
                let _ = ppp;
                let base = &toks[k - 3];
                let text = &src[t.start..t.end];
                if is_ident(text)
                    && base.kind != Kind::Sep('.')
                    && base.kind != Kind::Id
                    && base.kind != Kind::Sep(')')
                {
                    locals.insert(text.to_string());
                }
            }
        }
    }
    locals
}

#[allow(clippy::needless_range_loop)]
fn collect_params(toks: &[Token], from: usize, to: usize, src: &str, out: &mut HashSet<String>) {
    if from == 0 || from >= to {
        return;
    }
    for k in from..to {
        let t = &toks[k];
        if t.kind == Kind::Id && is_ident(&src[t.start..t.end]) {
            let pv = &toks[k - 1];
            if k == from || pv.kind == Kind::Sep(',') {
                out.insert(src[t.start..t.end].to_string());
            }
        }
    }
}

/// Identifiers and member chains a statement uses, minus its own locals.
/// Deliberately over-inclusive: object-literal keys are skipped (ternary
/// consequents are not), and member chains rooted at `this` or a local are
/// runtime state, not references.
#[allow(clippy::needless_range_loop)]
pub fn used_names(toks: &[Token], stmt: &Stmt, src: &str) -> HashSet<String> {
    let mut used = HashSet::new();
    let locals = local_names(toks, stmt, src);
    if stmt.first + 1 >= stmt.last {
        return used;
    }
    for k in stmt.first..stmt.last {
        let t = &toks[k];
        if t.in_tpl || t.kind != Kind::Id {
            continue;
        }
        let text = &src[t.start..t.end];
        if !is_ident(text) || is_keyword(text) || text == "this" || locals.contains(text) {
            continue;
        }
        let prev = &toks[k - 1]; // stmt.first >= 1: the previous statement's `;`
        let next = toks.get(k + 1);
        // spread `...x` (≥3 dot tokens) is a use; a single `.` is property access
        let mut dots = 0;
        let mut j = k as i64 - 1;
        while j >= stmt.first as i64 && toks[j as usize].kind == Kind::Sep('.') {
            dots += 1;
            j -= 1;
        }
        if prev.kind == Kind::Sep('.') && dots < 3 {
            continue;
        }
        if let Some(n) = next {
            if n.kind == Kind::Sep(':') && (prev.kind == Kind::Sep('{') || prev.kind == Kind::Sep(',')) {
                continue; // object-literal key
            }
        }
        used.insert(text.to_string());
        // member chain
        if next.is_some_and(|n| n.kind == Kind::Sep('.')) {
            let mut chain = vec![text.to_string()];
            let mut j2 = k + 1;
            while j2 + 1 < stmt.last {
                if toks[j2].kind == Kind::Sep('.') {
                    let id = &toks[j2 + 1];
                    if id.kind == Kind::Id && is_ident(&src[id.start..id.end]) {
                        chain.push(src[id.start..id.end].to_string());
                        j2 += 2;
                        continue;
                    }
                }
                break;
            }
            if chain.len() >= 2 {
                match toks.get(j2) {
                    Some(a) if a.kind == Kind::Sep('=') => {
                        // write: only the object part is read
                        if chain.len() == 2 {
                            if !locals.contains(&chain[0]) && !is_keyword(&chain[0]) {
                                used.insert(chain[0].clone());
                            }
                        } else {
                            used.insert(chain[..chain.len() - 1].join("."));
                        }
                    }
                    _ => {
                        used.insert(chain.join("."));
                    }
                }
            }
        }
    }
    used
}

/// One `alias = target.prototype` binding. The player reuses alias names
/// sequentially; writes between an alias and its next re-binding belong to
/// that alias's target prototype.
struct AliasRegion {
    idx: usize,
    end: usize,
    alias_key: String,
    target: String,
}

/// The result of [`build_script`].
pub struct Extracted {
    pub script: String,
    pub signature_timestamp: u64,
    pub nsig_name: String,
}

/// Find the decipher function and emit the self-contained script
/// (preamble + dependency closure + export), mirroring youtubei.js's
/// `JsExtractor.buildScript` output shape.
pub fn build_script(src: &str) -> Result<Extracted, String> {
    let toks = tokenize(src);
    let (open, close) = find_iife(&toks).ok_or("no IIFE body found in player JS")?;
    let stmts = split_statements(&toks, open, close);

    // nsigMatcher, structurally: `name=function(a,b="",c="")` whose statement
    // has `=new X.Y(…)` and `.set("alr","yes")`.
    let mut nsig_stmt = None;
    let mut nsig_start = 0;
    let mut nsig_name = String::new();
    for (idx, st) in stmts.iter().enumerate() {
        let text = &src[st.start..st.end];
        if let Some((name, at)) = find_nsig_head(text) {
            nsig_stmt = Some(idx);
            nsig_start = st.start + at;
            nsig_name = name;
            break;
        }
    }
    let nsig_stmt = nsig_stmt.ok_or("decipher function (nsigMatcher criteria) not found")?;

    // name/key -> statements declaring them
    let mut decl_index: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, st) in stmts.iter().enumerate() {
        let d = declared_names(&toks, st, src);
        for n in d.names {
            decl_index.entry(n).or_default().push(idx);
        }
        for key in d.keys {
            decl_index.entry(key).or_default().push(idx);
        }
    }

    // prototype alias regions, grouped per alias key in source order
    let mut alias_regions: Vec<AliasRegion> = Vec::new();
    {
        let mut by_alias: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        for (idx, st) in stmts.iter().enumerate() {
            let text = src[st.start..st.end].trim();
            if let Some((alias, target)) = parse_alias_stmt(text) {
                by_alias.entry(alias).or_default().push((idx, target));
            }
        }
        for (alias, regions) in by_alias {
            for (i, (idx, target)) in regions.iter().enumerate() {
                let end = regions.get(i + 1).map(|(nidx, _)| *nidx).unwrap_or(stmts.len());
                alias_regions.push(AliasRegion {
                    idx: *idx,
                    end,
                    alias_key: alias.clone(),
                    target: target.clone(),
                });
            }
        }
    }

    // dependency closure
    let mut pulled: HashSet<usize> = HashSet::from([nsig_stmt]);
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();
    for u in used_names(&toks, &stmts[nsig_stmt], src) {
        seen.insert(u.clone());
        queue.push(u);
    }
    while let Some(nm) = queue.pop() {
        let mut newly_pulled: Vec<usize> = Vec::new();
        if let Some(idxs) = decl_index.get(&nm) {
            for &idx in idxs {
                if pulled.insert(idx) {
                    newly_pulled.push(idx);
                }
            }
        }
        if nm.contains('.') {
            let dot_prefix = format!("{nm}.");
            for (key, idxs) in &decl_index {
                // at, below, or above `nm`: reading `a.b.c` needs `a.b` to
                // exist, and writes under `a.b.c.d` extend the same object
                let related = key == &nm
                    || key.starts_with(&dot_prefix)
                    || nm.starts_with(&format!("{key}."));
                if related {
                    for &idx in idxs {
                        if pulled.insert(idx) {
                            newly_pulled.push(idx);
                        }
                    }
                }
            }
        }
        // prototype alias regions for this target
        for region in &alias_regions {
            let t = &region.target;
            if nm == *t || nm == format!("{t}.prototype") || nm.starts_with(&format!("{t}.")) {
                if pulled.insert(region.idx) {
                    newly_pulled.push(region.idx);
                }
                let alias_prefix = format!("{}.", region.alias_key);
                #[allow(clippy::needless_range_loop)]
                for idx in (region.idx + 1)..region.end {
                    let d = declared_names(&toks, &stmts[idx], src);
                    let hit = d.keys.iter().any(|key| key == &region.alias_key || key.starts_with(&alias_prefix));
                    if hit && pulled.insert(idx) {
                        newly_pulled.push(idx);
                    }
                }
            }
        }
        for idx in newly_pulled {
            for u in used_names(&toks, &stmts[idx], src) {
                if seen.insert(u.clone()) {
                    queue.push(u);
                }
            }
        }
    }

    // emit in original order
    let mut ordered: Vec<usize> = pulled.into_iter().collect();
    ordered.sort_unstable();
    let mut body = String::with_capacity(src.len() / 4);
    for idx in ordered {
        let st = &stmts[idx];
        let start = if idx == nsig_stmt { nsig_start } else { st.start };
        let text = src[start..st.end].trim_start();
        // `var window=this` collides with the preamble's `window` binding;
        // youtubei.js's scope analysis never pulls it. Drop it the same way.
        if text == "var window=this;" {
            continue;
        }
        body.push_str(text);
        body.push('\n');
    }

    // signatureTimestamp — youtubei.js's timestampMatcher exports the raw value
    let signature_timestamp = src
        .split("signatureTimestamp:")
        .nth(1)
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
        .unwrap_or(0);

    Ok(Extracted { script: body, signature_timestamp, nsig_name })
}

/// `name=function(a,b="",c="")` at statement start (or after a merged `;`/`}`
/// from a preceding block statement), returning the name and match offset.
fn find_nsig_head(text: &str) -> Option<(String, usize)> {
    let mut from = 0;
    loop {
        let at = from + text[from..].find("=function(")?;
        // walk back over the assigned name
        let mut s = at;
        let b = text.as_bytes();
        while s > 0 && b[s - 1] != b';' && b[s - 1] != b'}' && b[s - 1] != b'\n' {
            s -= 1;
        }
        let name = text[s..at].trim();
        if is_ident(name) && name.len() == at - s {
            let head_end = at + text[at..].find('{')?;
            let head = &text[at..head_end];
            if head.matches(',').count() >= 2
                && head.contains("=\"\"")
                && head.contains("=\"\"")
                && text.contains("=new ")
                && text.contains(".set(\"alr\",\"yes\")")
            {
                return Some((name.to_string(), s));
            }
        }
        from = at + 1;
    }
}

/// `A.B=C.prototype;` — returns (alias key, target).
fn parse_alias_stmt(text: &str) -> Option<(String, String)> {
    let text = text.strip_suffix(';').unwrap_or(text).trim();
    let (lhs, rhs) = text.split_once('=')?;
    if !rhs.ends_with(".prototype") || !lhs.contains('.') {
        return None;
    }
    let target = &rhs[..rhs.len() - ".prototype".len()];
    if target.is_empty() || target.starts_with('.') || target.ends_with('.') {
        return None;
    }
    if target.split('.').all(is_ident) && !lhs.is_empty() {
        return Some((lhs.to_string(), target.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_statement_parses() {
        assert_eq!(parse_alias_stmt("g.J=yq.prototype;"), Some(("g.J".into(), "yq".into())));
        assert_eq!(parse_alias_stmt("g.J=g.No.prototype;"), Some(("g.J".into(), "g.No".into())));
        assert_eq!(parse_alias_stmt("g.J=yq.prototype"), Some(("g.J".into(), "yq".into())));
        assert_eq!(parse_alias_stmt("g.J=x("), None);
    }

    #[test]
    fn nsig_head_matches_the_real_shape() {
        let text = r#"x=1;nQ=function(d,Q="",t=""){d=new g.vL(d,!0);d.set("alr","yes");return d};"#;
        let (name, at) = find_nsig_head(text).expect("matches");
        assert_eq!(name, "nQ");
        assert_eq!(&text[at..at + 2], "nQ");
        assert!(find_nsig_head(r#"x=function(a){return a};"#).is_none());
        assert!(find_nsig_head(r#"y=function(a,b){return a};"#).is_none());
    }
}
