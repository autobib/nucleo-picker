use std::num::NonZero;

use nucleo::{
    Config,
    pattern::{CaseMatching, Normalization},
};

use super::{layout::ListLayout, *};

use crate::{match_engine::MatchEngine, render::StrRenderer};

use Action::*;

enum Action<'a> {
    Incr(u32),
    Decr(u32),
    Reset,
    Update(&'a [&'static str]),
    Resize(u16),
}

struct MatchListTester {
    engine: MatchEngine<&'static str, StrRenderer>,
    layout: ListLayout,
    config: MatchListConfig,
}

/// A view into a [`Matcher`] at a given point in time.
#[derive(Debug, Clone, PartialEq)]
struct LayoutView<'a> {
    /// The number of lines to render for each item beginning below the screen index and rendering
    /// downwards.
    pub below: &'a [usize],
    /// The number of lines to render for each item beginning above the screen index and rendering
    /// upwards.
    pub above: &'a [usize],
}

impl MatchListTester {
    fn init_inner(size: u16, max_padding: u16, reversed: bool) -> Self {
        let mc = MatchListConfig {
            scroll_padding: max_padding,
            reversed,
            ..MatchListConfig::default()
        };

        let engine = MatchEngine::new(
            Config::DEFAULT,
            nucleo::MatchListConfig {
                sort_results: false,
                reverse_items: false,
            },
            NonZero::new(1),
            StrRenderer.into(),
            CaseMatching::Smart,
            Normalization::Smart,
        );
        let mut layout = ListLayout::new();
        layout.resize(engine.snapshot(), size, &mc);

        Self {
            engine,
            layout,
            config: mc,
        }
    }

    fn init(size: u16, max_padding: u16) -> Self {
        Self::init_inner(size, max_padding, false)
    }

    fn init_rev(size: u16, max_padding: u16) -> Self {
        Self::init_inner(size, max_padding, true)
    }

    fn update(&mut self, lc: Action) {
        match lc {
            Action::Incr(incr) => {
                self.layout
                    .selection_incr(self.engine.snapshot(), incr, &self.config);
            }
            Action::Decr(decr) => {
                self.layout
                    .selection_decr(self.engine.snapshot(), decr, &self.config);
            }
            Action::Reset => {
                self.layout.reset(self.engine.snapshot(), &self.config);
            }
            Action::Update(items) => {
                self.engine.restart();
                self.engine.injector().push_batch(items.iter().copied());
                while self.engine.update(5).matching {}
                self.layout
                    .update_items(self.engine.snapshot(), &self.config);
            }
            Action::Resize(sz) => {
                self.layout.resize(self.engine.snapshot(), sz, &self.config);
            }
        }
    }

    fn view(&self) -> LayoutView<'_> {
        LayoutView {
            above: &self.layout.above,
            below: &self.layout.below,
        }
    }
}

macro_rules! assert_layout {
    ($lt:ident, $op:expr_2021, $below:expr_2021, $above:expr_2021) => {
        $lt.update($op);
        assert_eq!(
            $lt.view(),
            LayoutView {
                below: $below,
                above: $above
            }
        );
    };
}

#[test]
fn injector_lifetime_updates_status() {
    let mut lt = MatchListTester::init(1, 0);

    let injector = lt.engine.injector();
    let status = lt.engine.update(0);
    assert!(status.injecting);

    drop(injector);
    let mut status = lt.engine.update(0);
    assert!(!status.injecting);

    while status.matching {
        status = lt.engine.update(5);
    }
}

#[test]
fn basic() {
    let mut lt = MatchListTester::init(6, 2);
    assert_layout!(lt, Update(&["12\n34", "ab"]), &[2], &[1]);
    assert_layout!(lt, Incr(1), &[1, 2], &[]);
    assert_layout!(lt, Reset, &[2], &[1]);
    assert_layout!(lt, Incr(1), &[1, 2], &[]);
    assert_layout!(lt, Incr(1), &[1, 2], &[]);

    let mut lt = MatchListTester::init_rev(6, 2);
    assert_layout!(lt, Update(&["12\n34", "ab"]), &[2, 1], &[]);
    assert_layout!(lt, Incr(1), &[1], &[2]);
    assert_layout!(lt, Reset, &[2, 1], &[]);
    assert_layout!(lt, Incr(1), &[1], &[2]);
    assert_layout!(lt, Incr(1), &[1], &[2]);
}

