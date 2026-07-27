//! Expression evaluation for `arithmetic` and `conditional`.
//!
//! A hand-written Pratt parser rather than a crate, because the target is
//! *govaluate* semantics: `<>` as not-equal, implicit string↔number coercion,
//! `+` doubling as concatenation, and the three helper functions the Dipper
//! chains use (`strlen`, `sFromObj`, `nFromObj`).
//!
//! Identifiers are paths into the view produced by [`crate::daq::view`], so
//! `user.name` and `list[0]` work without any special syntax.

use serde_json::{json, Value};

use crate::daq;

pub type EvalResult = Result<Value, String>;

pub fn eval(expr: &str, view: &Value) -> EvalResult {
    eval_ast(&parse(expr)?, view)
}

/// Guard rails against user-supplied expressions. Expressions arrive over REST
/// (chain validation), and an unbounded recursive-descent parser aborts the
/// WHOLE process on stack overflow — a `SIGSEGV` that `catch_unwind` cannot
/// intercept — so a single crafted payload kills the daemon.
///
/// `MAX_TOKENS` caps the total size, which bounds *left-deep* chains like
/// `1+1+1+…` (built in a loop, so the parser stays shallow, but their AST depth
/// equals their length and would overflow on `Drop`/eval). `MAX_DEPTH` caps
/// nesting from `(` / unary `-`, which recurse directly.
const MAX_TOKENS: usize = 4096;
const MAX_DEPTH: usize = 256;
/// Bounds the recursion in `eval_ast` itself. Nesting depth is already capped by
/// the parser; this only catches left-deep chains (whose eval recurses down the
/// left spine) before they can overflow the stack.
const MAX_EVAL_DEPTH: usize = 1024;

/// Parse without data. Lets `validate` reject a typo at save time instead of
/// at 3am, and keeps short-circuit honest: nothing is evaluated while parsing.
pub fn parse(expr: &str) -> Result<Ast, String> {
    let tokens = lex(expr)?;
    if tokens.len() > MAX_TOKENS {
        return Err(format!(
            "biểu thức quá dài ({} token, tối đa {})",
            tokens.len(),
            MAX_TOKENS
        ));
    }
    let mut p = Parser { tokens, pos: 0 };
    let ast = p.parse_expr(0, 0)?;
    if p.pos < p.tokens.len() {
        return Err(format!(
            "thừa ký tự sau biểu thức: `{}`",
            p.tokens[p.pos].text()
        ));
    }
    Ok(ast)
}

/// Strict on purpose: only a real boolean routes a branch. A number quietly
/// becoming `true` is how the Go engine sent messages down the wrong path.
pub fn eval_bool(expr: &str, view: &Value) -> Result<bool, String> {
    match eval(expr, view)? {
        Value::Bool(b) => Ok(b),
        Value::String(s) if s.eq_ignore_ascii_case("true") => Ok(true),
        Value::String(s) if s.eq_ignore_ascii_case("false") => Ok(false),
        v => Err(format!(
            "biểu thức `{expr}` không trả về true/false (nhận `{v}`)"
        )),
    }
}

pub fn eval_f64(expr: &str, view: &Value) -> Result<f64, String> {
    let v = eval(expr, view)?;
    as_f64(&v).ok_or_else(|| format!("biểu thức `{expr}` không trả về số (nhận `{v}`)"))
}

// ------------------------------------------------------------------- lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(String),
    LParen,
    RParen,
    Comma,
}

