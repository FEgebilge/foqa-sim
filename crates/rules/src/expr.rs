//! Rule expression language: arithmetic, comparisons and logic over flight
//! parameters. Numbers carry units (`1000 ft`, `30 deg`, `2 s`), and every
//! expression is unit-checked when its rule is loaded, before any flight is
//! analysed. Evaluation is three-valued: missing data makes a result
//! unknown, never silently true or false.

use std::fmt;
use std::ops::Range;

use fdm_core::Unit;

/// Static type of an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Bool,
    /// A number with its unit; `None` is dimensionless.
    Num(Option<Unit>),
}

impl Ty {
    /// The type a parameter of this LFL unit has in expressions.
    pub fn of(unit: Unit) -> Self {
        match unit {
            Unit::Discrete => Self::Bool,
            Unit::Ratio | Unit::Index => Self::Num(None),
            unit => Self::Num(Some(unit)),
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => f.write_str("a condition"),
            Self::Num(None) => f.write_str("a plain number"),
            Self::Num(Some(unit)) => write!(f, "{}", unit.symbol()),
        }
    }
}

/// Names a rule may use, resolved at load time.
pub trait Scope {
    type Var: Copy;
    fn resolve(&self, name: &str) -> Option<(Self::Var, Ty)>;
}

/// Variable values at evaluation time; `None` where data is missing or
/// invalid. Conditions are 0.0 / 1.0.
pub trait Values<V> {
    fn get(&self, var: V) -> Option<f64>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub message: String,
    /// Byte range in the source.
    pub span: Range<usize>,
}

