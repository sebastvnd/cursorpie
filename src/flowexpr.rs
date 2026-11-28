//! The expressions of a flow: `{{= (price * qty) + 5 > limit }}`.
//!
//! A text of a flow may hold `{{name}}` to read a variable. A text that holds `{{= ... }}` holds an
//! **expression** instead: it is read into a syntax tree ([`Expr`]), worked out with the variables
//! of the run and replaced by its value. It works in every field that fills in variables: the value
//! of a *Set* block, either side of the condition of a *Branch* or a *While*, a message, a command.
//!
//! ```text
//! {{= (a + b) * 2 }}               arithmetic: + - * / %  (numbers)
//! {{= n >= 3 && name != "" }}      comparisons and && || !  (true is 1, false is 0)
//! {{= n > 0 ? "some" : "none" }}  a choice
//! {{= max(a, min(b, 10)) }}        functions, see [`FUNCTIONS`]
//! ```
//!
//! The rules are the ones of the blocks, so an expression and the equivalent blocks agree:
//!
//! - A variable that does not exist is empty text. Text that is a number counts as a number.
//! - Two numbers are compared as numbers, anything else as text ([`compare`]).
//! - A calculation that cannot be done (a text that is not a number, a division by zero, a result
//!   too large) makes the whole expression **empty**, like a *Calc* block does; an empty value is
//!   false.
//! - An expression with a **syntax error** is left as it is written, like a `{{name}}` of a
//!   variable that does not exist.
//!
//! The reading is bounded ([`MAX_EXPR_CHARS`], and how deep it may nest), so a file from anyone
//! cannot make it run long or recurse far. There is no assignment and no loop in an expression:
//! those are blocks.

use crate::config::{CmpOp, MAX_VALUE_CHARS};
use crate::flowrun::{compare, format_number, is_truthy, parse_number};

/// The most characters of an expression that are read.
pub const MAX_EXPR_CHARS: usize = 1_000;

/// How deep brackets, choices and prefix operators may nest.
const MAX_DEPTH: usize = 40;

/// The most arguments a function takes.
const MAX_ARGS: usize = 8;

/// The functions of an expression: (name, fewest arguments, most arguments, what it does).
pub const FUNCTIONS: [(&str, usize, usize, &str); 19] = [
    ("abs", 1, 1, "the number without its sign"),
    ("round", 1, 2, "round(x) or round(x, digits)"),
    ("floor", 1, 1, "the whole number below"),
    ("ceil", 1, 1, "the whole number above"),
    ("sqrt", 1, 1, "the square root"),
    ("pow", 2, 2, "pow(base, exponent)"),
    ("min", 2, MAX_ARGS, "the smallest number"),
    ("max", 2, MAX_ARGS, "the largest number"),
    ("len", 1, 1, "the number of characters"),
    ("upper", 1, 1, "the text in capitals"),
    ("lower", 1, 1, "the text in small letters"),
    ("trim", 1, 1, "the text without spaces at both ends"),
    ("concat", 1, MAX_ARGS, "the texts one after the other"),
    ("contains", 2, 2, "1 when the first text holds the second"),
    (
        "starts",
        2,
        2,
        "1 when the first text begins with the second",
    ),
    ("ends", 2, 2, "1 when the first text ends with the second"),
    ("empty", 1, 1, "1 when the text is empty or only spaces"),
    ("if", 3, 3, "if(condition, then, else); both are worked out"),
    ("not", 1, 1, "1 for false, 0 for true"),
];

/// The two sides of a binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    fn from_symbol(s: &str) -> Option<BinOp> {
        Some(match s {
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "/" => BinOp::Div,
            "%" => BinOp::Rem,
            "==" => BinOp::Eq,
            "!=" => BinOp::Ne,
            "<" => BinOp::Lt,
            "<=" => BinOp::Le,
            ">" => BinOp::Gt,
            ">=" => BinOp::Ge,
            "&&" => BinOp::And,
            "||" => BinOp::Or,
            _ => return None,
        })
    }

    /// How the operator is written.
    pub const fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

/// A syntax tree of an expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A number: `3`, `0.5`, `true` (1) and `false` (0).
    Num(f64),
    /// A text between quotes: `"yes"` or `'yes'`.
    Text(String),
    /// A variable.
    Var(String),
    /// `-x`
    Neg(Box<Expr>),
    /// `!x`
    Not(Box<Expr>),
    /// `a op b`
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// `condition ? then : otherwise`
    Choice(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `name(arguments)`
    Call(String, Vec<Expr>),
}