impl Tok {
    fn text(&self) -> String {
        match self {
            Tok::Num(n) => n.to_string(),
            Tok::Str(s) => format!("'{s}'"),
            Tok::Ident(s) => s.clone(),
            Tok::Op(s) => s.clone(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::Comma => ",".into(),
        }
    }
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        match ch {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '\'' | '"' => {
                let quote = ch;
                let mut s = String::new();
                i += 1;
                while i < c.len() && c[i] != quote {
                    if c[i] == '\\' && i + 1 < c.len() {
                        i += 1;
                        s.push(match c[i] {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        s.push(c[i]);
                    }
                    i += 1;
                }
                if i >= c.len() {
                    return Err("chuỗi chưa đóng nháy".to_string());
                }
                i += 1;
                out.push(Tok::Str(s));
            }
            '0'..='9' => {
                let start = i;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                // Scientific notation: `1e5`, `1.5e-3`, `2E10`. Only consume the
                // `e`/`E` when a real exponent (optional sign + digits) follows,
                // so we never swallow the `e` of a following identifier.
                if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                    let mut j = i + 1;
                    if j < c.len() && (c[j] == '+' || c[j] == '-') {
                        j += 1;
                    }
                    if j < c.len() && c[j].is_ascii_digit() {
                        i = j;
                        while i < c.len() && c[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text: String = c[start..i].iter().collect();
                let n = text
                    .parse::<f64>()
                    .map_err(|_| format!("số không hợp lệ: `{text}`"))?;
                out.push(Tok::Num(n));
            }
            _ if ch.is_alphabetic() || ch == '_' || ch == '$' => {
                let start = i;
                while i < c.len()
                    && (c[i].is_alphanumeric()
                        || c[i] == '_'
                        || c[i] == '.'
                        || c[i] == '$'
                        || c[i] == '['
                        || c[i] == ']')
                {
                    i += 1;
                }
                let text: String = c[start..i].iter().collect();
                out.push(Tok::Ident(text));
            }
            _ => {
                // Match operators directly on the char slice. The old code did
                // `c[i..].iter().collect::<String>()` per operator char, which
                // is O(n²) — a ~320KB expression froze a worker for ~17s.
                //
                // Longest operator first so `<=` never lexes as `<` then `=`.
                const OPS2: [[char; 2]; 8] = [
                    ['=', '='],
                    ['!', '='],
                    ['<', '>'],
                    ['>', '='],
                    ['<', '='],
                    ['&', '&'],
                    ['|', '|'],
                    ['*', '*'],
                ];
                const OPS1: [char; 11] = ['+', '-', '*', '/', '%', '<', '>', '!', '?', ':', '='];
                let two = if i + 1 < c.len() {
                    OPS2.iter().find(|op| op[0] == c[i] && op[1] == c[i + 1])
                } else {
                    None
                };
                if let Some(op) = two {
                    out.push(Tok::Op(op.iter().collect()));
                    i += 2;
                } else if OPS1.contains(&ch) {
                    out.push(Tok::Op(ch.to_string()));
                    i += 1;
                } else {
                    return Err(format!("ký tự lạ trong biểu thức: `{ch}`"));
                }
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ parser

/// Parsed expression. Evaluation walks this, so `&&` / `||` / `? :` can skip
/// the side they do not need.
#[derive(Debug, Clone)]
pub enum Ast {
    Lit(Value),
    Path(String),
    Neg(Box<Ast>),
    Not(Box<Ast>),
    Bin(String, Box<Ast>, Box<Ast>),
    Ternary(Box<Ast>, Box<Ast>, Box<Ast>),
    Call(String, Vec<Ast>),
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

/// Binding powers, loosest first. Mirrors govaluate's precedence table.
fn binding_power(op: &str) -> Option<u8> {
    Some(match op {
        "?" => 1,
        "||" => 2,
        "&&" => 3,
        "==" | "!=" | "<>" => 4,
        "<" | ">" | "<=" | ">=" => 5,
        "+" | "-" => 6,
        "*" | "/" | "%" => 7,
        "**" => 8,
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn parse_expr(&mut self, min_bp: u8, depth: usize) -> Result<Ast, String> {
        if depth > MAX_DEPTH {
            return Err("biểu thức lồng quá sâu".to_string());
        }
        let mut lhs = self.parse_unary(depth + 1)?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(op)) => op.clone(),
                _ => break,
            };
            let Some(bp) = binding_power(&op) else { break };
            if bp < min_bp {
                break;
            }
            self.pos += 1;

            if op == "?" {
                let then = self.parse_expr(0, depth + 1)?;
                match self.peek() {
                    Some(Tok::Op(c)) if c == ":" => self.pos += 1,
                    _ => return Err("thiếu `:` trong biểu thức ba ngôi".to_string()),
                }
                let other = self.parse_expr(0, depth + 1)?;
                lhs = Ast::Ternary(Box::new(lhs), Box::new(then), Box::new(other));
                continue;
            }

            // `**` is right-associative: 2**3**2 == 2**(3**2).
            let next_min = if op == "**" { bp } else { bp + 1 };
            let rhs = self.parse_expr(next_min, depth + 1)?;
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self, depth: usize) -> Result<Ast, String> {
        if depth > MAX_DEPTH {
            return Err("biểu thức lồng quá sâu".to_string());
        }
        match self.peek() {
            Some(Tok::Op(op)) if op == "-" => {
                self.pos += 1;
                Ok(Ast::Neg(Box::new(self.parse_unary(depth + 1)?)))
            }
            Some(Tok::Op(op)) if op == "!" => {
                self.pos += 1;
                Ok(Ast::Not(Box::new(self.parse_unary(depth + 1)?)))
            }
            _ => self.parse_atom(depth + 1),
        }
    }

    fn parse_atom(&mut self, depth: usize) -> Result<Ast, String> {
        if depth > MAX_DEPTH {
            return Err("biểu thức lồng quá sâu".to_string());
        }
        let tok = self
            .peek()
            .cloned()
            .ok_or_else(|| "biểu thức kết thúc đột ngột".to_string())?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(Ast::Lit(num(n))),
            Tok::Str(s) => Ok(Ast::Lit(Value::String(s))),
            Tok::LParen => {
                let v = self.parse_expr(0, depth + 1)?;
                match self.peek() {
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        Ok(v)
                    }
                    _ => Err("thiếu dấu `)`".to_string()),
                }
            }
            Tok::Ident(name) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.parse_expr(0, depth + 1)?);
                            match self.peek() {
                                Some(Tok::Comma) => self.pos += 1,
                                _ => break,
                            }
                        }
                    }
                    match self.peek() {
                        Some(Tok::RParen) => self.pos += 1,
                        _ => return Err(format!("thiếu `)` sau hàm `{name}`")),
                    }
                    if !is_known_fn(&name) {
                        return Err(format!("không có hàm `{name}`"));
                    }
                    return Ok(Ast::Call(name, args));
                }
                Ok(match name.as_str() {
                    "true" => Ast::Lit(json!(true)),
                    "false" => Ast::Lit(json!(false)),
                    "null" | "nil" => Ast::Lit(Value::Null),
                    _ => Ast::Path(name),
                })
            }
            other => Err(format!("không mong đợi `{}`", other.text())),
        }
    }
}

pub fn eval_ast(ast: &Ast, view: &Value) -> EvalResult {
    eval_ast_at(ast, view, 0)
}

fn eval_ast_at(ast: &Ast, view: &Value, depth: usize) -> EvalResult {
    // A left-deep `1+1+…` recurses down its left spine here; cap it so a huge
    // (but validly parsed) chain errors instead of overflowing the stack.
    if depth > MAX_EVAL_DEPTH {
        return Err("biểu thức lồng quá sâu khi tính".to_string());
    }
    let d = depth + 1;
    match ast {
        Ast::Lit(v) => Ok(v.clone()),
        Ast::Path(p) => Ok(daq::get(view, p).unwrap_or(Value::Null)),
        Ast::Neg(inner) => {
            let v = eval_ast_at(inner, view, d)?;
            let n = as_f64(&v).ok_or_else(|| format!("không đổi `{v}` thành số để đảo dấu"))?;
            Ok(num(-n))
        }
        Ast::Not(inner) => {
            let v = eval_ast_at(inner, view, d)?;
            Ok(json!(!as_bool(&v).unwrap_or(false)))
        }
        Ast::Ternary(cond, then, other) => {
            let c = eval_ast_at(cond, view, d)?;
            if as_bool(&c).unwrap_or(false) {
                eval_ast_at(then, view, d)
            } else {
                eval_ast_at(other, view, d)
            }
        }
        Ast::Bin(op, l, r) => match op.as_str() {
            "&&" => {
                if !as_bool(&eval_ast_at(l, view, d)?).unwrap_or(false) {
                    return Ok(json!(false));
                }
                Ok(json!(as_bool(&eval_ast_at(r, view, d)?).unwrap_or(false)))
            }
            "||" => {
                if as_bool(&eval_ast_at(l, view, d)?).unwrap_or(false) {
                    return Ok(json!(true));
                }
                Ok(json!(as_bool(&eval_ast_at(r, view, d)?).unwrap_or(false)))
            }
            _ => apply(op, &eval_ast_at(l, view, d)?, &eval_ast_at(r, view, d)?),
        },
        Ast::Call(name, args) => {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(eval_ast_at(a, view, d)?);
            }
            call(name, &vals)
        }
    }
}

// -------------------------------------------------------------- operators

fn apply(op: &str, a: &Value, b: &Value) -> EvalResult {
    match op {
        "+" => {
            // govaluate: string + string ALWAYS concatenates (`'2' + '3'` == "23");
            // `+` only adds when both operands are real numbers. We keep one
            // looser case the Dipper chains rely on — when exactly one side is a
            // string and both look numeric, `"2" + 3` stays arithmetic (== 5).
            match (a.is_string(), b.is_string()) {
                (true, true) => Ok(Value::String(format!("{}{}", to_str(a), to_str(b)))),
                (false, false) => arith(op, a, b),
                _ => {
                    if let (Some(_), Some(_)) = (as_f64(a), as_f64(b)) {
                        arith(op, a, b)
                    } else {
                        Ok(Value::String(format!("{}{}", to_str(a), to_str(b))))
                    }
                }
            }
        }
        "-" | "*" | "/" | "%" | "**" => arith(op, a, b),
        "==" => Ok(json!(loose_eq(a, b))),
        "!=" | "<>" => Ok(json!(!loose_eq(a, b))),
        "<" | ">" | "<=" | ">=" => {
            let ord = compare(a, b)
                .ok_or_else(|| format!("không so sánh được `{}` với `{}`", to_str(a), to_str(b)))?;
            Ok(json!(match op {
                "<" => ord < 0,
                ">" => ord > 0,
                "<=" => ord <= 0,
                _ => ord >= 0,
            }))
        }
        "&&" => Ok(json!(
            as_bool(a).unwrap_or(false) && as_bool(b).unwrap_or(false)
        )),
        "||" => Ok(json!(
            as_bool(a).unwrap_or(false) || as_bool(b).unwrap_or(false)
        )),
        other => Err(format!("toán tử không hỗ trợ: `{other}`")),
    }
}

fn arith(op: &str, a: &Value, b: &Value) -> EvalResult {
    let x = as_f64(a).ok_or_else(|| format!("`{}` không phải số", to_str(a)))?;
    let y = as_f64(b).ok_or_else(|| format!("`{}` không phải số", to_str(b)))?;
    let r = match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => {
            if y == 0.0 {
                return Err("chia cho 0".to_string());
            }
            x / y
        }
        "%" => {
            if y == 0.0 {
                return Err("chia lấy dư cho 0".to_string());
            }
            x % y
        }
        "**" => x.powf(y),
        _ => return Err(format!("toán tử số không hỗ trợ: `{op}`")),
    };
    // Overflow / NaN must surface as an error at the node that produced it,
    // rather than `num()` silently writing `null` into the payload.
    if !r.is_finite() {
        return Err("kết quả tràn số / không xác định".to_string());
    }
    Ok(num(r))
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        return x == y;
    }
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::String(_), _) | (_, Value::String(_)) => to_str(a) == to_str(b),
        _ => false,
    }
}

