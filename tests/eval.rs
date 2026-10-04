mod common;
use common::*;
use rustlogicengine::*;

fn d() -> Frame {
    frame("1d")
}
fn host(a: &[f64]) -> Host {
    Host::default().with("a", "1d", a)
}
fn ev(n: &Node, h: &Host) -> bool {
    eval(n, &d(), h)
}
fn t() -> Node {
    vs(Op::Gt, 0.0)
}
fn f() -> Node {
    vs(Op::Lt, 0.0)
}
fn pos() -> Host {
    host(&[1.0])
}

// ---- groups ----
#[test]
fn all_any_none_truth_tables() {
    let h = pos();
    for (mode, kids, want) in [
        (Mode::All, vec![t(), t()], true),
        (Mode::All, vec![t(), f()], false),
        (Mode::Any, vec![f(), t()], true),
        (Mode::Any, vec![f(), f()], false),
        (Mode::None, vec![f(), f()], true),
        (Mode::None, vec![f(), t()], false),
        (Mode::None, vec![t(), t()], false),
    ] {
        assert_eq!(ev(&grp(mode, kids), &h), want, "{mode:?}");
    }
}

#[test]
fn three_level_nesting() {
    // all( any(f, none(t)), none(any(f, f)), t ) -> all(f, true, t) = false
    let a = grp(
        Mode::All,
        vec![
            grp(Mode::Any, vec![f(), grp(Mode::None, vec![t()])]),
            grp(Mode::None, vec![grp(Mode::Any, vec![f(), f()])]),
            t(),
        ],
    );
    assert!(!ev(&a, &pos()));
    // flip the inner none(t) to none(f): any(f, true) -> true overall
    let b = grp(
        Mode::All,
        vec![
            grp(Mode::Any, vec![f(), grp(Mode::None, vec![f()])]),
            grp(Mode::None, vec![grp(Mode::Any, vec![f(), f()])]),
            t(),
        ],
    );
    assert!(ev(&b, &pos()));
}

#[test]
fn bare_condition_root() {
    assert!(ev(&t(), &pos()));
}

// ---- level ops ----
#[test]
fn gt_lt_strict() {
    let h = host(&[5.0]);
    assert!(ev(&vs(Op::Gt, 4.0), &h));
    assert!(!ev(&vs(Op::Gt, 5.0), &h));
    assert!(ev(&vs(Op::Lt, 6.0), &h));
    assert!(!ev(&vs(Op::Lt, 5.0), &h));
}

#[test]
fn series_vs_series_right() {
    let h = Host::default()
        .with("a", "1d", &[3.0])
        .with("b", "1d", &[2.0]);
    assert!(ev(&cond(ser("a"), Op::Gt, Some(ser("b")), None), &h));
}

#[test]
fn number_left_operand_works() {
    let h = host(&[3.0]);
    assert!(ev(&cond(num(5.0), Op::Gt, Some(ser("a")), None), &h));
}

// ---- crosses ----
#[test]
fn crosses_above_counts_equality_on_previous_bar() {
    // Pine ta.crossover: x>y now and x[1]<=y[1]. Touching then rising is a cross.
    let n = vs(Op::CrossesAbove, 10.0);
    assert!(ev(&n, &host(&[11.0, 10.0])));
    assert!(ev(&n, &host(&[11.0, 9.0])));
    assert!(!ev(&n, &host(&[10.0, 9.0])), "x(0)==y(0) is not above");
    assert!(!ev(&n, &host(&[12.0, 11.0])), "already above");
}

#[test]
fn crosses_below_counts_equality_on_previous_bar() {
    let n = vs(Op::CrossesBelow, 10.0);
    assert!(ev(&n, &host(&[9.0, 10.0])));
    assert!(ev(&n, &host(&[9.0, 11.0])));
    assert!(!ev(&n, &host(&[10.0, 11.0])));
    assert!(!ev(&n, &host(&[8.0, 9.0])));
}

#[test]
fn crosses_uses_right_series_history() {
    // right is a series: y(1) must be read at back=1, not y(0).
    let h = Host::default()
        .with("a", "1d", &[5.0, 3.0])
        .with("b", "1d", &[4.0, 3.0]);
    assert!(ev(
        &cond(ser("a"), Op::CrossesAbove, Some(ser("b")), None),
        &h
    ));
    let h = Host::default()
        .with("a", "1d", &[5.0, 5.0])
        .with("b", "1d", &[4.0, 9.0]);
    assert!(ev(
        &cond(ser("a"), Op::CrossesAbove, Some(ser("b")), None),
        &h
    ));
}

#[test]
fn crosses_needs_two_values() {
    assert!(!ev(&vs(Op::CrossesAbove, 10.0), &host(&[11.0])));
}

// ---- turning ----
#[test]
fn turning_up_strict_v() {
    assert!(ev(&solo(Op::TurningUp), &host(&[3.0, 1.0, 2.0])));
    assert!(
        !ev(&solo(Op::TurningUp), &host(&[3.0, 1.0, 1.0])),
        "flat bottom"
    );
    assert!(
        !ev(&solo(Op::TurningUp), &host(&[3.0, 2.0, 2.0])),
        "flat bottom"
    );
    assert!(!ev(&solo(Op::TurningUp), &host(&[1.0, 2.0, 3.0])));
    assert!(
        !ev(&solo(Op::TurningUp), &host(&[3.0, 1.0])),
        "needs 3 values"
    );
}