impl Expr {
    /// The names of the variables the expression reads, each once, in the order they appear.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<String>) {
        match self {
            Expr::Num(_) | Expr::Text(_) => {}
            Expr::Var(n) => {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
            Expr::Neg(e) | Expr::Not(e) => e.collect(out),
            Expr::Bin(_, a, b) => {
                a.collect(out);
                b.collect(out);
            }
            Expr::Choice(c, a, b) => {
                c.collect(out);
                a.collect(out);
                b.collect(out);
            }
            Expr::Call(_, args) => {
                for a in args {
                    a.collect(out);
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------ reading

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(n) => format!("the number {}", format_number(*n)),
        Tok::Str(_) => "a text".to_string(),
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Op(o) => format!("`{o}`"),
        Tok::LParen => "`(`".to_string(),
        Tok::RParen => "`)`".to_string(),
        Tok::Comma => "`,`".to_string(),
        Tok::Question => "`?`".to_string(),
        Tok::Colon => "`:`".to_string(),
    }
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let next_is_digit = chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        if c.is_ascii_digit() || (c == '.' && next_is_digit) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            let n: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
            out.push(Tok::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let d = chars[i];
                i += 1;
                if d == quote {
                    closed = true;
                    break;
                }
                if d == '\\' {
                    let Some(&e) = chars.get(i) else {
                        break;
                    };
                    i += 1;
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    s.push(d);
                }
            }
            if !closed {
                return Err("a text between quotes is not closed".to_string());
            }
            out.push(Tok::Str(s));
            continue;
        }
        let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
        let two_op: Option<&'static str> = match two.as_str() {
            "==" => Some("=="),
            "!=" => Some("!="),
            "<=" => Some("<="),
            ">=" => Some(">="),
            "&&" => Some("&&"),
            "||" => Some("||"),
            _ => None,
        };
        if let Some(op) = two_op {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        let tok = match c {
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '%' => Tok::Op("%"),
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            '!' => Tok::Op("!"),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            '?' => Tok::Question,
            ':' => Tok::Colon,
            '=' => return Err("`=` alone is not an operator: write `==` to compare".to_string()),
            '&' | '|' => return Err(format!("write `{c}{c}`, not a single `{c}`")),
            other => return Err(format!("`{other}` cannot be used in an expression")),
        };
        out.push(tok);
        i += 1;
    }
    Ok(out)
}

/// From the loosest binding operators to the tightest.
const LEVELS: [&[&str]; 6] = [
    &["||"],
    &["&&"],
    &["==", "!="],
    &["<", "<=", ">", ">="],
    &["+", "-"],
    &["*", "/", "%"],
];

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_op(&mut self, op: &str) -> bool {
        let hit = matches!(self.peek(), Some(Tok::Op(o)) if *o == op);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err("the expression nests too deeply".to_string())
        } else {
            Ok(())
        }
    }

    fn choice(&mut self) -> Result<Expr, String> {
        self.enter()?;
        let c = self.level(0)?;
        let out = if matches!(self.peek(), Some(Tok::Question)) {
            self.pos += 1;
            let a = self.choice()?;
            if !matches!(self.bump(), Some(Tok::Colon)) {
                return Err("a `?` needs a `:` after its first value".to_string());
            }
            let b = self.choice()?;
            Expr::Choice(Box::new(c), Box::new(a), Box::new(b))
        } else {
            c
        };
        self.depth -= 1;
        Ok(out)
    }

    fn level(&mut self, lvl: usize) -> Result<Expr, String> {
        if lvl >= LEVELS.len() {
            return self.unary();
        }
        let mut left = self.level(lvl + 1)?;
        loop {
            let symbol = match self.peek() {
                Some(Tok::Op(o)) if LEVELS[lvl].contains(o) => *o,
                _ => break,
            };
            self.pos += 1;
            let right = self.level(lvl + 1)?;
            let Some(op) = BinOp::from_symbol(symbol) else {
                return Err(format!("`{symbol}` is not an operator"));
            };
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_op("-") {
            self.enter()?;
            let e = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Neg(Box::new(e)));
        }
        if self.eat_op("!") {
            self.enter()?;
            let e = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(e)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Text(s)),
            Some(Tok::LParen) => {
                let e = self.choice()?;
                if !matches!(self.bump(), Some(Tok::RParen)) {
                    return Err("a `(` is not closed".to_string());
                }
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if matches!(self.peek(), Some(Tok::RParen)) {
                        self.pos += 1;
                    } else {
                        loop {
                            args.push(self.choice()?);
                            if args.len() > MAX_ARGS {
                                return Err(format!("`{name}` takes at most {MAX_ARGS} values"));
                            }
                            match self.bump() {
                                Some(Tok::Comma) => {}
                                Some(Tok::RParen) => break,
                                _ => return Err(format!("`{name}(` needs `,` or `)`")),
                            }
                        }
                    }
                    check_call(&name, args.len())?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(match name.as_str() {
                        "true" => Expr::Num(1.0),
                        "false" => Expr::Num(0.0),
                        _ => Expr::Var(name),
                    })
                }
            }
            Some(t) => Err(format!("{} cannot stand here", describe(&t))),
            None => Err("the expression ends too early".to_string()),
        }
    }
}

