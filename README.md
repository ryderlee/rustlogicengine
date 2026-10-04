# rustlogicengine

A small, dependency-free rule-set engine for time-series rules.

A rule is a tree of `All` / `Any` / `None` groups. Leaves compare series values,
for example `rsi(14) crosses_above 30` on closed bars. The crate knows nothing
about markets, indicator names, or what `"1h"` means. Your code plugs values in
through one trait.

## Why

General rule crates compare values at a single instant. Trading and monitoring
rules need more:

- **Crosses on closed bars**, using the previous bar's value (Pine `ta.crossover` semantics, equality on the previous bar counts).
- **Previous values**: turning up/down, rising/falling for N bars.
- **Multi-timeframe new-bar events**: an event on a slower frame fires only on the step where that frame closed a new bar, so it does not repeat on every fast-frame step.

## Install

```toml
[dependencies]
rustlogicengine = "0.1"
# JSON support:
# rustlogicengine = { version = "0.1", features = ["serde"] }
```

Core has zero dependencies. The `serde` feature adds `serde` and `serde_json`.
Core builds on Rust 1.70 (verified). The `serde` feature needs whatever the
current `serde` / `serde_json` need.

## Example

```rust
use rustlogicengine::*;
use std::collections::HashMap;

// The host owns the data. Values are stored newest first.
struct Host {
    data: HashMap<(&'static str, &'static str), Vec<f64>>,
    new_bar: Vec<&'static str>,
}

impl Values for Host {
    fn value(&self, s: &SeriesId, f: &Frame, back: usize) -> Option<f64> {
        let key = self.data.keys().find(|k| k.0 == s.0 && k.1 == f.0)?;
        self.data[key].get(back).copied()
    }
    fn is_new_bar(&self, f: &Frame) -> bool {
        self.new_bar.iter().any(|n| *n == f.0)
    }
}

// all( rsi(14) < 70,  none( ema(20) crosses_below ema(50) on 1h ) )
let rule = Node::Group(Group {
    mode: Mode::All,
    children: vec![
        Node::Cond(Condition {
            left: Operand::Series(SeriesId("rsi(14)".into())),
            op: Op::Lt,
            right: Some(Operand::Number(70.0)),
            frame: None,
        }),
        Node::Group(Group {
            mode: Mode::None,
            children: vec![Node::Cond(Condition {
                left: Operand::Series(SeriesId("ema(20)".into())),
                op: Op::CrossesBelow,
                right: Some(Operand::Series(SeriesId("ema(50)".into()))),
                frame: Some(Frame("1h".into())),
            })],
        }),
    ],
});

// Always validate untrusted rules before eval.
validate(&rule, Limits::default()).unwrap();

let host = Host {
    data: HashMap::from([
        (("rsi(14)", "1d"), vec![55.0]),
        (("ema(20)", "1h"), vec![9.0, 11.0]),
        (("ema(50)", "1h"), vec![10.0, 10.0]),
    ]),
    new_bar: vec!["1h"],
};
let daily = Frame("1d".into());
// The 1h cross below fired, so the `none` group is false.
assert!(!eval(&rule, &daily, &host));

// Which (series, frame) pairs must the host load?
let needed: Vec<_> = series(&rule, &daily).collect();
assert_eq!(needed.len(), 3);
```

The same rule as JSON (feature `serde`):

```rust,ignore
let rule: Node = serde_json::from_str(r#"{
  "match": "all",
  "conditions": [
    {"left": "rsi(14)", "op": "<", "right": 70},
    {"match": "none", "conditions": [
      {"left": "ema(20)", "op": "crosses_below", "right": "ema(50)", "timeframe": "1h"}
    ]}
  ]
}"#)?;
let canonical = serde_json::to_string(&rule)?; // fixed key order, absent keys omitted
```

JSON shape: a node is a group if it has a `match` key (`all`, `any`, `none`) with a
`conditions` array. Otherwise it is a condition with `left`, `op`, optional `right`
and optional `timeframe`. `left` is a string (series) or number. `right` is a number
or a string (series). For `rising` / `falling`, `right` is a positive integer N.
`serde_json::from_str::<Node>` stops at 63 nested groups (serde_json recursion limit 128);
use `Node::from_json(&str)` to load every tree `validate` accepts (64 groups by default).
`turning_up` / `turning_down` take no `right`. Ops: `>`, `<`, `crosses_above`,
`crosses_below`, `turning_up`, `turning_down`, `rising`, `falling`. Unknown keys are
rejected. Parsing does not check structure (empty groups, missing `right`); call
`validate`.

## Semantics

`x(k)` is the left value `k` closed bars ago, `y(k)` the right value.
Any needed value missing makes the condition false.

| Op | True when |
|---|---|
| `Gt` | `x(0) > y(0)` |
| `Lt` | `x(0) < y(0)` |
| `CrossesAbove` | `x(0) > y(0)` and `x(1) <= y(1)` |
| `CrossesBelow` | `x(0) < y(0)` and `x(1) >= y(1)` |
| `TurningUp` | `x(2) > x(1)` and `x(1) < x(0)` (strict V, flat bottoms do not count) |
| `TurningDown` | `x(2) < x(1)` and `x(1) > x(0)` |
| `Rising(N)` | `x(i) > x(i+1)` for every `i` in `0..N` (needs N+1 values, any tie fails) |
| `Falling(N)` | `x(i) < x(i+1)` for every `i` in `0..N` |

A `Number` right operand is a constant at every `back`.

Groups: `All` = every child true, `Any` = at least one true, `None` = no child true.

**Frame gating.** An event op (everything except `Gt` / `Lt`) on a frame other than
the default frame is false unless `is_new_bar(frame)` is true. Values are not even
read. `Gt` / `Lt` are never gated. Conditions on the default frame are never gated.

**`None` and missing data.** A missing value, or a gated (stale) event, makes a
condition false. So inside a `None` group it counts as "not true", and the group
is **true**. If you want "no data" to block a rule, do not put the condition under
`None`; use `All` with the positive form instead.

## Validate before eval

`eval` does not validate. `validate(root, Limits)` checks: no empty groups; `right`
present for `>`, `<`, crosses and absent for turning/rising/falling; `Rising`/`Falling`
N in `1..=max_lookback`; finite numbers; nesting depth <= `max_depth` (root group is
depth 1); total conditions <= `max_conditions`. Defaults: 64 / 1000 / 50. Errors carry
a path such as `2.1.3` (1-based child indices; `root` for the root). Validation stops
descending at `max_depth`, so it is safe on hostile input. Always call it on
untrusted rules; `eval` recurses once per nesting level. `Clone`, `Debug` and
`PartialEq` are also recursive, so run them only on validated trees (`Drop` is iterative).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
