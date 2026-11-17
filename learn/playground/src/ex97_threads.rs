//! Lesson 97: threads and scoped threads. Run `cargo test ex97`.

use std::thread;

/// 1. Run `work` on a new OS thread and return its result. `thread::spawn` needs a `'static` closure
///    (it may outlive this function), so `move` the input in; `.join()` waits and returns
///    `Result<T, panic payload>`. Return `None` if the thread panicked.
pub fn on_another_thread(input: Vec<u64>) -> Option<u64> {
    todo!()
}

/// 2. Sum `values` using `parts` threads, each summing one chunk. `thread::scope` lets threads BORROW
///    `values`: the scope waits for all of them before returning, so no `'static` or `move` of the data.
///    Hint: `values.chunks(chunk_len)`, `scope.spawn(move || chunk.iter().sum::<u64>())`, collect the
///    handles in a Vec, then join each one (`.join().unwrap_or(0)` is fine here).
pub fn parallel_sum(values: &[u64], parts: usize) -> u64 {
    let chunk_len = values.len().div_ceil(parts.max(1)).max(1);
    todo!()
}

// 3. Uncomment, read the error ("`Rc<u32>` cannot be sent between threads safely"), then comment it out again.
//    Lesson 98 explains it.
//
// pub fn not_send() {
//     let shared = std::rc::Rc::new(5);
//     thread::spawn(move || println!("{shared}"));
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_elsewhere() {
        assert_eq!(on_another_thread(vec![1, 2, 3]), Some(6));
    }

    #[test]
    fn sums_in_parallel_with_borrowed_data() {
        let values: Vec<u64> = (1..=1000).collect();
        assert_eq!(parallel_sum(&values, 4), 500_500);
        assert_eq!(parallel_sum(&values, 1), 500_500);
        assert_eq!(parallel_sum(&[], 3), 0);
        // `values` is still ours: the threads only borrowed it.
        assert_eq!(values.len(), 1000);
    }
}
