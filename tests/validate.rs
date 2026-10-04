mod common;
use common::*;
use rustlogicengine::*;

fn lim() -> Limits {
    Limits::default()
}
fn err(n: &Node, l: Limits) -> RuleError {
    validate(n, l).unwrap_err()
}
fn ok_leaf() -> Node {
    vs(Op::Gt, 1.0)
}

#[test]
fn defaults() {
    let l = Limits::default();
    assert_eq!(
        (l.max_depth, l.max_conditions, l.max_lookback),
        (64, 1000, 50)
    );
}

#[test]
fn valid_tree_passes() {
    let n = grp(
        Mode::All,
        vec![ok_leaf(), grp(Mode::None, vec![solo(Op::TurningUp)])],
    );
    assert!(validate(&n, lim()).is_ok());
    assert!(validate(&ok_leaf(), lim()).is_ok());
}

#[test]
fn empty_group_path() {
    let e = err(&grp(Mode::All, vec![]), lim());
    assert_eq!(e.path, "root");
    let n = grp(
        Mode::All,
        vec![
            ok_leaf(),
            grp(Mode::Any, vec![ok_leaf(), grp(Mode::Any, vec![])]),
        ],
    );
    assert_eq!(err(&n, lim()).path, "2.2");
    assert!(err(&n, lim()).to_string().starts_with("2.2: "));
}

#[test]
fn right_required_for_binary_ops() {
    for op in [Op::Gt, Op::Lt, Op::CrossesAbove, Op::CrossesBelow] {
        let n = grp(Mode::All, vec![ok_leaf(), solo(op)]);
        assert_eq!(err(&n, lim()).path, "2");
    }
}

#[test]
fn right_forbidden_for_unary_ops() {
    for op in [
        Op::TurningUp,
        Op::TurningDown,
        Op::Rising(2),
        Op::Falling(2),
    ] {
        let n = grp(
            Mode::All,
            vec![grp(Mode::Any, vec![ok_leaf(), ok_leaf(), vs(op, 1.0)])],
        );
        assert_eq!(err(&n, lim()).path, "1.3");
    }
}

#[test]
fn rising_falling_bounds() {
    for op in [
        Op::Rising(0),
        Op::Falling(0),
        Op::Rising(51),
        Op::Falling(51),
    ] {
        assert_eq!(err(&solo(op), lim()).path, "root");
    }
    assert!(validate(&solo(Op::Rising(50)), lim()).is_ok());
    assert!(validate(&solo(Op::Falling(1)), lim()).is_ok());
}

#[test]
fn non_finite_numbers_rejected() {
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let n = grp(Mode::All, vec![vs(Op::Gt, x)]);
        assert_eq!(err(&n, lim()).path, "1");
        let n = cond(num(x), Op::Gt, Some(ser("a")), None);
        assert!(validate(&n, lim()).is_err());
    }
}

fn nest(depth: usize) -> Node {
    let mut n = ok_leaf();
    for _ in 0..depth {
        n = grp(Mode::All, vec![n]);
    }
    n
}

#[test]
fn depth_boundary() {
    // root group is depth 1; a leaf under it is not a group so does not add depth.
    let l = Limits {
        max_depth: 3,
        ..lim()
    };
    assert!(validate(&nest(3), l).is_ok());
    assert!(validate(&nest(4), l).is_err());
    assert!(validate(
        &ok_leaf(),
        Limits {
            max_depth: 1,
            ..lim()
        }
    )
    .is_ok());
}

#[test]
fn condition_count_boundary() {
    let l = Limits {
        max_conditions: 3,
        ..lim()
    };
    let mk = |n| grp(Mode::Any, (0..n).map(|_| ok_leaf()).collect());
    assert!(validate(&mk(3), l).is_ok());
    assert_eq!(err(&mk(4), l).path, "root");
}

#[test]
fn hostile_deep_tree_is_rejected_without_overflow() {
    // WHY: untrusted JSON can nest arbitrarily; validate must bail out at max_depth.
    let n = nest(100_000);
    assert!(validate(&n, lim()).is_err());
    drop(n); // Group's Drop must be iterative too
}

#[test]
fn first_error_in_document_order_is_reported() {
    // WHY: hosts show one error; it must be the first one a reader meets.
    let n = grp(Mode::All, vec![solo(Op::Gt), ok_leaf(), solo(Op::Gt)]);
    assert_eq!(err(&n, lim()).path, "1");
    // pre-order: an error in an earlier subtree beats a shallower later one
    let n = grp(
        Mode::All,
        vec![grp(Mode::Any, vec![ok_leaf(), solo(Op::Gt)]), solo(Op::Gt)],
    );
    assert_eq!(err(&n, lim()).path, "1.2");
}

#[test]
fn deep_paths_and_depth_error_path() {
    let n = grp(
        Mode::All,
        vec![
            ok_leaf(),
            grp(
                Mode::Any,
                vec![grp(Mode::All, vec![ok_leaf(), ok_leaf(), solo(Op::Gt)])],
            ),
        ],
    );
    assert_eq!(err(&n, lim()).path, "2.1.3");
    let l = Limits {
        max_depth: 3,
        ..lim()
    };
    let e = err(&nest(4), l);
    assert_eq!(e.path, "1.1.1");
    assert!(e.message.contains('3'), "{}", e.message);
}

#[test]
fn custom_max_lookback_is_honoured() {
    let l = Limits {
        max_lookback: 5,
        ..lim()
    };
    assert!(validate(&solo(Op::Rising(5)), l).is_ok());
    let e = err(&solo(Op::Falling(6)), l);
    assert!(e.message.contains('5'), "{}", e.message);
    assert!(err(&solo(Op::Rising(6)), l).message.contains('5'));
}