/// Whether `name` is a function and takes `count` values.
fn check_call(name: &str, count: usize) -> Result<(), String> {
    match FUNCTIONS.iter().find(|f| f.0 == name) {
        None => Err(format!("`{name}` is not a function of expressions")),
        Some(&(_, least, most, _)) => {
            if count < least || count > most {
                let wanted = if least == most {
                    least.to_string()
                } else {
                    format!("{least} to {most}")
                };
                Err(format!("`{name}` takes {wanted} value(s), not {count}"))
            } else {
                Ok(())
            }
        }
    }
}

/// Reads the text of an expression (without the `{{=` and `}}`). The error says what is wrong.
pub fn parse(src: &str) -> Result<Expr, String> {
    if src.chars().count() > MAX_EXPR_CHARS {
        return Err(format!(
            "the expression is longer than {MAX_EXPR_CHARS} characters"
        ));
    }
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("the expression is empty".to_string());
    }
    let mut p = Parser {
        toks,
        pos: 0,
        depth: 0,
    };
    let e = p.choice()?;
    if let Some(t) = p.peek() {
        return Err(format!("{} cannot stand here", describe(t)));
    }
    Ok(e)
}

// ------------------------------------------------------------------------------- working out

/// A value on the way: a number, or a text (a variable's text is a number when it spells one).
#[derive(Clone, Debug)]
enum Val {
    Num(f64),
    Text(String),
}

impl Val {
    fn flag(b: bool) -> Val {
        Val::Num(if b { 1.0 } else { 0.0 })
    }

    fn text(&self) -> String {
        match self {
            Val::Num(n) => format_number(*n),
            Val::Text(s) => s.clone(),
        }
    }

    fn num(&self) -> Result<f64, String> {
        match self {
            Val::Num(n) => Ok(*n),
            Val::Text(s) => {
                parse_number(s).ok_or_else(|| format!("`{}` is not a number", brief(s)))
            }
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Val::Num(n) => *n != 0.0,
            Val::Text(s) => is_truthy(s),
        }
    }
}

fn brief(s: &str) -> String {
    let mut short: String = s.chars().take(24).collect();
    if s.chars().count() > 24 {
        short.push('\u{2026}');
    }
    short
}

/// A text cut to what a variable can hold, so no step of an expression can grow without limit.
fn cap(s: String) -> String {
    if s.chars().count() > MAX_VALUE_CHARS {
        s.chars().take(MAX_VALUE_CHARS).collect()
    } else {
        s
    }
}

fn finite(n: f64) -> Result<Val, String> {
    if n.is_finite() {
        Ok(Val::Num(n))
    } else {
        Err("the result is too large".to_string())
    }
}

fn eval(e: &Expr, vars: &dyn Fn(&str) -> Option<String>) -> Result<Val, String> {
    match e {
        Expr::Num(n) => Ok(Val::Num(*n)),
        Expr::Text(s) => Ok(Val::Text(s.clone())),
        Expr::Var(n) => Ok(Val::Text(vars(n).unwrap_or_default())),
        Expr::Neg(x) => finite(-eval(x, vars)?.num()?),
        Expr::Not(x) => Ok(Val::flag(!eval(x, vars)?.truthy())),
        Expr::Choice(c, a, b) => {
            if eval(c, vars)?.truthy() {
                eval(a, vars)
            } else {
                eval(b, vars)
            }
        }
        Expr::Bin(op, a, b) => binary(*op, a, b, vars),
        Expr::Call(name, args) => {
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(eval(a, vars)?);
            }
            call(name, &values)
        }
    }
}

