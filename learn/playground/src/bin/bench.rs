//! Lesson 109: measuring. Run it twice and compare:
//!
//!     cargo run --bin bench              # debug build: no optimisations
//!     cargo run --release --bin bench    # release build
//!
//! `std::hint::black_box` hides a value from the optimiser, so it can't delete work whose result is unused or
//! compute it at compile time. This is a rough stopwatch, not a benchmark framework: for real measurements use
//! criterion (repeated runs, warm-up, statistics). See lesson 109.

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

fn time(label: &str, runs: u32, mut work: impl FnMut()) {
    work(); // warm-up
    let start = Instant::now();
    for _ in 0..runs {
        work();
    }
    let each: Duration = start.elapsed() / runs;
    println!("{label:<45} {each:>12.2?} per run");
}

fn main() {
    let values: Vec<u64> = (0..1_000_000).collect();

    time("sum with an iterator", 20, || {
        black_box(black_box(&values).iter().sum::<u64>());
    });
    time("sum with an index loop", 20, || {
        let values = black_box(&values);
        let mut total = 0u64;
        for index in 0..values.len() {
            total += values[index];
        }
        black_box(total);
    });

    time("100k format! calls, one String each", 10, || {
        let mut lines = Vec::new();
        for n in 0..100_000 {
            lines.push(format!("{n:08}"));
        }
        black_box(lines);
    });
    time("100k write! calls into one presized String", 10, || {
        let mut text = String::with_capacity(100_000 * 8);
        for n in 0..100_000 {
            let _ = write!(text, "{n:08}");
        }
        black_box(text);
    });

    let dynamic: Box<dyn Fn(u64) -> u64> = Box::new(|x| x + 1);
    let generic = |x: u64| x + 1;
    time("1M calls through Box<dyn Fn>", 20, || {
        let f = black_box(&dynamic);
        let mut total = 0;
        for x in 0..1_000_000 {
            total += f(x);
        }
        black_box(total);
    });
    time("1M calls through a generic closure", 20, || {
        let f = black_box(&generic);
        let mut total = 0;
        for x in 0..1_000_000 {
            total += f(x);
        }
        black_box(total);
    });
}