impl Error {
    fn new(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// The source with a caret line under the offending part.
    pub fn render(&self, src: &str) -> String {
        let col = src[..self.span.start.min(src.len())].chars().count();
        let len = src
            .get(self.span.clone())
            .map_or(1, |s| s.chars().count().max(1));
        format!(
            "{src}\n{}{} {}",
            " ".repeat(col),
            "^".repeat(len),
            self.message
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arith {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Func {
    Abs,
    Min,
    Max,
}

#[derive(Debug, Clone)]
enum Node<V> {
    Const(f64),
    Var(V),
    Neg(Box<Expr<V>>),
    Not(Box<Expr<V>>),
    Arith(Arith, Box<Expr<V>>, Box<Expr<V>>),
    Cmp(Cmp, Box<Expr<V>>, Box<Expr<V>>),
    And(Box<Expr<V>>, Box<Expr<V>>),
    Or(Box<Expr<V>>, Box<Expr<V>>),
    Call(Func, Vec<Expr<V>>),
}

/// A compiled, type-checked expression.
#[derive(Debug, Clone)]
pub struct Expr<V> {
    node: Node<V>,
    ty: Ty,
    span: Range<usize>,
}

impl<V: Copy> Expr<V> {
    pub fn ty(&self) -> Ty {
        self.ty
    }

    pub fn eval(&self, values: &impl Values<V>) -> Option<f64> {
        let truth = |b: bool| f64::from(u8::from(b));
        match &self.node {
            Node::Const(x) => Some(*x),
            Node::Var(v) => values.get(*v),
            Node::Neg(e) => Some(-e.eval(values)?),
            Node::Not(e) => e.truth(values).map(|b| truth(!b)),
            Node::Arith(op, a, b) => {
                let (a, b) = (a.eval(values)?, b.eval(values)?);
                Some(match op {
                    Arith::Add => a + b,
                    Arith::Sub => a - b,
                    Arith::Mul => a * b,
                    Arith::Div => a / b,
                })
            }
            Node::Cmp(op, a, b) => {
                let (a, b) = (a.eval(values)?, b.eval(values)?);
                Some(truth(match op {
                    Cmp::Lt => a < b,
                    Cmp::Le => a <= b,
                    Cmp::Gt => a > b,
                    Cmp::Ge => a >= b,
                    Cmp::Eq => a == b,
                    Cmp::Ne => a != b,
                }))
            }
            // Kleene logic: a known false/true operand decides even if the
            // other is unknown.
            Node::And(a, b) => match (a.truth(values), b.truth(values)) {
                (Some(false), _) | (_, Some(false)) => Some(0.0),
                (Some(true), Some(true)) => Some(1.0),
                _ => None,
            },
            Node::Or(a, b) => match (a.truth(values), b.truth(values)) {
                (Some(true), _) | (_, Some(true)) => Some(1.0),
                (Some(false), Some(false)) => Some(0.0),
                _ => None,
            },
            Node::Call(func, args) => {
                let x = args[0].eval(values)?;
                match func {
                    Func::Abs => Some(x.abs()),
                    Func::Min => Some(x.min(args[1].eval(values)?)),
                    Func::Max => Some(x.max(args[1].eval(values)?)),
                }
            }
        }
    }

    /// A condition's value.
    pub fn truth(&self, values: &impl Values<V>) -> Option<bool> {
        self.eval(values).map(|x| x >= 0.5)
    }
}

/// Compiles and type-checks an expression.
pub fn compile<S: Scope>(src: &str, scope: &S) -> Result<Expr<S::Var>, Error> {
    let mut parser = Parser {
        tokens: lex(src)?,
        pos: 0,
        scope,
    };
    let expr = parser.or()?;
    match parser.peek() {
        (Tok::End, _) => Ok(expr),
        (_, span) => Err(Error::new("unexpected text after the expression", span)),
    }
}

/// A single number with an optional unit, e.g. a threshold `30 deg` or a
/// duration `2 s`.
pub fn literal(src: &str) -> Result<(f64, Option<Unit>), Error> {
    let tokens = lex(src)?;
    match &tokens[..] {
        [(Tok::Num(x, unit), _), (Tok::End, _)] => Ok((*x, *unit)),
        [(Tok::Op("-"), _), (Tok::Num(x, unit), _), (Tok::End, _)] => Ok((-x, *unit)),
        _ => Err(Error::new(
            "expected a number with a unit, e.g. `30 deg`",
            0..src.len(),
        )),
    }
}

/// A `literal` that must be in `unit`. A wrong unit reads
/// "{name} `{src}` is {its unit}, but {against} is {unit}".
pub fn quantity(src: &str, unit: Option<Unit>, name: &str, against: &str) -> Result<f64, String> {
    let (value, got) = literal(src).map_err(|e| e.render(src))?;
    if got != unit {
        return Err(format!(
            "{name} `{src}` is {}, but {against} is {}",
            Ty::Num(got),
            Ty::Num(unit)
        ));
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, Option<Unit>),
    Ident(String),
    Op(&'static str),
    End,
}

const OPS: [&str; 13] = [
    "<=", ">=", "==", "!=", "<", ">", "+", "-", "*", "/", "(", ")", ",",
];

fn unit_named(word: &str) -> Option<Unit> {
    Some(match word {
        "ft" => Unit::Feet,
        "kt" => Unit::Knots,
        "deg" => Unit::Degrees,
        "g" => Unit::G,
        "fpm" => Unit::FeetPerMinute,
        "dot" | "dots" => Unit::Dots,
        "%" => Unit::Percent,
        "degC" => Unit::Celsius,
        "hPa" => Unit::Hectopascal,
        "lb" => Unit::Pounds,
        "s" => Unit::Seconds,
        _ => return None,
    })
}

fn lex(src: &str) -> Result<Vec<(Tok, Range<usize>)>, Error> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let word_end = |from: usize, ok: fn(u8) -> bool| {
        (from..bytes.len())
            .find(|&j| !ok(bytes[j]))
            .unwrap_or(bytes.len())
    };
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let end = word_end(i, |b| b.is_ascii_digit() || b == b'.');
            let x: f64 = src[i..end]
                .parse()
                .map_err(|_| Error::new("malformed number", i..end))?;
            // A unit may follow, separated by spaces.
            let unit_start = word_end(end, |b| b == b' ');
            let unit_end = word_end(unit_start, |b| b.is_ascii_alphabetic() || b == b'%');
            match unit_named(&src[unit_start..unit_end]) {
                Some(unit) => {
                    tokens.push((Tok::Num(x, Some(unit)), i..unit_end));
                    i = unit_end;
                }
                None => {
                    tokens.push((Tok::Num(x, None), i..end));
                    i = end;
                }
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let end = word_end(i, |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
            tokens.push((Tok::Ident(src[i..end].to_owned()), i..end));
            i = end;
        } else if let Some(op) = OPS.iter().find(|op| src[i..].starts_with(*op)) {
            tokens.push((Tok::Op(op), i..i + op.len()));
            i += op.len();
        } else {
            let len = src[i..].chars().next().map_or(1, char::len_utf8);
            return Err(Error::new("unexpected character", i..i + len));
        }
    }
    tokens.push((Tok::End, src.len()..src.len()));
    Ok(tokens)
}

struct Parser<'s, S> {
    tokens: Vec<(Tok, Range<usize>)>,
    pos: usize,
    scope: &'s S,
}

impl<S: Scope> Parser<'_, S> {
    fn peek(&self) -> (Tok, Range<usize>) {
        self.tokens[self.pos].clone()
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        let hit = &self.tokens[self.pos].0 == tok;
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn eat_word(&mut self, word: &str) -> bool {
        self.eat(&Tok::Ident(word.to_owned()))
    }

    fn expect(&mut self, op: &'static str) -> Result<(), Error> {
        if self.eat(&Tok::Op(op)) {
            Ok(())
        } else {
            Err(Error::new(format!("expected `{op}`"), self.peek().1))
        }
    }

    fn or(&mut self) -> Result<Expr<S::Var>, Error> {
        let mut left = self.and()?;
        while self.eat_word("or") {
            let right = self.and()?;
            left = logic(left, right, Node::Or)?;
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr<S::Var>, Error> {
        let mut left = self.not()?;
        while self.eat_word("and") {
            let right = self.not()?;
            left = logic(left, right, Node::And)?;
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr<S::Var>, Error> {
        let start = self.peek().1.start;
        if !self.eat_word("not") {
            return self.comparison();
        }
        let inner = self.not()?;
        expect_bool(&inner)?;
        let span = start..inner.span.end;
        Ok(Expr {
            node: Node::Not(Box::new(inner)),
            ty: Ty::Bool,
            span,
        })
    }

    fn comparison(&mut self) -> Result<Expr<S::Var>, Error> {
        let left = self.sum()?;
        let op = match self.peek().0 {
            Tok::Op("<") => Cmp::Lt,
            Tok::Op("<=") => Cmp::Le,
            Tok::Op(">") => Cmp::Gt,
            Tok::Op(">=") => Cmp::Ge,
            Tok::Op("==") => Cmp::Eq,
            Tok::Op("!=") => Cmp::Ne,
            _ => return Ok(left),
        };
        self.pos += 1;
        let right = self.sum()?;
        let bools = left.ty == Ty::Bool && right.ty == Ty::Bool && matches!(op, Cmp::Eq | Cmp::Ne);
        if !bools {
            expect_num(&left)?;
            same_unit(&left, &right, "compares")?;
        }
        let span = left.span.start..right.span.end;
        Ok(Expr {
            node: Node::Cmp(op, Box::new(left), Box::new(right)),
            ty: Ty::Bool,
            span,
        })
    }

    fn sum(&mut self) -> Result<Expr<S::Var>, Error> {
        let mut left = self.term()?;
        loop {
            let op = match self.peek().0 {
                Tok::Op("+") => Arith::Add,
                Tok::Op("-") => Arith::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.term()?;
            expect_num(&left)?;
            same_unit(
                &left,
                &right,
                if op == Arith::Add {
                    "adds"
                } else {
                    "subtracts"
                },
            )?;
            let (ty, span) = (left.ty, left.span.start..right.span.end);
            left = Expr {
                node: Node::Arith(op, Box::new(left), Box::new(right)),
                ty,
                span,
            };
        }
    }

    fn term(&mut self) -> Result<Expr<S::Var>, Error> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek().0 {
                Tok::Op("*") => Arith::Mul,
                Tok::Op("/") => Arith::Div,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.unary()?;
            let span = left.span.start..right.span.end;
            // Scaling only: a unit times a unit has no LFL unit.
            let ty = match (expect_num(&left)?, expect_num(&right)?) {
                (unit, None) => Ty::Num(unit),
                (None, unit) if op == Arith::Mul => Ty::Num(unit),
                _ => {
                    return Err(Error::new(
                        "only scaling by a plain number is supported",
                        span,
                    ));
                }
            };
            left = Expr {
                node: Node::Arith(op, Box::new(left), Box::new(right)),
                ty,
                span,
            };
        }
    }

    fn unary(&mut self) -> Result<Expr<S::Var>, Error> {
        let start = self.peek().1.start;
        if !self.eat(&Tok::Op("-")) {
            return self.primary();
        }
        let inner = self.unary()?;
        expect_num(&inner)?;
        let (ty, span) = (inner.ty, start..inner.span.end);
        Ok(Expr {
            node: Node::Neg(Box::new(inner)),
            ty,
            span,
        })
    }

    fn primary(&mut self) -> Result<Expr<S::Var>, Error> {
        let (tok, span) = self.peek();
        self.pos += 1;
        match tok {
            Tok::Num(x, unit) => Ok(Expr {
                node: Node::Const(x),
                ty: Ty::Num(unit),
                span,
            }),
            Tok::Op("(") => {
                let inner = self.or()?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::Ident(word) if word == "true" || word == "false" => Ok(Expr {
                node: Node::Const(f64::from(u8::from(word == "true"))),
                ty: Ty::Bool,
                span,
            }),
            Tok::Ident(name) if self.peek().0 == Tok::Op("(") => self.call(&name, span),
            Tok::Ident(name) => match self.scope.resolve(&name) {
                Some((var, ty)) => Ok(Expr {
                    node: Node::Var(var),
                    ty,
                    span,
                }),
                None => Err(Error::new(format!("unknown parameter `{name}`"), span)),
            },
            Tok::End => Err(Error::new("expression ends too early", span)),
            _ => Err(Error::new("expected a number, parameter or `(`", span)),
        }
    }

    fn call(&mut self, name: &str, name_span: Range<usize>) -> Result<Expr<S::Var>, Error> {
        let (func, arity) = match name {
            "abs" => (Func::Abs, 1),
            "min" => (Func::Min, 2),
            "max" => (Func::Max, 2),
            _ => {
                return Err(Error::new(
                    format!("unknown function `{name}`; available: abs, min, max"),
                    name_span,
                ));
            }
        };
        self.expect("(")?;
        let mut args = vec![self.or()?];
        while self.eat(&Tok::Op(",")) {
            args.push(self.or()?);
        }
        let end = self.peek().1.end;
        self.expect(")")?;
        let span = name_span.start..end;
        if args.len() != arity {
            return Err(Error::new(
                format!("`{name}` takes {arity} argument(s)"),
                span,
            ));
        }
        expect_num(&args[0])?;
        if let [a, b] = &args[..] {
            same_unit(a, b, "mixes")?;
        }
        let ty = args[0].ty;
        Ok(Expr {
            node: Node::Call(func, args),
            ty,
            span,
        })
    }
}

/// Builds a two-operand node, e.g. `Node::And`.
type Binary<V> = fn(Box<Expr<V>>, Box<Expr<V>>) -> Node<V>;

fn logic<V>(left: Expr<V>, right: Expr<V>, node: Binary<V>) -> Result<Expr<V>, Error> {
    expect_bool(&left)?;
    expect_bool(&right)?;
    let span = left.span.start..right.span.end;
    Ok(Expr {
        node: node(Box::new(left), Box::new(right)),
        ty: Ty::Bool,
        span,
    })
}

fn expect_bool<V>(e: &Expr<V>) -> Result<(), Error> {
    match e.ty {
        Ty::Bool => Ok(()),
        ty => Err(Error::new(
            format!("expected a condition, found {ty}"),
            e.span.clone(),
        )),
    }
}

fn expect_num<V>(e: &Expr<V>) -> Result<Option<Unit>, Error> {
    match e.ty {
        Ty::Num(unit) => Ok(unit),
        Ty::Bool => Err(Error::new(
            "expected a number, found a condition",
            e.span.clone(),
        )),
    }
}

fn same_unit<V>(left: &Expr<V>, right: &Expr<V>, verb: &str) -> Result<(), Error> {
    let (l, r) = (left.ty, right.ty);
    if l == r {
        return Ok(());
    }
    let message = match (l, r) {
        (Ty::Num(Some(unit)), Ty::Num(None)) => format!(
            "this number needs a unit: the other side is {}",
            unit.symbol()
        ),
        _ => format!("{verb} {l} with {r}"),
    };
    Err(Error::new(message, right.span.clone()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct Toy(HashMap<&'static str, (Ty, Option<f64>)>);

    impl Toy {
        fn new() -> Self {
            Self(HashMap::from([
                ("height", (Ty::Num(Some(Unit::Feet)), Some(800.0))),
                ("roll", (Ty::Num(Some(Unit::Degrees)), Some(-38.0))),
                ("vs", (Ty::Num(Some(Unit::FeetPerMinute)), Some(-1200.0))),
                ("ap.engaged", (Ty::Bool, Some(0.0))),
                ("nav.gs_dev", (Ty::Num(Some(Unit::Dots)), None)),
            ]))
        }
    }

    impl Scope for Toy {
        type Var = &'static str;
        fn resolve(&self, name: &str) -> Option<(&'static str, Ty)> {
            self.0.get_key_value(name).map(|(k, (ty, _))| (*k, *ty))
        }
    }

    impl Values<&'static str> for Toy {
        fn get(&self, var: &'static str) -> Option<f64> {
            self.0[var].1
        }
    }

    fn eval(src: &str) -> Option<f64> {
        let toy = Toy::new();
        compile(src, &toy)
            .unwrap_or_else(|e| panic!("{}", e.render(src)))
            .eval(&toy)
    }

    fn error(src: &str) -> String {
        compile(src, &Toy::new()).unwrap_err().render(src)
    }

    #[test]
    fn evaluates_with_units() {
        assert_eq!(eval("abs(roll)"), Some(38.0));
        assert_eq!(eval("height < 1000 ft and abs(roll) > 35 deg"), Some(1.0));
        assert_eq!(eval("-vs > 2 * 500 fpm"), Some(1.0));
        assert_eq!(eval("not ap.engaged"), Some(1.0));
        assert_eq!(eval("max(height, 1000 ft) - 50 ft"), Some(950.0));
    }

    #[test]
    fn missing_data_is_unknown_unless_logic_decides() {
        assert_eq!(eval("abs(nav.gs_dev) > 1 dot"), None);
        assert_eq!(
            eval("height > 5000 ft and abs(nav.gs_dev) > 1 dot"),
            Some(0.0)
        );
        assert_eq!(
            eval("height < 5000 ft or abs(nav.gs_dev) > 1 dot"),
            Some(1.0)
        );
    }

    #[test]
    fn unit_mismatch_points_at_the_offending_side() {
        assert_eq!(
            error("height < 1000 fpm"),
            "height < 1000 fpm\n         ^^^^^^^^ compares ft with fpm"
        );
        assert_eq!(
            error("height < 1000"),
            "height < 1000\n         ^^^^ this number needs a unit: the other side is ft"
        );
        assert_eq!(
            error("roll * vs > 1 deg"),
            "roll * vs > 1 deg\n^^^^^^^^^ only scaling by a plain number is supported"
        );
    }

    #[test]
    fn rejects_unknown_names_and_misused_types() {
        assert!(error("heigth < 1000 ft").ends_with("unknown parameter `heigth`"));
        assert!(error("height and ap.engaged").ends_with("expected a condition, found ft"));
        assert!(error("sqrt(height)").contains("unknown function `sqrt`"));
        assert!(error("height < 1000 ft )").ends_with("unexpected text after the expression"));
    }

    #[test]
    fn literals_carry_units() {
        assert_eq!(literal("30 deg"), Ok((30.0, Some(Unit::Degrees))));
        assert_eq!(literal("-10 kt"), Ok((-10.0, Some(Unit::Knots))));
        assert_eq!(literal("2 s"), Ok((2.0, Some(Unit::Seconds))));
        assert_eq!(literal("1.6"), Ok((1.6, None)));
        assert!(literal("height").is_err());
    }
}