fn binary(
    op: BinOp,
    a: &Expr,
    b: &Expr,
    vars: &dyn Fn(&str) -> Option<String>,
) -> Result<Val, String> {
    let left = eval(a, vars)?;
    // `&&` and `||` do not work out the right side when the left one decides
    match op {
        BinOp::And if !left.truthy() => return Ok(Val::flag(false)),
        BinOp::Or if left.truthy() => return Ok(Val::flag(true)),
        _ => {}
    }
    let right = eval(b, vars)?;
    let order = |cmp: CmpOp| Val::flag(compare(cmp, &left.text(), &right.text()));
    match op {
        BinOp::And | BinOp::Or => Ok(Val::flag(right.truthy())),
        BinOp::Eq => Ok(order(CmpOp::Eq)),
        BinOp::Ne => Ok(order(CmpOp::Ne)),
        BinOp::Lt => Ok(order(CmpOp::Lt)),
        BinOp::Le => Ok(order(CmpOp::Le)),
        BinOp::Gt => Ok(order(CmpOp::Gt)),
        BinOp::Ge => Ok(order(CmpOp::Ge)),
        BinOp::Add => finite(left.num()? + right.num()?),
        BinOp::Sub => finite(left.num()? - right.num()?),
        BinOp::Mul => finite(left.num()? * right.num()?),
        BinOp::Div | BinOp::Rem => {
            let (x, y) = (left.num()?, right.num()?);
            if y == 0.0 {
                return Err("it divides by zero".to_string());
            }
            finite(if op == BinOp::Div { x / y } else { x % y })
        }
    }
}

fn call(name: &str, v: &[Val]) -> Result<Val, String> {
    let text = |i: usize| v.get(i).map(Val::text).unwrap_or_default();
    let num = |i: usize| match v.get(i) {
        Some(x) => x.num(),
        None => Err("a value is missing".to_string()),
    };
    match name {
        "abs" => finite(num(0)?.abs()),
        "floor" => finite(num(0)?.floor()),
        "ceil" => finite(num(0)?.ceil()),
        "sqrt" => {
            let x = num(0)?;
            if x < 0.0 {
                return Err("the square root of a negative number".to_string());
            }
            finite(x.sqrt())
        }
        "pow" => finite(num(0)?.powf(num(1)?)),
        "round" => {
            let x = num(0)?;
            if v.len() < 2 {
                return finite(x.round());
            }
            let digits = num(1)?.clamp(0.0, 12.0).round();
            let scale = 10f64.powf(digits);
            finite((x * scale).round() / scale)
        }
        "min" | "max" => {
            let mut best = num(0)?;
            for i in 1..v.len() {
                let x = num(i)?;
                best = if name == "min" {
                    best.min(x)
                } else {
                    best.max(x)
                };
            }
            finite(best)
        }
        "len" => Ok(Val::Num(text(0).chars().count() as f64)),
        "upper" => Ok(Val::Text(cap(text(0).to_uppercase()))),
        "lower" => Ok(Val::Text(cap(text(0).to_lowercase()))),
        "trim" => Ok(Val::Text(text(0).trim().to_string())),
        "concat" => {
            let mut s = String::new();
            for x in v {
                s.push_str(&x.text());
                s = cap(s);
            }
            Ok(Val::Text(s))
        }
        "contains" => Ok(Val::flag(text(0).contains(&text(1)))),
        "starts" => Ok(Val::flag(text(0).starts_with(&text(1)))),
        "ends" => Ok(Val::flag(text(0).ends_with(&text(1)))),
        "empty" => Ok(Val::flag(text(0).trim().is_empty())),
        "if" => Ok(if v.first().is_some_and(Val::truthy) {
            v.get(1).cloned().unwrap_or(Val::Text(String::new()))
        } else {
            v.get(2).cloned().unwrap_or(Val::Text(String::new()))
        }),
        "not" => Ok(Val::flag(!v.first().is_some_and(Val::truthy))),
        other => Err(format!("`{other}` is not a function of expressions")),
    }
}