#[test]
fn size_and_item_edge_cases() {
    let mut lt = MatchListTester::init(6, 2);
    assert_layout!(lt, Incr(1), &[], &[]);
    assert_layout!(lt, Decr(1), &[], &[]);
    assert_layout!(lt, Update(&[]), &[], &[]);
    assert_layout!(lt, Resize(0), &[], &[]);
    assert_layout!(lt, Resize(1), &[], &[]);
    assert_layout!(lt, Update(&["a"]), &[1], &[]);
    assert_layout!(lt, Resize(0), &[], &[]);

    let mut lt = MatchListTester::init_rev(6, 2);
    assert_layout!(lt, Incr(1), &[], &[]);
    assert_layout!(lt, Decr(1), &[], &[]);
    assert_layout!(lt, Update(&[]), &[], &[]);
    assert_layout!(lt, Resize(0), &[], &[]);
    assert_layout!(lt, Resize(1), &[], &[]);
    assert_layout!(lt, Update(&["a"]), &[1], &[]);
    assert_layout!(lt, Resize(0), &[], &[]);
}

#[test]
fn zero_height_navigation() {
    check_zero_height_navigation(false);
}

#[test]
fn zero_height_navigation_reversed() {
    check_zero_height_navigation(true);
}

fn check_zero_height_navigation(reversed: bool) {
    for initial_size in [0, 3] {
        let mut lt = MatchListTester::init_inner(initial_size, 2, reversed);
        lt.update(Update(&["a", "b", "c"]));
        assert_layout!(lt, Resize(0), &[], &[]);

        for (requested, expected, changed) in [
            (1, 1, true),
            (2, 2, true),
            (1, 1, true),
            (1, 1, false),
            (u32::MAX, 2, true),
            (u32::MAX, 2, false),
            (0, 0, true),
            (0, 0, false),
            (1, 1, true),
        ] {
            assert_eq!(
                lt.layout
                    .set_selection(lt.engine.snapshot(), requested, &lt.config),
                changed
            );
            assert_eq!(lt.layout.selection(lt.engine.snapshot()), Some(expected));
            assert_eq!(
                lt.view(),
                LayoutView {
                    below: &[],
                    above: &[],
                }
            );
        }

        assert_layout!(lt, Resize(1), &[1], &[]);
        assert_eq!(lt.layout.selection(lt.engine.snapshot()), Some(1));
        assert_layout!(lt, Resize(3), &[1, 1], &[1]);
        assert_eq!(lt.layout.selection(lt.engine.snapshot()), Some(1));
    }
}

#[test]
fn small() {
    let mut lt = MatchListTester::init(5, 1);
    assert_layout!(lt, Update(&["12", "a\nb"]), &[1], &[2]);
    assert_layout!(lt, Incr(1), &[2, 1], &[]);
    assert_layout!(lt, Decr(1), &[1], &[2]);

    let mut lt = MatchListTester::init_rev(5, 1);
    assert_layout!(lt, Update(&["12", "a\nb"]), &[1, 2], &[]);
    assert_layout!(lt, Incr(1), &[2], &[1]);
    assert_layout!(lt, Decr(1), &[1, 2], &[]);
}

