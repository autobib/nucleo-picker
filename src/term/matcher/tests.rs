use std::sync::Arc;

use nucleo::{Config, Nucleo, Utf32String};

use super::*;

use LayoutChange::*;

enum LayoutChange<'a> {
    Incr(u32),
    Decr(u32),
    Reset,
    Update(&'a [&'static str]),
    Resize(u16, u16, u16),
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

struct LayoutTester {
    nc: Nucleo<&'static str>,
    layout: Matcher,
}

impl LayoutTester {
    fn init(size: u16, padding_bottom: u16, padding_top: u16) -> Self {
        let nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);
        let layout = Matcher::new(size, padding_bottom, padding_top);

        Self { nc, layout }
    }

    fn update(&mut self, lc: LayoutChange) {
        match lc {
            LayoutChange::Incr(incr) => {
                self.layout.selection_incr(incr, self.nc.snapshot());
            }
            LayoutChange::Decr(decr) => {
                self.layout.selection_decr(decr, self.nc.snapshot());
            }
            LayoutChange::Reset => {
                self.layout.reset(self.nc.snapshot());
            }
            LayoutChange::Update(items) => {
                reset(&mut self.nc, items);
                self.layout.update_items(self.nc.snapshot());
            }
            LayoutChange::Resize(sz, bot, top) => {
                self.layout.resize(sz, bot, top, self.nc.snapshot());
            }
        }
    }

    fn view(&self) -> LayoutView {
        self.layout.view()
    }

    #[allow(unused)]
    fn debug_items(&self) {
        for item in self.nc.snapshot().matched_items(..).rev() {
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
    let mut lt = LayoutTester::init(6, 2, 3);
    assert_layout!(lt, Update(&["12\n34", "ab"]), &[2], &[1]);
    assert_layout!(lt, Incr(1), &[1, 2], &[]);
    assert_layout!(lt, Reset, &[2], &[1]);
    assert_layout!(lt, Incr(1), &[1, 2], &[]);
}

#[test]
fn small() {
    let mut lt = LayoutTester::init(5, 1, 1);
    assert_layout!(lt, Update(&["12", "a\nb"]), &[1], &[2]);
    assert_layout!(lt, Incr(1), &[2, 1], &[]);
    assert_layout!(lt, Decr(1), &[1], &[2]);
}

#[test]
fn large() {
    let mut lt = LayoutTester::init(3, 0, 0);
    assert_layout!(lt, Update(&["1\n2\n3\n4\n5", "1"]), &[3], &[]);
    assert_layout!(lt, Resize(3, 1, 0), &[3], &[]);
    assert_layout!(lt, Resize(3, 1, 1), &[2], &[1]);
}

#[test]
fn item_change() {
    let mut lt = LayoutTester::init(4, 1, 1);
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
}

#[test]
fn overflow() {
    let mut lt = LayoutTester::init(6, 2, 3);
    assert_layout!(lt, Update(&["12\n34", "ab", "1\n2\n3\n4"]), &[2], &[1, 3]);
    assert_layout!(lt, Incr(1), &[1, 2], &[3]);
    assert_layout!(lt, Incr(1), &[3], &[]);
    assert_layout!(lt, Resize(5, 2, 2), &[3], &[]);
    assert_layout!(lt, Resize(7, 2, 3), &[4], &[]);
    assert_layout!(lt, Decr(2), &[2], &[1, 4]);
    assert_layout!(lt, Resize(6, 2, 0), &[2], &[1, 3]);
    assert_layout!(lt, Incr(2), &[4, 1, 1], &[]);
    assert_layout!(lt, Resize(8, 2, 0), &[4, 1, 2], &[]);
    assert_layout!(lt, Resize(7, 2, 0), &[4, 1, 2], &[]);
    assert_layout!(lt, Resize(6, 2, 0), &[4, 1, 1], &[]);
}

#[test]
fn resize_basic() {
    let mut lt = LayoutTester::init(5, 1, 1);
    assert_layout!(
        lt,
        Update(&["0", "1", "2", "3", "4", "5"]),
        &[1],
        &[1, 1, 1, 1]
    );
    assert_layout!(lt, Incr(5), &[1, 1, 1, 1], &[]);
    assert_layout!(lt, Resize(3, 1, 1), &[1, 1], &[]);
    assert_layout!(lt, Resize(5, 1, 1), &[1, 1, 1, 1], &[]);
}

#[test]
fn resize_with_padding_change() {
    let mut lt = LayoutTester::init(10, 3, 3);
    assert_layout!(
        lt,
        Update(&["0\n0\n0", "1", "2\n2", "3\n3\n3\n3"]),
        &[3],
        &[1, 2, 4]
    );
    assert_layout!(lt, Resize(5, 1, 1), &[3], &[1, 1]);
    assert_layout!(lt, Incr(2), &[2, 1, 1], &[1]);
    assert_layout!(lt, Resize(8, 2, 2), &[2, 1, 3], &[2]);
    assert_layout!(lt, Resize(5, 1, 1), &[2, 1, 1], &[1]);
}

#[test]
fn item_alignment() {
    let mut lt = LayoutTester::init(20, 3, 3);
    assert_layout!(lt, Update(&["0\n1\n2\n", "0\n1", "0\n1"]), &[4], &[2, 2]);
    assert_layout!(lt, Incr(2), &[2, 2, 4], &[]);
    assert_layout!(lt, Decr(1), &[2, 4], &[2]);
}

#[test]
fn scrolldown() {
    let mut lt = LayoutTester::init(8, 2, 2);
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
}

#[test]
fn scrollback() {
    let mut lt = LayoutTester::init(5, 1, 1);
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
}

#[test]
fn multiline_jitter() {
    let mut lt = LayoutTester::init(12, 3, 3);
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
}

#[test]
fn scroll_mid() {
    let mut lt = LayoutTester::init(5, 1, 1);
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
    assert_layout!(lt, Resize(7, 1, 1), &[1, 1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Incr(2), &[1, 1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(1), &[1, 1, 1], &[1, 1, 1, 1]);
    assert_layout!(lt, Incr(1), &[1, 1, 1, 1], &[1, 1, 1]);
    assert_layout!(lt, Decr(2), &[1, 1], &[1, 1, 1, 1, 1]);
    assert_layout!(lt, Incr(20), &[1, 1, 1, 1, 1, 1], &[]);
}
