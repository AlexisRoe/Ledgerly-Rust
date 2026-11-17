//! Lesson 8. Run with `cargo run --bin hello`.

fn main() {
    println!("Hello from the playground!");

    // Exercise 1
    let name = "Alexis";
    println!("Hello, {name}!");

    // Exercise 2: without `mut`, `counter += 1` is error E0384 "cannot assign twice to immutable variable".
    let mut counter = 0;
    counter += 1;
    counter += 1;
    counter += 1;
    println!("counter = {counter}");
}
