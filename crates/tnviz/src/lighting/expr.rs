//! Shading programs: functions from a page point to a colour, independent of
//! any backend.
//!
//! A program is a sequence of bindings followed by three channel
//! expressions.  Each binding may use the point, earlier bindings, and the
//! RGB of base colours, which stay parameters: the engine never knows them.
//! Programs are evaluated numerically (for tests and raster backends) or
//! compiled to PostScript calculator code (for PDF functional shadings),
//! where bindings live on the stack so shared parts are computed once.

/// A scalar expression.  Booleans are 1 and 0 when evaluated.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Const(f64),
    /// The page coordinates of the point.
    X,
    Y,
    /// A channel (0, 1, 2 for r, g, b) of a base colour's RGB.
    Param {
        slot: usize,
        channel: u8,
    },
    /// An earlier binding.
    Var(usize),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Abs(Box<Expr>),
    Sqrt(Box<Expr>),
    Min(Box<Expr>, Box<Expr>),
    Max(Box<Expr>, Box<Expr>),
    /// Cosine of an angle in degrees.
    Cos(Box<Expr>),
    /// atan2(y, x) in degrees, in [0, 360).
    Atan2(Box<Expr>, Box<Expr>),
    /// e to the power of the argument.
    Exp(Box<Expr>),
    /// A non-negative base to a power.
    Pow(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

/// Shorthand constructors.
pub fn c(x: f64) -> Expr {
    Expr::Const(x)
}

macro_rules! binary {
    ($($name:ident => $variant:ident),*) => {
        $(pub fn $name(a: Expr, b: Expr) -> Expr { Expr::$variant(Box::new(a), Box::new(b)) })*
    };
}
macro_rules! unary {
    ($($name:ident => $variant:ident),*) => {
        $(pub fn $name(a: Expr) -> Expr { Expr::$variant(Box::new(a)) })*
    };
}
binary!(add => Add, sub => Sub, mul => Mul, div => Div, min => Min, max => Max, pow => Pow, lt => Lt, and => And);
unary!(neg => Neg, abs => Abs, sqrt => Sqrt, exp => Exp);

// Angles: no lighting formula uses them now, but programs may.
#[allow(dead_code)]
pub fn atan2(y: Expr, x: Expr) -> Expr {
    Expr::Atan2(Box::new(y), Box::new(x))
}
#[allow(dead_code)]
pub fn cos(a: Expr) -> Expr {
    Expr::Cos(Box::new(a))
}

pub fn if_(cond: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(cond), Box::new(a), Box::new(b))
}

/// Clamp to [0, 1].
pub fn clamp01(a: Expr) -> Expr {
    min(max(a, c(0.0)), c(1.0))
}

/// Smoothstep of a value already in [0, 1]: 3t² − 2t³.
pub fn smooth(t: Expr) -> Expr {
    // Written with t twice; callers bind t first when it is not a variable.
    mul(mul(t.clone(), t.clone()), sub(c(3.0), mul(c(2.0), t)))
}

/// A shading program.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Program {
    pub binds: Vec<Expr>,
    pub out: [Option<Expr>; 3],
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind an expression and return the variable that refers to it.
    pub fn bind(&mut self, e: Expr) -> Expr {
        // Variables and constants need no binding of their own.
        if matches!(e, Expr::Var(_) | Expr::Const(_) | Expr::X | Expr::Y | Expr::Param { .. }) {
            return e;
        }
        self.binds.push(e);
        Expr::Var(self.binds.len() - 1)
    }

    pub fn output(&mut self, rgb: [Expr; 3]) {
        let [r, g, b] = rgb;
        self.out = [Some(r), Some(g), Some(b)];
    }

    /// Evaluate at a point; `param` gives the RGB channels of base colours.
    pub fn eval(&self, x: f64, y: f64, param: &dyn Fn(usize, u8) -> f64) -> [f64; 3] {
        let mut vars = Vec::with_capacity(self.binds.len());
        for e in &self.binds {
            let v = eval(e, x, y, &vars, param);
            vars.push(v);
        }
        let out = |k: usize| eval(self.out[k].as_ref().expect("program has no output"), x, y, &vars, param);
        [out(0), out(1), out(2)]
    }

    /// PostScript calculator code taking `x y` and leaving `r g b`.
    /// `param` writes the reference to a base colour's channel, for
    /// instance a TeX macro that the runtime expands to a number.
    ///
    /// Bindings live on the operand stack, and each one, like x and y, is
    /// removed right after its last use: PDF limits the stack of a
    /// calculator function to 100 entries.
    pub fn to_postscript(&self, param: &dyn Fn(usize, u8) -> String) -> String {
        let outputs: Vec<&Expr> =
            self.out.iter().map(|e| e.as_ref().expect("program has no output")).collect();
        // The step that uses each slot last; outputs are step `binds.len()`.
        let n = self.binds.len();
        let mut last = std::collections::HashMap::new();
        for (step, e) in self.binds.iter().enumerate() {
            last.insert(Slot::Var(step), step);
            e.visit(&mut |u| {
                last.insert(u, step);
            });
        }
        for e in &outputs {
            e.visit(&mut |u| {
                last.insert(u, n);
            });
        }
        let mut c = Compiler { out: String::new(), param, stack: vec![Slot::X, Slot::Y] };
        c.free(&last, usize::MAX);
        for (step, e) in self.binds.iter().enumerate() {
            let depth = c.stack.len();
            c.expr(e, depth);
            c.stack.push(Slot::Var(step));
            c.free(&last, step);
        }
        let below = c.stack.len();
        for (k, e) in outputs.iter().enumerate() {
            c.expr(e, below + k);
        }
        // Drop what is left under the result.
        if below > 0 {
            c.word(&format!("{} 3 roll", below + 3));
            for _ in 0..below {
                c.word("pop");
            }
        }
        c.out.trim_end().to_string()
    }
}