#[test]
fn turning_down_strict_peak() {
    assert!(ev(&solo(Op::TurningDown), &host(&[1.0, 3.0, 2.0])));
    assert!(
        !ev(&solo(Op::TurningDown), &host(&[1.0, 3.0, 3.0])),
        "flat top"
    );
    assert!(!ev(&solo(Op::TurningDown), &host(&[3.0, 2.0, 1.0])));
}

// ---- rising / falling ----
#[test]
fn rising_n_strict_steps() {
    let n = solo(Op::Rising(3));
    assert!(ev(&n, &host(&[4.0, 3.0, 2.0, 1.0])));
    assert!(!ev(&n, &host(&[4.0, 3.0, 3.0, 1.0])), "any tie fails");
    assert!(!ev(&n, &host(&[4.0, 3.0, 2.0])), "needs N+1 values");
    assert!(ev(&solo(Op::Rising(1)), &host(&[2.0, 1.0])));
}

#[test]
fn falling_n_strict_steps() {
    let n = solo(Op::Falling(2));
    assert!(ev(&n, &host(&[1.0, 2.0, 3.0])));
    assert!(!ev(&n, &host(&[1.0, 2.0, 2.0])));
    assert!(!ev(&n, &host(&[1.0, 2.0])));
}

// ---- missing ----
#[test]
fn missing_values_make_condition_false() {
    assert!(!ev(&vs(Op::Gt, 0.0), &Host::default()));
    let h = host(&[1.0]);
    assert!(!ev(&cond(ser("a"), Op::Gt, Some(ser("missing")), None), &h));
}

#[test]
fn none_group_with_missing_value_is_true() {
    // Documented consequence: a missing value is a false condition, so "none true" holds.
    let n = grp(
        Mode::None,
        vec![cond(ser("zzz"), Op::Gt, Some(num(0.0)), None)],
    );
    assert!(ev(&n, &pos()));
}

// ---- frame gating ----
fn h4() -> Host {
    // 1h series crossing above 10 at the latest bar.
    Host::default().with("a", "1h", &[11.0, 9.0])
}
fn on_1h(op: Op, r: Option<Operand>) -> Node {
    cond(ser("a"), op, r, Some("1h"))
}

#[test]
fn event_on_other_frame_gated_without_new_bar() {
    let n = on_1h(Op::CrossesAbove, Some(num(10.0)));
    assert!(!ev(&n, &h4()), "stale event must not fire again");
    assert!(ev(&n, &h4().new_bar("1h")));
}

#[test]
fn level_op_on_other_frame_never_gated() {
    assert!(ev(&on_1h(Op::Gt, Some(num(10.0))), &h4()));
    assert!(ev(&on_1h(Op::Lt, Some(num(99.0))), &h4()));
}

#[test]
fn default_frame_event_never_gated() {
    let h = Host::default().with("a", "1d", &[11.0, 9.0]); // no new_bar flag
    assert!(ev(&vs(Op::CrossesAbove, 10.0), &h));
    let explicit = cond(ser("a"), Op::CrossesAbove, Some(num(10.0)), Some("1d"));
    assert!(ev(&explicit, &h));
}

#[test]
fn gated_event_does_not_read_values() {
    struct Panics;
    impl Values for Panics {
        fn value(&self, _: &SeriesId, _: &Frame, _: usize) -> Option<f64> {
            panic!("read while gated")
        }
        fn is_new_bar(&self, _: &Frame) -> bool {
            false
        }
    }
    for op in [Op::TurningUp, Op::Rising(2)] {
        assert!(!eval(&on_1h(op, None), &d(), &Panics));
    }
}

#[test]
fn none_group_with_stale_child_is_true() {
    // Documented consequence: a gated (stale) event is false, so none() holds.
    let n = grp(Mode::None, vec![on_1h(Op::CrossesAbove, Some(num(10.0)))]);
    assert!(ev(&n, &h4()));
    assert!(!ev(&n, &h4().new_bar("1h")));
}

// ---- series() ----
#[test]
fn series_dedups_and_resolves_frames_in_order() {
    let n = grp(
        Mode::All,
        vec![
            cond(ser("a"), Op::Gt, Some(ser("b")), None),
            grp(
                Mode::Any,
                vec![
                    cond(ser("a"), Op::Gt, Some(num(1.0)), None),
                    cond(ser("a"), Op::Gt, Some(ser("c")), Some("1h")),
                    cond(num(1.0), Op::Lt, Some(ser("b")), None),
                    cond(ser("b"), Op::Lt, Some(num(2.0)), Some("1h")),
                ],
            ),
        ],
    );
    let df = d();
    let got: Vec<(String, String)> = series(&n, &df)
        .map(|(s, f)| (s.0.clone(), f.0.clone()))
        .collect();
    let want: Vec<(String, String)> = [
        ("a", "1d"),
        ("b", "1d"),
        ("a", "1h"),
        ("c", "1h"),
        ("b", "1h"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(got, want);
}

#[test]
fn op_is_event() {
    assert!(!Op::Gt.is_event() && !Op::Lt.is_event());
    for op in [
        Op::CrossesAbove,
        Op::CrossesBelow,
        Op::TurningUp,
        Op::TurningDown,
        Op::Rising(1),
        Op::Falling(1),
    ] {
        assert!(op.is_event());
    }
}

#[test]
fn turning_requires_strict_last_step() {
    // newest-first: x[0]=2, x[1]=1, x[2]=1 -> tie on the x[1]/x[0] side must not fire.
    assert!(!ev(&solo(Op::TurningUp), &host(&[1.0, 1.0, 2.0])));
    assert!(!ev(&solo(Op::TurningDown), &host(&[3.0, 3.0, 1.0])));
}