/// -1 / 0 / 1, or `None` when the two are not comparable.
fn compare(a: &Value, b: &Value) -> Option<i32> {
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        // `partial_cmp` yields `None` for NaN, so `<`/`>`/`<=`/`>=` surface a
        // "không so sánh được" error instead of treating NaN as equal to every
        // number (which used to make `x <= 100` and `x >= 100` both true).
        return x.partial_cmp(&y).map(|o| match o {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Equal => 0,
        });
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => Some(match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        _ => None,
    }
}

// -------------------------------------------------------------- functions

const FUNCTIONS: &[&str] = &[
    "strlen",
    "sFromObj",
    "nFromObj",
    "len",
    "abs",
    "round",
    "floor",
    "ceil",
    "min",
    "max",
    "lower",
    "upper",
    "trim",
    "contains",
    "startsWith",
    "endsWith",
    "str",
    "num",
    "float",
    "int",
    "bool",
    "isNull",
    "coalesce",
    "now",
    "nowMs",
];

fn is_known_fn(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

fn call(name: &str, args: &[Value]) -> EvalResult {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let need = |i: usize| -> Result<&Value, String> {
        args.get(i)
            .ok_or_else(|| format!("hàm `{name}` thiếu tham số thứ {}", i + 1))
    };
    match name {
        // The three govaluate extensions the Dipper chains rely on.
        "strlen" => Ok(num(to_str(need(0)?).chars().count() as f64)),
        "sFromObj" => {
            let path = to_str(need(1)?);
            Ok(Value::String(
                daq::get(need(0)?, &path)
                    .map(|v| to_str(&v))
                    .unwrap_or_default(),
            ))
        }
        "nFromObj" => {
            let path = to_str(need(1)?);
            Ok(num(daq::get_f64(need(0)?, &path).unwrap_or(0.0)))
        }

        "len" => Ok(num(match need(0)? {
            Value::Array(a) => a.len() as f64,
            Value::Object(o) => o.len() as f64,
            other => to_str(other).chars().count() as f64,
        })),
        "abs" => Ok(num(as_f64(need(0)?).unwrap_or(0.0).abs())),
        "round" => {
            let n = as_f64(need(0)?).unwrap_or(0.0);
            let digits = args.get(1).and_then(as_f64).unwrap_or(0.0);
            let f = 10f64.powf(digits);
            Ok(num((n * f).round() / f))
        }
        "floor" => Ok(num(as_f64(need(0)?).unwrap_or(0.0).floor())),
        "ceil" => Ok(num(as_f64(need(0)?).unwrap_or(0.0).ceil())),
        "min" | "max" => {
            let mut nums: Vec<f64> = args.iter().filter_map(as_f64).collect();
            if let Some(Value::Array(a)) = args.first() {
                nums = a.iter().filter_map(as_f64).collect();
            }
            if nums.is_empty() {
                return Err(format!("hàm `{name}` cần ít nhất một số"));
            }
            let v = if name == "min" {
                nums.iter().cloned().fold(f64::INFINITY, f64::min)
            } else {
                nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            };
            Ok(num(v))
        }
        "lower" => Ok(Value::String(to_str(need(0)?).to_lowercase())),
        "upper" => Ok(Value::String(to_str(need(0)?).to_uppercase())),
        "trim" => Ok(Value::String(to_str(need(0)?).trim().to_string())),
        "contains" => Ok(json!(to_str(need(0)?).contains(&to_str(need(1)?)))),
        "startsWith" => Ok(json!(to_str(need(0)?).starts_with(&to_str(need(1)?)))),
        "endsWith" => Ok(json!(to_str(need(0)?).ends_with(&to_str(need(1)?)))),
        "str" => Ok(Value::String(to_str(need(0)?))),
        "num" | "float" => Ok(num(as_f64(need(0)?).unwrap_or(0.0))),
        "int" => Ok(num(as_f64(need(0)?).unwrap_or(0.0).trunc())),
        "bool" => Ok(json!(as_bool(need(0)?).unwrap_or(false))),
        "isNull" => Ok(json!(arg(0).is_null())),
        "coalesce" => Ok(args
            .iter()
            .find(|v| !v.is_null())
            .cloned()
            .unwrap_or(Value::Null)),
        "now" => Ok(num(crate::engine::types::now_ms() as f64 / 1000.0)),
        "nowMs" => Ok(num(crate::engine::types::now_ms() as f64)),
        other => Err(format!("không có hàm `{other}`")),
    }
}

// ------------------------------------------------------------- coercions

fn num(n: f64) -> Value {
    serde_json::Number::from_f64(n)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" | "" => Some(false),
            _ => None,
        },
        Value::Null => Some(false),
        _ => None,
    }
}

