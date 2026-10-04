#![cfg(feature = "serde")]
mod common;
use common::*;
use rustlogicengine::*;

fn parse(s: &str) -> Result<Node, serde_json::Error> {
    serde_json::from_str(s)
}

const SAMPLE: &str = r#"{"match":"all","conditions":[{"left":"rsi(14)","op":"<","right":30.0},{"match":"none","conditions":[{"left":"ema(20)","op":"crosses_above","right":"ema(50)","timeframe":"1h"},{"left":"a","op":"rising","right":3},{"left":"a","op":"turning_down"}]}]}"#;

#[test]
fn parses_expected_model() {
    let n = parse(SAMPLE).unwrap();
    let want = grp(
        Mode::All,
        vec![
            cond(ser("rsi(14)"), Op::Lt, Some(num(30.0)), None),
            grp(
                Mode::None,
                vec![
                    cond(
                        ser("ema(20)"),
                        Op::CrossesAbove,
                        Some(ser("ema(50)")),
                        Some("1h"),
                    ),
                    cond(ser("a"), Op::Rising(3), None, None),
                    cond(ser("a"), Op::TurningDown, None, None),
                ],
            ),
        ],
    );
    assert_eq!(n, want);
}

#[test]
fn round_trip_and_canonical_bytes_stable() {
    let n = parse(SAMPLE).unwrap();
    let s1 = serde_json::to_string(&n).unwrap();
    assert_eq!(s1, SAMPLE, "sample is already canonical");
    assert_eq!(parse(&s1).unwrap(), n);
    // Non-canonical key order / whitespace normalises to the same bytes.
    let messy =
        r#"{ "conditions":[{"timeframe":"1h","right":"b","op":">","left":"a"}], "match":"any" }"#;
    let canon =
        r#"{"match":"any","conditions":[{"left":"a","op":">","right":"b","timeframe":"1h"}]}"#;
    assert_eq!(
        serde_json::to_string(&parse(messy).unwrap()).unwrap(),
        canon
    );
}

#[test]
fn number_left_and_float_right() {
    let n = parse(r#"{"left":5,"op":">","right":2.5}"#).unwrap();
    assert_eq!(n, cond(num(5.0), Op::Gt, Some(num(2.5)), None));
    assert_eq!(
        serde_json::to_string(&n).unwrap(),
        r#"{"left":5.0,"op":">","right":2.5}"#
    );
    assert_eq!(parse(&serde_json::to_string(&n).unwrap()).unwrap(), n);
}

#[test]
fn every_op_string() {
    for (s, op, r) in [
        (">", Op::Gt, true),
        ("<", Op::Lt, true),
        ("crosses_above", Op::CrossesAbove, true),
        ("crosses_below", Op::CrossesBelow, true),
        ("turning_up", Op::TurningUp, false),
        ("turning_down", Op::TurningDown, false),
    ] {
        let j = if r {
            format!(r#"{{"left":"a","op":"{s}","right":1.0}}"#)
        } else {
            format!(r#"{{"left":"a","op":"{s}"}}"#)
        };
        let n = parse(&j).unwrap();
        assert_eq!(n, cond(ser("a"), op, r.then(|| num(1.0)), None));
        assert_eq!(serde_json::to_string(&n).unwrap(), j);
    }
    let n = parse(r#"{"left":"a","op":"falling","right":2}"#).unwrap();
    assert_eq!(n, cond(ser("a"), Op::Falling(2), None, None));
}

#[test]
fn unknown_key_names_the_key() {
    let e = parse(r#"{"left":"a","op":">","right":1,"bogus":1}"#)
        .unwrap_err()
        .to_string();
    assert!(e.contains("bogus"), "{e}");
    let e = parse(r#"{"match":"all","conditions":[],"extra":1}"#)
        .unwrap_err()
        .to_string();
    assert!(e.contains("extra"), "{e}");
}

#[test]
fn bad_op_and_bad_match_rejected() {
    assert!(parse(r#"{"left":"a","op":"=="}"#).is_err());
    assert!(parse(r#"{"match":"xor","conditions":[]}"#).is_err());
}

#[test]
fn rising_needs_positive_integer() {
    for r in ["2.5", "0", "-1", r#""3""#, "null"] {
        let j = format!(r#"{{"left":"a","op":"rising","right":{r}}}"#);
        assert!(parse(&j).is_err(), "{j}");
    }
    assert!(parse(r#"{"left":"a","op":"rising"}"#).is_err());
}

#[test]
fn unary_ops_reject_right_and_binary_ops_need_it() {
    assert!(parse(r#"{"left":"a","op":"turning_up","right":1}"#).is_err());
    // Binary op without right parses (validate() flags it), not a parse error.
    assert!(parse(r#"{"left":"a","op":">"}"#).is_ok());
}

#[test]
fn missing_required_keys() {
    assert!(parse(r#"{"op":">","right":1}"#).is_err());
    assert!(parse(r#"{"left":"a","right":1}"#).is_err());
    assert!(parse(r#"{"match":"all"}"#).is_err());
}

fn nest_json(groups: usize) -> String {
    format!(
        r#"{}{{"left":"a","op":">","right":1.0}}{}"#,
        r#"{"match":"all","conditions":["#.repeat(groups),
        "]}".repeat(groups)
    )
}

#[test]
fn max_depth_tree_round_trips_through_json() {
    // validate() accepts 64 nested groups; from_json must load what validate accepts.
    let d = Limits::default().max_depth;
    let s = nest_json(d);
    let n = Node::from_json(&s).expect("depth-64 tree parses");
    assert!(validate(&n, Limits::default()).is_ok());
    assert_eq!(serde_json::to_string(&n).unwrap(), s);
    assert_eq!(
        Node::from_json(&nest_json(63)).unwrap(),
        parse(&nest_json(63)).unwrap()
    );
}

#[test]
fn hostile_json_nesting_fails_cleanly() {
    let s = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    assert!(Node::from_json(&s).is_err());
    assert!(parse(&s).is_err());
    let s = nest_json(100_000);
    assert!(Node::from_json(&s).is_err());
    // 65 groups parse but validate rejects, so the limit stays validate's job.
    let n = Node::from_json(&nest_json(65));
    assert!(n.is_err() || validate(&n.unwrap(), Limits::default()).is_err());
}
