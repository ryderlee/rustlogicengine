#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::fmt;

#[cfg(feature = "serde")]
mod serde_impl;

/// Opaque series identifier, e.g. `"rsi(14)"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SeriesId(pub String);

/// Opaque timeframe identifier, e.g. `"1h"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Frame(pub String);

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Series(SeriesId),
    /// A constant at every `back`.
    Number(f64),
}

/// Comparison operator. Rising/Falling carry the number of strict steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Gt,
    Lt,
    CrossesAbove,
    CrossesBelow,
    TurningUp,
    TurningDown,
    Rising(u32),
    Falling(u32),
}

impl Op {
    /// True for every op except `Gt` and `Lt`. Event ops fire on a change,
    /// so on a non-default frame they are only evaluated on a new bar.
    pub fn is_event(&self) -> bool {
        !matches!(self, Op::Gt | Op::Lt)
    }
}

/// A leaf: `left op right`, optionally on a specific frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub left: Operand,
    pub op: Op,
    pub right: Option<Operand>,
    pub frame: Option<Frame>,
}

/// How a group combines its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    All,
    Any,
    /// No child is true (NOT any).
    None,
}

/// A group of nodes. An empty group is invalid.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub mode: Mode,
    pub children: Vec<Node>,
}

/// Iterative drop: a hostile 100k-deep tree must not overflow the stack when freed.
impl Drop for Group {
    fn drop(&mut self) {
        let mut stack = std::mem::take(&mut self.children);
        while let Some(n) = stack.pop() {
            if let Node::Group(mut g) = n {
                stack.append(&mut g.children);
            }
        }
    }
}

/// A rule tree node.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Group(Group),
    Cond(Condition),
}

/// Host-supplied data.
pub trait Values {
    /// Value of `series` on `frame`, `back` closed bars ago (0 = latest closed).
    /// `None` = not enough data.
    fn value(&self, series: &SeriesId, frame: &Frame, back: usize) -> Option<f64>;
    /// True when `frame` closed a new bar at this evaluation step.
    fn is_new_bar(&self, frame: &Frame) -> bool;
}

/// Evaluate a rule. Does not validate; see the crate docs.
pub fn eval(root: &Node, default_frame: &Frame, v: &impl Values) -> bool {
    match root {
        Node::Group(g) => match g.mode {
            Mode::All => g.children.iter().all(|c| eval(c, default_frame, v)),
            Mode::Any => g.children.iter().any(|c| eval(c, default_frame, v)),
            Mode::None => !g.children.iter().any(|c| eval(c, default_frame, v)),
        },
        Node::Cond(c) => eval_cond(c, default_frame, v).unwrap_or(false),
    }
}

/// `None` = a needed value was missing.
fn eval_cond(c: &Condition, df: &Frame, v: &impl Values) -> Option<bool> {
    let frame = c.frame.as_ref().unwrap_or(df);
    if c.op.is_event() && frame != df && !v.is_new_bar(frame) {
        return Some(false);
    }
    let get = |o: &Operand, k: usize| match o {
        Operand::Number(n) => Some(*n),
        Operand::Series(s) => v.value(s, frame, k),
    };
    let x = |k| get(&c.left, k);
    let y = |k| get(c.right.as_ref()?, k);
    Some(match c.op {
        Op::Gt => x(0)? > y(0)?,
        Op::Lt => x(0)? < y(0)?,
        Op::CrossesAbove => x(0)? > y(0)? && x(1)? <= y(1)?,
        Op::CrossesBelow => x(0)? < y(0)? && x(1)? >= y(1)?,
        Op::TurningUp => x(2)? > x(1)? && x(1)? < x(0)?,
        Op::TurningDown => x(2)? < x(1)? && x(1)? > x(0)?,
        Op::Rising(n) => steps(n, |i| Some(x(i)? > x(i + 1)?))?,
        Op::Falling(n) => steps(n, |i| Some(x(i)? < x(i + 1)?))?,
    })
}

/// All of `n` steps hold; any missing value gives `None` (condition false).
fn steps(n: u32, f: impl Fn(usize) -> Option<bool>) -> Option<bool> {
    for i in 0..n as usize {
        if !f(i)? {
            return Some(false);
        }
    }
    Some(true)
}

/// Every `(series, frame)` the rule reads (left and right, frame resolved to
/// the condition's or the default), de-duplicated, in order of first appearance.
pub fn series<'a>(
    root: &'a Node,
    default_frame: &'a Frame,
) -> impl Iterator<Item = (&'a SeriesId, &'a Frame)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        match n {
            Node::Group(g) => stack.extend(g.children.iter().rev()),
            Node::Cond(c) => {
                let f = c.frame.as_ref().unwrap_or(default_frame);
                for o in std::iter::once(&c.left).chain(c.right.as_ref()) {
                    if let Operand::Series(s) = o {
                        if seen.insert((s, f)) {
                            out.push((s, f));
                        }
                    }
                }
            }
        }
    }
    out.into_iter()
}

/// Structural limits for [`validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_depth: usize,
    pub max_conditions: usize,
    pub max_lookback: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: 64,
            max_conditions: 1000,
            max_lookback: 50,
        }
    }
}

/// Why a rule is invalid, and where. `path` is dotted 1-based child indices
/// (`"root"`, `"2.1.3"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for RuleError {}

fn err(path: &str, message: impl Into<String>) -> Result<(), RuleError> {
    Err(RuleError {
        path: path.into(),
        message: message.into(),
    })
}

/// Check structure and limits. Iterative, and stops descending as soon as
/// depth exceeds `max_depth`, so it is safe on hostile input.
pub fn validate(root: &Node, limits: Limits) -> Result<(), RuleError> {
    let mut conditions = 0usize;
    // (node, path, depth of the group containing it + own depth if a group)
    let mut stack: Vec<(&Node, String, usize)> = vec![(root, "root".into(), 1)];
    while let Some((n, path, depth)) = stack.pop() {
        match n {
            Node::Group(g) => {
                if depth > limits.max_depth {
                    return err(&path, format!("nesting deeper than {}", limits.max_depth));
                }
                if g.children.is_empty() {
                    return err(&path, "empty group");
                }
                for (i, c) in g.children.iter().enumerate().rev() {
                    let p = if path == "root" {
                        (i + 1).to_string()
                    } else {
                        format!("{path}.{}", i + 1)
                    };
                    stack.push((c, p, depth + 1));
                }
            }
            Node::Cond(c) => {
                conditions += 1;
                if conditions > limits.max_conditions {
                    return err(
                        "root",
                        format!("more than {} conditions", limits.max_conditions),
                    );
                }
                check_cond(c, &path, limits)?;
            }
        }
    }
    Ok(())
}

fn check_cond(c: &Condition, path: &str, limits: Limits) -> Result<(), RuleError> {
    let finite = |o: &Operand| !matches!(o, Operand::Number(n) if !n.is_finite());
    if !finite(&c.left) || !c.right.as_ref().map_or(true, finite) {
        return err(path, "number must be finite");
    }
    match c.op {
        Op::Gt | Op::Lt | Op::CrossesAbove | Op::CrossesBelow if c.right.is_none() => {
            err(path, "right operand required")
        }
        Op::TurningUp | Op::TurningDown | Op::Rising(_) | Op::Falling(_) if c.right.is_some() => {
            err(path, "right operand not allowed")
        }
        Op::Rising(n) | Op::Falling(n) if n == 0 || n > limits.max_lookback => {
            err(path, format!("N must be 1..={}", limits.max_lookback))
        }
        _ => Ok(()),
    }
}