#[test]
fn item_change() {
    let mut lt = MatchListTester::init(4, 1);
    assert_layout!(
        lt,
        Update(&["0\n1\n2\n3\n4\n5", "0\n1", "0\n1", "0\n1\n2\n3"]),
        &[3],
        &[1]
    );
    assert_layout!(lt, Incr(1), &[2, 1], &[1]);
    assert_layout!(lt, Update(&["0\n0", "1"]), &[1, 2], &[]);
    assert_layout!(lt, Update(&["0\n0", "1\n1"]), &[2, 1], &[]);
    assert_layout!(lt, Update(&["0"]), &[1], &[]);
    assert_layout!(lt, Update(&["0\n0\n0\n0", "1", "2", "3"]), &[3], &[1]);
    assert_layout!(lt, Incr(1), &[1, 2], &[1]);
    assert_layout!(lt, Update(&["0", "1", "2", "3"]), &[1, 1], &[1, 1]);
    assert_layout!(lt, Update(&[]), &[], &[]);

    let mut lt = MatchListTester::init_rev(4, 1);
    assert_layout!(
        lt,
        Update(&["0\n1\n2\n3\n4\n5", "0\n1", "0\n1", "0\n1\n2\n3"]),
        &[4],
        &[]
    );
    assert_layout!(lt, Incr(1), &[2], &[2]);
    assert_layout!(lt, Update(&["0\n0", "1"]), &[1], &[2]);
    assert_layout!(lt, Update(&["0\n0", "1\n1"]), &[2], &[2]);
    assert_layout!(lt, Update(&["0"]), &[1], &[]);
    assert_layout!(lt, Update(&["0\n0\n0\n0", "1", "2", "3"]), &[4], &[]);
    assert_layout!(lt, Incr(1), &[1, 1], &[2]);
    assert_layout!(lt, Update(&["0", "1", "2", "3"]), &[1, 1, 1], &[1]);
    assert_layout!(lt, Update(&[]), &[], &[]);
}

#[test]
fn rev_incl_selection() {
    let mut lt = MatchListTester::init_rev(5, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5\n5\n5"]),
        &[1, 1, 1, 1, 1],
        &[]
    );
    assert_layout!(lt, Incr(4), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Incr(1), &[3], &[1, 1]);
}

#[test]
fn resize_basic() {
    let mut lt = MatchListTester::init(5, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5"]),
        &[1],
        &[1, 1, 1, 1]
    );
    assert_layout!(lt, Incr(5), &[1, 1, 1, 1], &[]);
    assert_layout!(lt, Resize(3), &[1, 1], &[]);
    assert_layout!(lt, Resize(5), &[1, 1, 1, 1], &[]);

    let mut lt = MatchListTester::init_rev(5, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5"]),
        &[1, 1, 1, 1, 1],
        &[]
    );
    assert_layout!(lt, Incr(5), &[1], &[1, 1, 1]);
    assert_layout!(lt, Resize(3), &[1], &[1]);
}

#[test]
fn resize_with_padding_change() {
    let mut lt = MatchListTester::init(10, 2);
    assert_layout!(
        lt,
        Update(&["0\n0\n0", "1", "2\n2", "3\n3\n3\n3"]),
        &[3],
        &[1, 2, 4]
    );
    assert_layout!(lt, Resize(4), &[3], &[1]);
    assert_layout!(lt, Incr(2), &[2, 1], &[1]);
    assert_layout!(lt, Resize(8), &[2, 1, 3], &[2]);
    assert_layout!(lt, Resize(4), &[2, 1], &[1]);

    let mut lt = MatchListTester::init_rev(10, 2);
    assert_layout!(
        lt,
        Update(&["0\n0\n0", "1", "2\n2", "3\n3\n3\n3"]),
        &[3, 1, 2, 4],
        &[]
    );
    assert_layout!(lt, Resize(4), &[3, 1], &[]);
    assert_layout!(lt, Incr(2), &[2], &[1, 1]);
    assert_layout!(lt, Resize(8), &[2, 2], &[1, 3]);
    assert_layout!(lt, Resize(4), &[2], &[1, 1]);
}

#[test]
fn item_alignment() {
    let mut lt = MatchListTester::init(20, 3);
    assert_layout!(lt, Update(&["0\n1\n2\n", "0\n1", "0\n1"]), &[4], &[2, 2]);
    assert_layout!(lt, Incr(2), &[2, 2, 4], &[]);
    assert_layout!(lt, Decr(1), &[2, 4], &[2]);

    let mut lt = MatchListTester::init_rev(20, 3);
    assert_layout!(lt, Update(&["0\n1\n2\n", "0\n1", "0\n1"]), &[4, 2, 2], &[]);
    assert_layout!(lt, Incr(2), &[2], &[2, 4]);
    assert_layout!(lt, Decr(1), &[2, 2], &[4]);
}

