//! Checks that every specification rule maps to at least one test.
//!
//! See principle P-6 in `docs/test-strategy.md`. The baseline file lists the
//! rules with no test. The list must only shrink.

use lark_test::coverage;

#[test]
fn the_uncovered_rule_list_does_not_grow() {
    let root = lark_test::repository_root();
    let scan = match coverage::scan(&root) {
        Ok(scan) => scan,
        Err(error) => panic!("cannot scan for rule coverage: {error}"),
    };

    assert!(
        !scan.rules.is_empty(),
        "the specification states no rule, so the scan is broken"
    );

    if lark_test::bless_mode() {
        if let Err(error) = coverage::write_baseline(&root, &scan) {
            panic!("cannot write the baseline: {error}");
        }
        return;
    }

    if let Err(report) = coverage::check(&root, &scan) {
        panic!("{report}");
    }
}

#[test]
fn the_scan_finds_the_rules_from_every_chapter() {
    let root = lark_test::repository_root();
    let Ok(scan) = coverage::scan(&root) else {
        panic!("cannot scan for rule coverage");
    };
    // One rule from each area, to prove that the scan reads every chapter.
    for rule in [
        "S-1", "L-6", "T-1", "M-8", "O-2", "G-10", "N-3", "I-1", "C-1", "X-5",
    ] {
        assert!(
            scan.rules.contains(rule),
            "the scan did not find rule {rule}"
        );
    }
}
