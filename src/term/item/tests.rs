use std::sync::Arc;

use nucleo::{Config, Nucleo, Utf32String};

use super::*;

fn reset(nc: &mut Nucleo<&'static str>, items: &[&'static str]) {
    nc.restart(true);
    let injector = nc.injector();
    for item in items {
        injector.push(item, |item, cols| {
            cols[0] = Utf32String::from(*item);
        });
    }

    while nc.tick(10).running {}

    println!("Reset!");
    for item in nc.snapshot().matched_items(..).rev() {
        println!("* * * * * *\n{}", item.data);
    }
}

#[test]
fn test_layout_basic() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    // basic test to check small number of items
    reset(&mut nc, &["12\n34", "ab"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(6, 2, 3, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 3, 1, nc.snapshot()),
        LayoutView {
            below: &[2],
            current: 1,
            above: &[],
        }
    );
}

#[test]
fn test_layout_large() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["1\n2\n3\n4\n5", "1"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(3, 0, 0, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 3,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(3, 1, 0, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 3,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(3, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1],
        }
    );
}

#[test]
fn test_layout_overflow() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["12\n34", "ab", "1\n2\n3\n4"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(6, 2, 3, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1, 3],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 3, 1, nc.snapshot()),
        LayoutView {
            below: &[2],
            current: 1,
            above: &[3],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 3, 2, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 3,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 2, 2, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 4,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(7, 2, 2, 2, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 4,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(7, 2, 2, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1, 4],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 0, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 4,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 0, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 4,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(7, 2, 0, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 4,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(6, 2, 0, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 4,
            above: &[],
        }
    );
}

#[test]
fn test_scrolldown() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);
    reset(
        &mut nc,
        &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"],
    );
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(8, 2, 2, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 1,
            above: &[1, 1, 1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 10, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1, 1, 1],
            current: 1,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 9, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1, 1],
            current: 1,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 8, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 7, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 6, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 5, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(8, 2, 2, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 1,
            above: &[1, 1, 1, 1, 1, 1, 1],
        }
    );
}

#[test]
fn test_layout_scrollback() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["12\n34", "ab", "c", "d", "e", "f\ng"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(5, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 3, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 4, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 5, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 2,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 4, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[2],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 5, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 2,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 4, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[2],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 3, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 2],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 2, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 4, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 2,
            above: &[1, 1, 1],
        }
    );
}

#[test]
fn test_layout_small() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["12", "a\nb"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(5, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 1,
            above: &[2],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 2,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 1,
            above: &[2],
        }
    );
}

#[test]
fn test_bottom_item_alignment() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["0\n1\n2\n", "0\n1", "0\n1"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(20, 3, 3, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 4,
            above: &[2, 2],
        }
    );

    assert_eq!(
        layout.recompute(20, 3, 3, 2, nc.snapshot()),
        LayoutView {
            below: &[2, 4],
            current: 2,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(20, 3, 3, 1, nc.snapshot()),
        LayoutView {
            below: &[4],
            current: 2,
            above: &[2],
        }
    );
}

#[test]
fn test_multiline_jitter() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["a", "b", "0\n1\n2\n3", "0\n1", "0\n1"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(12, 3, 3, 0, nc.snapshot()),
        LayoutView {
            below: &[],
            current: 1,
            above: &[1, 4, 2, 2],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[4, 2, 2],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 4,
            above: &[2, 2],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 3, nc.snapshot()),
        LayoutView {
            below: &[4, 1, 1],
            current: 2,
            above: &[2],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 4, nc.snapshot()),
        LayoutView {
            below: &[2, 4, 1],
            current: 2,
            above: &[],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 3, nc.snapshot()),
        LayoutView {
            below: &[4, 1],
            current: 2,
            above: &[2],
        }
    );

    assert_eq!(
        layout.recompute(12, 3, 3, 2, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 4,
            above: &[2, 2],
        }
    );

    println!("Starting");

    assert_eq!(
        layout.recompute(12, 3, 3, 3, nc.snapshot()),
        LayoutView {
            below: &[4, 1],
            current: 2,
            above: &[2],
        }
    );
}

#[test]
fn test_layout_mid_screen() {
    let mut nc = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);

    reset(&mut nc, &["0", "1", "2", "3", "4", "5", "6", "7"]);
    let mut layout = Layout::default();

    assert_eq!(
        layout.recompute(5, 1, 1, 4, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 2, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 3, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1],
        }
    );

    assert_eq!(
        layout.recompute(5, 1, 1, 2, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 3, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 2, nc.snapshot()),
        LayoutView {
            below: &[1, 1],
            current: 1,
            above: &[1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 3, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1],
            current: 1,
            above: &[1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 1, nc.snapshot()),
        LayoutView {
            below: &[1],
            current: 1,
            above: &[1, 1, 1, 1, 1],
        }
    );

    assert_eq!(
        layout.recompute(7, 1, 1, 7, nc.snapshot()),
        LayoutView {
            below: &[1, 1, 1, 1, 1],
            current: 1,
            above: &[],
        }
    );
}