#[test]
fn scrolldown() {
    let mut lt = MatchListTester::init(8, 2);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
        &[1],
        &[1, 1, 1, 1, 1, 1, 1]
    );
    assert_layout!(lt, Incr(100), &[1, 1, 1, 1, 1, 1], &[]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1], &[1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Decr(3), &[1, 1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1], &[1, 1, 1, 1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1], &[1, 1, 1, 1, 1, 1, 1]);

    let mut lt = MatchListTester::init_rev(8, 2);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
        &[1, 1, 1, 1, 1, 1, 1, 1],
        &[]
    );
    assert_layout!(lt, Incr(100), &[1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(3), &[1, 1, 1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1, 1, 1], &[1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1, 1, 1, 1], &[]);
}

#[test]
fn scrollback() {
    let mut lt = MatchListTester::init(5, 1);
    assert_layout!(
        lt,
        Update(&["12\n34", "ab", "c", "d", "e", "f\ng"]),
        &[2],
        &[1, 1, 1]
    );
    assert_layout!(lt, Incr(3), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Incr(1), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Incr(1), &[2, 1, 1], &[]);
    assert_layout!(lt, Decr(1), &[1, 1], &[2]);
    assert_layout!(lt, Incr(1), &[2, 1, 1], &[]);
    assert_layout!(lt, Decr(2), &[1, 1], &[1, 2]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[2], &[1, 1, 1]);

    let mut lt = MatchListTester::init_rev(5, 1);
    assert_layout!(
        lt,
        Update(&["12\n34", "ab", "c", "d", "e", "f\ng"]),
        &[2, 1, 1, 1],
        &[]
    );
    assert_layout!(lt, Incr(3), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Incr(1), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Incr(1), &[2], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 2], &[1, 1]);
    assert_layout!(lt, Incr(1), &[2], &[1, 1, 1]);
    assert_layout!(lt, Decr(2), &[1, 1, 2], &[1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Decr(1), &[2, 1, 1, 1], &[]);
}

#[test]
fn multiline_jitter() {
    let mut lt = MatchListTester::init(12, 3);
    assert_layout!(
        lt,
        Update(&["a", "b", "0\n1\n2\n3", "0\n1", "0\n1"]),
        &[1],
        &[1, 4, 2, 2]
    );
    assert_layout!(lt, Incr(1), &[1, 1], &[4, 2, 2]);
    assert_layout!(lt, Incr(1), &[4, 1, 1], &[2, 2]);
    assert_layout!(lt, Incr(1), &[2, 4, 1, 1], &[2]);
    assert_layout!(lt, Incr(1), &[2, 2, 4, 1], &[]);
    assert_layout!(lt, Decr(1), &[2, 4, 1], &[2]);
    assert_layout!(lt, Decr(1), &[4, 1], &[2, 2]);
    assert_layout!(lt, Incr(1), &[2, 4, 1], &[2]);

    let mut lt = MatchListTester::init_rev(10, 3);
    assert_layout!(
        lt,
        Update(&["a", "b", "0\n1\n2\n3", "0\n1", "0\n1"]),
        &[1, 1, 4, 2, 2],
        &[]
    );
    assert_layout!(lt, Incr(1), &[1, 4, 2, 2], &[1]);
    assert_layout!(lt, Incr(1), &[4, 2, 2], &[1, 1]);
    assert_layout!(lt, Incr(1), &[2, 2], &[4, 1, 1]);
    assert_layout!(lt, Incr(1), &[2], &[2, 4]);
    assert_layout!(lt, Decr(1), &[2, 2], &[4]);
    assert_layout!(lt, Decr(1), &[4, 2, 2], &[1, 1]);
    assert_layout!(lt, Incr(1), &[2, 2], &[4, 1, 1]);
}

#[test]
fn scroll_mid() {
    let mut lt = MatchListTester::init(5, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5", "6", "7"]),
        &[1],
        &[1, 1, 1, 1]
    );

    assert_layout!(lt, Incr(4), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Decr(2), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Incr(1), &[1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Resize(7), &[1, 1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Incr(2), &[1, 1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Incr(1), &[1, 1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(2), &[1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Incr(20), &[1, 1, 1, 1, 1, 1], &[]);

    let mut lt = MatchListTester::init_rev(5, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5", "6", "7"]),
        &[1, 1, 1, 1, 1],
        &[]
    );
    assert_layout!(lt, Incr(4), &[1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(2), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Incr(1), &[1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1], &[1]);
    assert_layout!(lt, Resize(7), &[1, 1, 1, 1, 1], &[1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1, 1, 1, 1], &[1]);
}