fn to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v() -> Value {
        json!({ "a": 10, "b": 20, "d": 5, "x": 3, "name": "kho A",
                "ac": { "a": 4 }, "list": [1, 2, 3] })
    }

    /// The oracle from the Go `math_test.go` table, which is the only place the
    /// original expression behaviour was pinned down.
    #[test]
    fn go_arithmetic_oracle() {
        let view = v();
        assert_eq!(eval_f64("a+b", &view).unwrap(), 30.0);
        assert_eq!(eval_f64("a-b", &view).unwrap(), -10.0);
        assert_eq!(eval_f64("a*b", &view).unwrap(), 200.0);
        assert_eq!(eval_f64("a+10", &view).unwrap(), 20.0);
        assert_eq!(eval_f64("(a+b)*(a+x)", &view).unwrap(), 390.0);
        assert_eq!(eval_f64("nFromObj(ac,'a')+b", &view).unwrap(), 24.0);
    }

    #[test]
    fn precedence_and_parens() {
        let view = v();
        assert_eq!(eval_f64("2+3*4", &view).unwrap(), 14.0);
        assert_eq!(eval_f64("(2+3)*4", &view).unwrap(), 20.0);
        assert_eq!(eval_f64("2**3**2", &view).unwrap(), 512.0, "right assoc");
        assert_eq!(eval_f64("-a+2", &view).unwrap(), -8.0);
    }

    #[test]
    fn govaluate_not_equal_spelling() {
        let view = v();
        assert!(eval_bool("a <> b", &view).unwrap());
        assert!(eval_bool("a != b", &view).unwrap());
        assert!(!eval_bool("a <> 10", &view).unwrap());
    }

    #[test]
    fn comparisons_and_logic() {
        let view = v();
        assert!(eval_bool("a > 5 && b < 100", &view).unwrap());
        assert!(!eval_bool("a > 50 || b > 100", &view).unwrap());
        assert!(eval_bool("!(a > 50)", &view).unwrap());
        assert!(eval_bool("a >= 10 && a <= 10", &view).unwrap());
    }

    #[test]
    fn short_circuit_does_not_evaluate_the_bad_side() {
        let view = v();
        // `missing.deep / 0` would error if it were evaluated.
        assert!(!eval_bool("false && (1/0 > 0)", &view).unwrap());
        assert!(eval_bool("true || (1/0 > 0)", &view).unwrap());
    }

    #[test]
    fn ternary_picks_a_branch() {
        let view = v();
        assert_eq!(eval("a > 5 ? 'cao' : 'thấp'", &view).unwrap(), json!("cao"));
        assert_eq!(
            eval("a > 50 ? 'cao' : 'thấp'", &view).unwrap(),
            json!("thấp")
        );
    }

    #[test]
    fn paths_and_indices_resolve_against_the_view() {
        let view = v();
        assert_eq!(eval_f64("ac.a", &view).unwrap(), 4.0);
        assert_eq!(eval_f64("list[2]", &view).unwrap(), 3.0);
        assert_eq!(eval("missing", &view).unwrap(), Value::Null);
    }

    #[test]
    fn plus_concatenates_when_a_side_is_a_non_numeric_string() {
        let view = v();
        assert_eq!(eval("name + ' nóng'", &view).unwrap(), json!("kho A nóng"));
        // One string + a number still adds up when both look numeric, like govaluate.
        assert_eq!(eval_f64("'2' + 3", &view).unwrap(), 5.0);
    }

    #[test]
    fn plus_concatenates_two_numeric_strings() {
        // govaluate: string + string is ALWAYS concatenation, never addition.
        let view = v();
        assert_eq!(eval("'2' + '3'", &view).unwrap(), json!("23"));
        assert_eq!(eval("'007' + '1'", &view).unwrap(), json!("0071"));
        assert_eq!(eval("'2026' + '01'", &view).unwrap(), json!("202601"));
    }

    #[test]
    fn deeply_nested_input_errors_instead_of_crashing() {
        // 100000 `(` must return Err, not abort the process on stack overflow.
        let bomb = "(".repeat(100_000);
        assert!(parse(&bomb).is_err());
        // Nesting past the depth cap is rejected even under the token limit.
        let nested = format!("{}1{}", "(".repeat(400), ")".repeat(400));
        assert!(parse(&nested).is_err());
        // A long unary chain also recurses; it must not overflow either.
        let unary = format!("{}1", "-".repeat(100_000));
        assert!(parse(&unary).is_err());
    }

    #[test]
    fn long_expression_lexes_quickly_without_quadratic_blowup() {
        // ~20k operators: the old O(n²) lexer would hang here.
        let expr = "1".to_string() + &"+1".repeat(10_000);
        // Either parses (under the token cap) or is rejected for length — the
        // point is that lexing returns promptly rather than freezing a worker.
        let _ = parse(&expr);
    }

    #[test]
    fn nan_is_not_comparable() {
        let view = json!({ "n": "NaN" });
        // NaN must not silently satisfy ordering comparisons.
        assert!(eval("n <= 100", &view).is_err());
        assert!(eval("n >= 100", &view).is_err());
        assert!(eval("n > 100", &view).is_err());
        // Equality stays well-defined: NaN equals nothing.
        assert!(!eval_bool("n == 100", &view).unwrap());
    }

    #[test]
    fn scientific_notation_is_accepted() {
        let view = v();
        assert_eq!(eval_f64("1e5", &view).unwrap(), 100000.0);
        assert_eq!(eval_f64("1.5e-3", &view).unwrap(), 0.0015);
        assert_eq!(eval_f64("2E10", &view).unwrap(), 2e10);
        assert_eq!(eval_f64("1e3 + 1", &view).unwrap(), 1001.0);
    }

    #[test]
    fn numeric_overflow_is_an_error_not_a_silent_null() {
        let view = json!({ "a": 1e308 });
        assert!(eval("a * a", &view).is_err(), "overflow phải là lỗi");
        assert!(eval("a * 10", &view).is_err());
        // An infinite string operand also errors rather than yielding null.
        assert!(eval("'inf' + 1", &view).is_err());
    }

    #[test]
    fn helper_functions() {
        let view = v();
        assert_eq!(eval_f64("strlen(name)", &view).unwrap(), 5.0);
        assert_eq!(eval("sFromObj(ac,'a')", &view).unwrap(), json!("4"));
        assert_eq!(eval_f64("len(list)", &view).unwrap(), 3.0);
        assert_eq!(eval_f64("round(3.14159, 2)", &view).unwrap(), 3.14);
        assert_eq!(eval_f64("max(1, 9, 4)", &view).unwrap(), 9.0);
        assert!(eval_bool("contains(name, 'kho')", &view).unwrap());
        assert_eq!(eval("upper(name)", &view).unwrap(), json!("KHO A"));
    }

    #[test]
    fn errors_are_reported_not_swallowed() {
        let view = v();
        assert!(eval("a +", &view).is_err());
        assert!(eval("(a + b", &view).is_err());
        assert!(eval("nope(1)", &view).is_err());
        assert!(eval_f64("a / 0", &view).is_err());
        assert!(eval_bool("a + b", &view).is_err(), "số không phải bool");
        assert!(eval("'chưa đóng", &view).is_err());
    }

    #[test]
    fn eval_bool_refuses_to_guess_at_non_booleans() {
        let view = json!({ "z": 0, "s": "", "n": null, "flag": true });
        assert!(eval_bool("z", &view).is_err(), "số không tự thành bool");
        assert!(eval_bool("s", &view).is_err());
        assert!(eval_bool("n", &view).is_err());
        assert!(eval_bool("flag", &view).unwrap());
        assert!(!eval_bool("z == 1", &view).unwrap());
        // The Go Query.Number() cached wrongly on 0 — make sure 0 reads back.
        assert_eq!(eval_f64("z", &view).unwrap(), 0.0);
    }

    #[test]
    fn falsy_coercion_still_applies_inside_and_or() {
        let view = json!({ "z": 0, "one": 1 });
        assert!(!eval_bool("z && true", &view).unwrap());
        assert!(eval_bool("one || false", &view).unwrap());
        assert_eq!(eval("z ? 'a' : 'b'", &view).unwrap(), json!("b"));
    }

    #[test]
    fn parse_catches_typos_without_any_data() {
        assert!(parse("a >").is_err());
        assert!(parse("nope(1)").is_err());
        assert!(parse("a > 3 && b == 'x'").is_ok());
    }

    #[test]
    fn meta_data_is_reachable_from_an_expression() {
        let view = crate::daq::view(&json!({ "t": 1 }), &json!({ "device_id": "d1" }));
        assert_eq!(
            eval("sFromObj(meta_data, 'device_id')", &view).unwrap(),
            json!("d1")
        );
    }
}