/// A value on the stack of compiled code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slot {
    X,
    Y,
    Var(usize),
}

impl Expr {
    /// Call `f` on every point coordinate and binding the expression uses.
    fn visit(&self, f: &mut dyn FnMut(Slot)) {
        match self {
            Expr::Const(_) | Expr::Param { .. } => {}
            Expr::X => f(Slot::X),
            Expr::Y => f(Slot::Y),
            Expr::Var(i) => f(Slot::Var(*i)),
            Expr::Neg(a) | Expr::Abs(a) | Expr::Sqrt(a) | Expr::Cos(a) | Expr::Exp(a) => a.visit(f),
            Expr::Add(a, b)
            | Expr::Sub(a, b)
            | Expr::Mul(a, b)
            | Expr::Div(a, b)
            | Expr::Min(a, b)
            | Expr::Max(a, b)
            | Expr::Atan2(a, b)
            | Expr::Pow(a, b)
            | Expr::Lt(a, b)
            | Expr::And(a, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::If(c, a, b) => {
                c.visit(f);
                a.visit(f);
                b.visit(f);
            }
        }
    }
}

fn eval(e: &Expr, x: f64, y: f64, vars: &[f64], param: &dyn Fn(usize, u8) -> f64) -> f64 {
    let ev = |e: &Expr| eval(e, x, y, vars, param);
    let bool_ = |b: bool| if b { 1.0 } else { 0.0 };
    match e {
        Expr::Const(v) => *v,
        Expr::X => x,
        Expr::Y => y,
        Expr::Param { slot, channel } => param(*slot, *channel),
        Expr::Var(i) => vars[*i],
        Expr::Add(a, b) => ev(a) + ev(b),
        Expr::Sub(a, b) => ev(a) - ev(b),
        Expr::Mul(a, b) => ev(a) * ev(b),
        Expr::Div(a, b) => ev(a) / ev(b),
        Expr::Neg(a) => -ev(a),
        Expr::Abs(a) => ev(a).abs(),
        Expr::Sqrt(a) => ev(a).sqrt(),
        Expr::Min(a, b) => ev(a).min(ev(b)),
        Expr::Max(a, b) => ev(a).max(ev(b)),
        Expr::Cos(a) => ev(a).to_radians().cos(),
        Expr::Atan2(y, x) => ev(y).atan2(ev(x)).to_degrees().rem_euclid(360.0),
        Expr::Exp(a) => ev(a).exp(),
        Expr::Pow(a, b) => ev(a).powf(ev(b)),
        Expr::Lt(a, b) => bool_(ev(a) < ev(b)),
        Expr::And(a, b) => bool_(ev(a) != 0.0 && ev(b) != 0.0),
        Expr::If(cond, a, b) => {
            if ev(cond) != 0.0 {
                ev(a)
            } else {
                ev(b)
            }
        }
    }
}

/// A number as PostScript reads it: plain decimal notation, no exponent.
/// Its magnitude must stay below 2³¹, since a number written without a
/// point is an integer in PDF.
pub fn ps_number(v: f64) -> String {
    debug_assert!(v.abs() < 2f64.powi(31), "{v} is out of range for PDF");
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

struct Compiler<'a> {
    out: String,
    param: &'a dyn Fn(usize, u8) -> String,
    /// What each stack entry holds, bottom first, between steps.
    stack: Vec<Slot>,
}

impl Compiler<'_> {
    /// Remove the slots whose last use is `step` (or that are never used,
    /// when `step` is `usize::MAX`).
    fn free(&mut self, last: &std::collections::HashMap<Slot, usize>, step: usize) {
        let mut k = self.stack.len();
        while k > 0 {
            k -= 1;
            let slot = self.stack[k];
            let done = match last.get(&slot) {
                Some(&s) => s == step,
                None => step == usize::MAX,
            };
            if done {
                let above = self.stack.len() - k;
                if above > 1 {
                    self.word(&format!("{above} -1 roll"));
                }
                self.word("pop");
                self.stack.remove(k);
            }
        }
    }

