//! JSON shape: groups `{"match","conditions"}`, conditions `{"left","op","right","timeframe"}`.
use crate::*;
use serde::de::{Deserializer, Error as _};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

impl Serialize for Operand {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Operand::Series(id) => s.serialize_str(&id.0),
            Operand::Number(n) => s.serialize_f64(*n),
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Group(g) => {
                let mut m = s.serialize_map(Some(2))?;
                let mode = match g.mode {
                    Mode::All => "all",
                    Mode::Any => "any",
                    Mode::None => "none",
                };
                m.serialize_entry("match", mode)?;
                m.serialize_entry("conditions", &g.children)?;
                m.end()
            }
            Node::Cond(c) => {
                let mut m = s.serialize_map(None)?;
                m.serialize_entry("left", &c.left)?;
                let (op, n) = match c.op {
                    Op::Gt => (">", None),
                    Op::Lt => ("<", None),
                    Op::CrossesAbove => ("crosses_above", None),
                    Op::CrossesBelow => ("crosses_below", None),
                    Op::TurningUp => ("turning_up", None),
                    Op::TurningDown => ("turning_down", None),
                    Op::Rising(n) => ("rising", Some(n)),
                    Op::Falling(n) => ("falling", Some(n)),
                };
                m.serialize_entry("op", op)?;
                match (n, &c.right) {
                    (Some(n), _) => m.serialize_entry("right", &n)?,
                    (None, Some(r)) => m.serialize_entry("right", r)?,
                    _ => {}
                }
                if let Some(f) = &c.frame {
                    m.serialize_entry("timeframe", &f.0)?;
                }
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        node(&Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

impl Node {
    /// Parse JSON, accepting every tree `validate` accepts under `Limits::default()`.
    /// `serde_json::from_str::<Node>` stops at 63 nested groups (its recursion limit of
    /// 128 counts a group as object + array); this lifts that limit but first bounds
    /// raw bracket nesting to what a default-limit tree needs, so hostile input
    /// cannot overflow the stack.
    pub fn from_json(s: &str) -> Result<Node, serde_json::Error> {
        let max = 2 * Limits::default().max_depth + 1;
        let (mut depth, mut in_str, mut esc) = (0usize, false, false);
        for b in s.bytes() {
            if in_str {
                match (esc, b) {
                    (true, _) => esc = false,
                    (_, b'\\') => esc = true,
                    (_, b'"') => in_str = false,
                    _ => {}
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > max {
                        return Err(serde_json::Error::custom("nesting too deep"));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        let mut de = serde_json::Deserializer::from_str(s);
        de.disable_recursion_limit();
        let v = Value::deserialize(&mut de)?;
        de.end()?;
        node(&v).map_err(serde_json::Error::custom)
    }
}

fn only_keys(m: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown key `{k}`")),
        None => Ok(()),
    }
}

fn node(v: &Value) -> Result<Node, String> {
    let m = v.as_object().ok_or("node must be an object")?;
    if let Some(mode) = m.get("match") {
        only_keys(m, &["match", "conditions"])?;
        let mode = match mode.as_str() {
            Some("all") => Mode::All,
            Some("any") => Mode::Any,
            Some("none") => Mode::None,
            _ => return Err("`match` must be \"all\", \"any\" or \"none\"".into()),
        };
        let kids = m
            .get("conditions")
            .and_then(Value::as_array)
            .ok_or("`conditions` must be an array")?;
        let children = kids.iter().map(node).collect::<Result<Vec<_>, _>>()?;
        return Ok(Node::Group(Group { mode, children }));
    }
    only_keys(m, &["left", "op", "right", "timeframe"])?;
    let left = match m.get("left") {
        Some(Value::String(s)) => Operand::Series(SeriesId(s.clone())),
        Some(Value::Number(n)) => Operand::Number(n.as_f64().ok_or("bad `left` number")?),
        _ => return Err("`left` must be a string or number".into()),
    };
    let opname = m
        .get("op")
        .and_then(Value::as_str)
        .ok_or("`op` must be a string")?;
    let right = m.get("right");
    let steps = || -> Result<u32, String> {
        right
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n > 0)
            .ok_or_else(|| format!("`right` for `{opname}` must be a positive integer"))
    };
    let (op, right) = match opname {
        "rising" => (Op::Rising(steps()?), None),
        "falling" => (Op::Falling(steps()?), None),
        "turning_up" | "turning_down" => {
            if right.is_some() {
                return Err(format!("`{opname}` takes no `right`"));
            }
            (
                if opname == "turning_up" {
                    Op::TurningUp
                } else {
                    Op::TurningDown
                },
                None,
            )
        }
        ">" | "<" | "crosses_above" | "crosses_below" => {
            let op = match opname {
                ">" => Op::Gt,
                "<" => Op::Lt,
                "crosses_above" => Op::CrossesAbove,
                _ => Op::CrossesBelow,
            };
            let r = match right {
                None => None,
                Some(Value::String(s)) => Some(Operand::Series(SeriesId(s.clone()))),
                Some(Value::Number(n)) => {
                    Some(Operand::Number(n.as_f64().ok_or("bad `right` number")?))
                }
                _ => return Err("`right` must be a string or number".into()),
            };
            (op, r)
        }
        other => return Err(format!("unknown op `{other}`")),
    };
    let frame = match m.get("timeframe") {
        None => None,
        Some(Value::String(s)) => Some(Frame(s.clone())),
        _ => return Err("`timeframe` must be a string".into()),
    };
    Ok(Node::Cond(Condition {
        left,
        op,
        right,
        frame,
    }))
}
