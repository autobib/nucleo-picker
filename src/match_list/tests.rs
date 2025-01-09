use std::sync::Arc;

use nucleo::{Config, Nucleo, Utf32String};

use super::*;

use crate::render::StrRenderer;

use Action::*;

enum Action<'a> {
    Incr(u32),
    Decr(u32),
    Reset,
    Update(&'a [&'static str]),
    Resize(u16),
}

fn reset(nc: &mut Nucleo<&'static str>, items: &[&'static str]) {
    nc.restart(true);
    let injector = nc.injector();
    for item in items {
        injector.push(item, |item, cols| {
            cols[0] = Utf32String::from(*item);
        });
    }

    while nc.tick(5).running {}
}

struct MatchListTester {
    match_list: MatchList<&'static str, StrRenderer>,
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
        let nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);
        let mut mc = MatchListConfig::default();
        mc.scroll_padding = max_padding;
        mc.reversed = reversed;

        let mut match_list = MatchList::new(mc, Config::DEFAULT, nc, StrRenderer.into());
        match_list.resize(size);

        Self { match_list }
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
                self.match_list.selection_incr(incr);
            }
            Action::Decr(decr) => {
                self.match_list.selection_decr(decr);
            }
            Action::Reset => {
                self.match_list.reset();
            }
            Action::Update(items) => {
                reset(&mut self.match_list.nucleo, items);
                self.match_list.update_items();
            }
            Action::Resize(sz) => {
                self.match_list.resize(sz);
            }
        }
    }

    fn view(&self) -> LayoutView {
        LayoutView {
            above: &self.match_list.above,
            below: &self.match_list.below,
        }
    }

    #[allow(unused)]
    fn debug_items(&self) {
        for item in self.match_list.nucleo.snapshot().matched_items(..).rev() {
            println!("* * * * * *\n{}", item.data);
        }
    }

    #[allow(unused)]
    fn debug_items_rev(&self) {
        for item in self.match_list.nucleo.snapshot().matched_items(..) {
            println!("* * * * * *\n{}", item.data);
        }
    }
}

macro_rules! assert_layout {
    ($lt:ident, $op:expr, $below:expr, $above:expr) => {
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