    /// The `index` operand that copies `slot`, with `depth` values on the
    /// stack.
    fn index_of(&self, slot: Slot, depth: usize) -> usize {
        let pos = self.stack.iter().position(|s| *s == slot).expect("a live slot");
        depth - pos - 1
    }

    fn word(&mut self, w: &str) {
        self.out.push_str(w);
        self.out.push(' ');
    }

    /// Code that pushes the value of `e`, with `depth` values already on
    /// the stack.
    fn expr(&mut self, e: &Expr, depth: usize) {
        let bin = |c: &mut Self, a: &Expr, b: &Expr, op: &str| {
            c.expr(a, depth);
            c.expr(b, depth + 1);
            c.word(op);
        };
        match e {
            Expr::Const(v) => self.word(&ps_number(*v)),
            Expr::X => self.word(&format!("{} index", self.index_of(Slot::X, depth))),
            Expr::Y => self.word(&format!("{} index", self.index_of(Slot::Y, depth))),
            Expr::Param { slot, channel } => {
                let p = (self.param)(*slot, *channel);
                self.word(&p);
            }
            Expr::Var(i) => self.word(&format!("{} index", self.index_of(Slot::Var(*i), depth))),
            Expr::Add(a, b) => bin(self, a, b, "add"),
            Expr::Sub(a, b) => bin(self, a, b, "sub"),
            Expr::Mul(a, b) => bin(self, a, b, "mul"),
            Expr::Div(a, b) => bin(self, a, b, "div"),
            Expr::Min(a, b) => bin(self, a, b, "2 copy gt { exch } if pop"),
            Expr::Max(a, b) => bin(self, a, b, "2 copy lt { exch } if pop"),
            Expr::Pow(a, b) => bin(self, a, b, "exp"),
            Expr::Lt(a, b) => bin(self, a, b, "lt"),
            Expr::And(a, b) => bin(self, a, b, "and"),
            Expr::Atan2(y, x) => bin(self, y, x, "atan"),
            Expr::Neg(a) => {
                self.expr(a, depth);
                self.word("neg");
            }
            Expr::Abs(a) => {
                self.expr(a, depth);
                self.word("abs");
            }
            Expr::Sqrt(a) => {
                self.expr(a, depth);
                self.word("sqrt");
            }
            Expr::Cos(a) => {
                self.expr(a, depth);
                self.word("cos");
            }
            Expr::Exp(a) => {
                self.word(&ps_number(std::f64::consts::E));
                self.expr(a, depth + 1);
                self.word("exp");
            }
            Expr::If(cond, a, b) => {
                self.expr(cond, depth);
                self.word("{");
                self.expr(a, depth);
                self.word("} {");
                self.expr(b, depth);
                self.word("} ifelse");
            }
        }
    }
}

// ---- A PostScript calculator interpreter, for checking compiled code -------

#[cfg(test)]
pub(crate) mod ps {
    #[derive(Clone, Debug)]
    enum Tok {
        Num(f64),
        Op(String),
        Block(Vec<Tok>),
    }

    #[derive(Clone, Debug)]
    enum Val {
        Num(f64),
        Bool(bool),
        Block(Vec<Tok>),
    }

    fn parse(words: &mut std::iter::Peekable<std::str::SplitWhitespace<'_>>) -> Vec<Tok> {
        let mut out = Vec::new();
        while let Some(w) = words.next() {
            match w {
                "{" => out.push(Tok::Block(parse(words))),
                "}" => return out,
                _ => out.push(match w.parse() {
                    Ok(v) => Tok::Num(v),
                    Err(_) => Tok::Op(w.to_string()),
                }),
            }
        }
        out
    }

    /// PDF's limit on the operand stack of a calculator function.
    const STACK_LIMIT: usize = 100;

