#![allow(dead_code)]
use rustlogicengine::*;
use std::collections::{HashMap, HashSet};

/// Test host: values stored newest-first per (series, frame).
#[derive(Default)]
pub struct Host {
    pub data: HashMap<(String, String), Vec<f64>>,
    pub new_bars: HashSet<String>,
}

impl Host {
    pub fn with(mut self, s: &str, f: &str, newest_first: &[f64]) -> Self {
        self.data
            .insert((s.into(), f.into()), newest_first.to_vec());
        self
    }
    pub fn new_bar(mut self, f: &str) -> Self {
        self.new_bars.insert(f.into());
        self
    }
}

impl Values for Host {
    fn value(&self, s: &SeriesId, f: &Frame, back: usize) -> Option<f64> {
        self.data
            .get(&(s.0.clone(), f.0.clone()))?
            .get(back)
            .copied()
    }
    fn is_new_bar(&self, f: &Frame) -> bool {
        self.new_bars.contains(&f.0)
    }
}

pub fn ser(s: &str) -> Operand {
    Operand::Series(SeriesId(s.into()))
}
pub fn num(n: f64) -> Operand {
    Operand::Number(n)
}
pub fn frame(f: &str) -> Frame {
    Frame(f.into())
}
pub fn cond(l: Operand, op: Op, r: Option<Operand>, f: Option<&str>) -> Node {
    Node::Cond(Condition {
        left: l,
        op,
        right: r,
        frame: f.map(frame),
    })
}
/// `a` vs constant `n` on the default frame.
pub fn vs(op: Op, n: f64) -> Node {
    cond(ser("a"), op, Some(num(n)), None)
}
/// Single-series (no right) condition on the default frame.
pub fn solo(op: Op) -> Node {
    cond(ser("a"), op, None, None)
}
pub fn grp(mode: Mode, children: Vec<Node>) -> Node {
    Node::Group(Group { mode, children })
}