/// The value of the expression `src` (the text between `{{=` and `}}`) with the variables of
/// `vars`.
///
/// - `None` when the expression has a **syntax error**: the caller leaves the text as written.
/// - `Some("")` when it reads well but cannot be worked out (not a number, a division by zero).
pub fn evaluate(src: &str, vars: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let expr = parse(src).ok()?;
    Some(match eval(&expr, vars) {
        Ok(v) => cap(v.text()),
        Err(_) => String::new(),
    })
}

/// The names of the variables the expression `src` reads; empty when it has a syntax error.
pub fn variables(src: &str) -> Vec<String> {
    parse(src).map(|e| e.variables()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, vars: &[(&str, &str)]) -> Option<String> {
        let lookup = |n: &str| {
            vars.iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| (*v).to_string())
        };
        evaluate(src, &lookup)
    }

    fn val(src: &str) -> String {
        run(src, &[]).unwrap_or_else(|| "SYNTAX".to_string())
    }

    #[test]
    fn arithmetic_follows_the_usual_order() {
        assert_eq!(val("1 + 2 * 3"), "7");
        assert_eq!(val("(1 + 2) * 3"), "9");
        assert_eq!(val("10 - 4 - 3"), "3", "left to right");
        assert_eq!(val("7 % 4 + 2 * -3"), "-3");
        assert_eq!(val("1 / 4"), "0.25");
        assert_eq!(val("--2"), "2");
    }

    #[test]
    fn variables_are_read_as_numbers_or_as_text() {
        let vars = [("a", "4"), ("b", "2.5"), ("name", "Ada")];
        assert_eq!(run("a * b", &vars).as_deref(), Some("10"));
        assert_eq!(run("name == \"Ada\"", &vars).as_deref(), Some("1"));
        assert_eq!(run("upper(name)", &vars).as_deref(), Some("ADA"));
        assert_eq!(
            run("missing == \"\"", &vars).as_deref(),
            Some("1"),
            "an unset variable is empty text"
        );
    }

    #[test]
    fn comparison_and_logic_give_one_or_zero() {
        assert_eq!(val("2 < 10"), "1", "as numbers, not as text");
        assert_eq!(
            val("\"2\" < \"10\""),
            "1",
            "texts that spell numbers are numbers"
        );
        assert_eq!(val("\"b\" > \"a\""), "1");
        assert_eq!(val("1 == 1.0"), "1");
        assert_eq!(val("1 && 0 || 1"), "1");
        assert_eq!(val("!0"), "1");
        assert_eq!(val("!(2 > 1)"), "0");
        assert_eq!(val("true && false"), "0");
    }

    #[test]
    fn a_choice_picks_one_side_and_nests_to_the_right() {
        assert_eq!(val("1 ? \"a\" : \"b\""), "a");
        assert_eq!(val("0 ? \"a\" : 0 ? \"b\" : \"c\""), "c");
        assert_eq!(val("if(2 > 1, 10, 20)"), "10");
    }

    #[test]
    fn the_right_side_of_and_or_is_not_worked_out_when_the_left_decides() {
        // `1 / 0` would make the whole expression empty if it were worked out
        assert_eq!(val("0 && 1 / 0"), "0");
        assert_eq!(val("1 || 1 / 0"), "1");
        assert_eq!(val("1 && 1 / 0"), "");
    }

    #[test]
    fn what_cannot_be_worked_out_is_empty_and_what_cannot_be_read_stays_as_written() {
        assert_eq!(run("1 / 0", &[]).as_deref(), Some(""), "division by zero");
        assert_eq!(run("1 + \"x\"", &[]).as_deref(), Some(""), "not a number");
        assert_eq!(run("sqrt(-1)", &[]).as_deref(), Some(""));
        assert_eq!(run("1 +", &[]), None, "ends too early");
        assert_eq!(run("1 = 1", &[]), None, "a single =");
        assert_eq!(run("nope(1)", &[]), None, "no such function");
        assert_eq!(run("abs(1, 2)", &[]), None, "wrong number of values");
        assert_eq!(run("(1 + 2", &[]), None, "bracket not closed");
        assert_eq!(run("\"open", &[]), None, "text not closed");
        assert_eq!(run("", &[]), None, "empty");
        assert_eq!(run("1 2", &[]), None, "two values in a row");
    }

    #[test]
    fn functions() {
        assert_eq!(val("abs(-3)"), "3");
        assert_eq!(val("round(2.5)"), "3");
        assert_eq!(val("round(3.14159, 2)"), "3.14");
        assert_eq!(val("floor(2.9) + ceil(2.1)"), "5");
        assert_eq!(val("pow(2, 10)"), "1024");
        assert_eq!(val("sqrt(16)"), "4");
        assert_eq!(val("min(5, 2, 9)"), "2");
        assert_eq!(val("max(5, 2, 9)"), "9");
        assert_eq!(val("len(\"héllo\")"), "5");
        assert_eq!(val("concat(\"a\", 1, 'b')"), "a1b");
        assert_eq!(val("trim('  x ')"), "x");
        assert_eq!(val("contains(\"hello\", \"ell\")"), "1");
        assert_eq!(val("starts(\"hello\", \"he\")"), "1");
        assert_eq!(val("ends(\"hello\", \"he\")"), "0");
        assert_eq!(val("empty(\"  \")"), "1");
        assert_eq!(val("lower(\"AbC\")"), "abc");
        assert_eq!(val("not(0)"), "1");
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(val(r#""say \"hi\"""#), "say \"hi\"");
        assert_eq!(val(r"'it\'s'"), "it's");
        assert_eq!(val(r#"len("a\nb")"#), "3");
    }

    #[test]
    fn the_reading_is_bounded() {
        let deep = format!(
            "{}1{}",
            "(".repeat(MAX_DEPTH + 5),
            ")".repeat(MAX_DEPTH + 5)
        );
        assert!(parse(&deep).is_err(), "nesting is limited");
        let nots = format!("{}1", "!".repeat(MAX_DEPTH + 5));
        assert!(parse(&nots).is_err(), "prefix operators are limited");
        let long = format!("1{}", " + 1".repeat(MAX_EXPR_CHARS));
        assert!(parse(&long).is_err(), "length is limited");
        let many = format!("max({})", ["1"; MAX_ARGS + 1].join(","));
        assert!(parse(&many).is_err(), "arguments are limited");
    }

    #[test]
    fn a_text_cannot_grow_without_limit() {
        let big = "x".repeat(MAX_VALUE_CHARS);
        let vars = [("big", big.as_str())];
        let src = "concat(concat(big,big,big,big,big,big,big,big),concat(big,big,big,big,big,big,big,big),big)";
        let out = run(src, &vars).unwrap();
        assert!(out.chars().count() <= MAX_VALUE_CHARS);
    }

    #[test]
    fn the_tree_names_the_variables_it_reads() {
        let e = parse("a + b * a > limit ? max(c, 1) : d").unwrap();
        assert_eq!(e.variables(), ["a", "b", "limit", "c", "d"]);
        assert_eq!(variables("x +"), Vec::<String>::new());
        assert_eq!(
            parse("1 + 2").unwrap(),
            Expr::Bin(
                BinOp::Add,
                Box::new(Expr::Num(1.0)),
                Box::new(Expr::Num(2.0))
            )
        );
    }

    #[test]
    fn every_function_is_known_to_the_checker_and_the_evaluator() {
        for (name, least, _, _) in FUNCTIONS {
            let args = vec!["1"; least].join(",");
            let src = format!("{name}({args})");
            let e = parse(&src).unwrap_or_else(|why| panic!("{src}: {why}"));
            assert!(matches!(e, Expr::Call(..)));
            // reaching the evaluator's last arm would be an error text, never a value
            let lookup = |_: &str| -> Option<String> { None };
            if let Err(why) = eval(&e, &lookup) {
                assert!(!why.contains("is not a function"), "{src}: {why}");
            }
        }
    }

    #[test]
    fn a_text_with_an_expression_inside_is_filled_in() {
        use crate::platform::Os;
        let mut vars = crate::flowrun::Vars::default();
        vars.set("n", "4");
        let out = crate::flowrun::expand("n+1 is {{= n + 1 }}, big: {{= n > 3 }}", &vars, Os::Mac);
        assert_eq!(out, "n+1 is 5, big: 1");
        // a syntax error stays as written
        let out = crate::flowrun::expand("{{= n + }}", &vars, Os::Mac);
        assert_eq!(out, "{{= n + }}");
        // a calculation that fails is empty
        let out = crate::flowrun::expand("[{{= n / 0 }}]", &vars, Os::Mac);
        assert_eq!(out, "[]");
    }
}