    fn run(code: &[Tok], s: &mut Vec<Val>) {
        let num = |v: Val| match v {
            Val::Num(x) => x,
            other => panic!("expected a number, found {other:?}"),
        };
        for t in code {
            assert!(s.len() <= STACK_LIMIT, "the stack exceeds {STACK_LIMIT} entries");
            match t {
                Tok::Num(v) => s.push(Val::Num(*v)),
                Tok::Block(b) => s.push(Val::Block(b.clone())),
                Tok::Op(op) => {
                    macro_rules! bin {
                        ($f:expr) => {{
                            let b = num(s.pop().unwrap());
                            let a = num(s.pop().unwrap());
                            s.push($f(a, b));
                        }};
                    }
                    match op.as_str() {
                        "add" => bin!(|a, b| Val::Num(a + b)),
                        "sub" => bin!(|a, b| Val::Num(a - b)),
                        "mul" => bin!(|a, b| Val::Num(a * b)),
                        "div" => bin!(|a, b| Val::Num(a / b)),
                        "exp" => bin!(|a: f64, b| Val::Num(a.powf(b))),
                        "lt" => bin!(|a, b| Val::Bool(a < b)),
                        "gt" => bin!(|a, b| Val::Bool(a > b)),
                        "atan" => bin!(|y: f64, x| Val::Num(y.atan2(x).to_degrees().rem_euclid(360.0))),
                        "neg" | "abs" | "sqrt" | "cos" => {
                            let a = num(s.pop().unwrap());
                            s.push(Val::Num(match op.as_str() {
                                "neg" => -a,
                                "abs" => a.abs(),
                                "sqrt" => a.sqrt(),
                                _ => a.to_radians().cos(),
                            }));
                        }
                        "and" => {
                            let (Some(Val::Bool(b)), Some(Val::Bool(a))) = (s.pop(), s.pop()) else {
                                panic!("and")
                            };
                            s.push(Val::Bool(a && b));
                        }
                        "pop" => {
                            s.pop().unwrap();
                        }
                        "exch" => {
                            let n = s.len();
                            s.swap(n - 1, n - 2);
                        }
                        "index" => {
                            let k = num(s.pop().unwrap()) as usize;
                            let v = s[s.len() - 1 - k].clone();
                            s.push(v);
                        }
                        "copy" => {
                            let k = num(s.pop().unwrap()) as usize;
                            let n = s.len();
                            let top: Vec<Val> = s[n - k..].to_vec();
                            s.extend(top);
                        }
                        "roll" => {
                            let j = num(s.pop().unwrap()) as i64;
                            let n = num(s.pop().unwrap()) as usize;
                            let len = s.len();
                            let part = &mut s[len - n..];
                            part.rotate_right(j.rem_euclid(n as i64) as usize);
                        }
                        "if" => {
                            let (Some(Val::Block(b)), Some(Val::Bool(c))) = (s.pop(), s.pop()) else {
                                panic!("if")
                            };
                            if c {
                                run(&b, s);
                            }
                        }
                        "ifelse" => {
                            let (Some(Val::Block(no)), Some(Val::Block(yes)), Some(Val::Bool(c))) =
                                (s.pop(), s.pop(), s.pop())
                            else {
                                panic!("ifelse")
                            };
                            run(if c { &yes } else { &no }, s);
                        }
                        other => panic!("unknown operator {other}"),
                    }
                }
            }
        }
    }

    /// Run calculator code on `x y`; the result is the final stack.
    pub fn eval(code: &str, x: f64, y: f64) -> Vec<f64> {
        let toks = parse(&mut code.split_whitespace().peekable());
        let mut s = vec![Val::Num(x), Val::Num(y)];
        run(&toks, &mut s);
        s.into_iter()
            .map(|v| match v {
                Val::Num(x) => x,
                other => panic!("left {other:?} on the stack"),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_code_matches_evaluation() {
        let mut p = Program::new();
        let d = p.bind(sqrt(add(mul(Expr::X, Expr::X), mul(Expr::Y, Expr::Y))));
        let a = p.bind(atan2(Expr::Y, Expr::X));
        let inside = p.bind(lt(d.clone(), c(1.0)));
        let t = p.bind(clamp01(sub(c(1.0), d.clone())));
        let s = p.bind(smooth(t.clone()));
        let base = Expr::Param { slot: 0, channel: 0 };
        p.output([
            if_(inside.clone(), mul(s.clone(), base.clone()), c(0.25)),
            add(mul(cos(a.clone()), c(0.5)), c(0.5)),
            min(max(exp(neg(d.clone())), pow(abs(Expr::X), c(0.5))), c(0.9)),
        ]);
        let code = p.to_postscript(&|_, _| "0.4".into());
        for (x, y) in [(0.3, 0.2), (-0.7, 0.5), (1.5, -0.4), (0.01, -0.02), (-2.0, -2.0)] {
            let direct = p.eval(x, y, &|_, _| 0.4);
            let stack = ps::eval(&code, x, y);
            assert_eq!(stack.len(), 3, "{code}");
            for k in 0..3 {
                assert!((direct[k] - stack[k]).abs() < 1e-5, "channel {k} at ({x}, {y}): {code}");
            }
        }
    }

    #[test]
    fn numbers_have_no_exponent() {
        assert_eq!(ps_number(1e-7), "0");
        assert_eq!(ps_number(2.5), "2.5");
        assert_eq!(ps_number(-3.0), "-3");
        assert_eq!(ps_number(123456789.0), "123456789");
        assert!(std::panic::catch_unwind(|| ps_number(1e12)).is_err());
    }
}
