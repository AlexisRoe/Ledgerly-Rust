//! Exercises for the course in `learn/`. One module per exercise, named after the lesson that introduces it.
//!
//! Run one exercise's tests with `cargo test exNN`, e.g. `cargo test ex16`.

// Exercises start with unfinished functions; these warnings would only be noise.
#![allow(dead_code, unused_variables, unused_mut, unused_imports, unreachable_code)]
pub mod ex09_expressions;
pub mod ex12_paths;
pub mod ex16_numbers;
pub mod ex18_strings;
pub mod ex19_collections;
pub mod ex20_structs;
pub mod ex21_impl;
pub mod ex22_enums;
pub mod ex23_enum_data;
pub mod ex24_option;
pub mod ex25_result;
pub mod ex26_newtypes;
pub mod ex30_loops;
pub mod ex31_match;
pub mod ex33_if_let;
pub mod ex34_question_mark;
pub mod ex37_moves;
pub mod ex39_borrowing;
pub mod ex40_borrow_errors;
pub mod ex45_closures;
pub mod ex46_iterators;
pub mod ex47_collect;
pub mod ex49_traits;
pub mod ex51_conversions;
pub mod ex52_generics;
pub mod ex56_thiserror;
pub mod ex63_tokio;
pub mod ex68_serde;
pub mod ex80_lifetimes;
pub mod ex81_borrowing_structs;
pub mod ex83_borrowing_practice;
pub mod ex84_interior_mutability;
pub mod ex85_cow;
pub mod ex86_drop_and_take;
pub mod ex88_associated_types;
pub mod ex89_iterators_by_hand;
pub mod ex90_blanket_impls;
pub mod ex91_orphan_rule;
pub mod ex92_dyn_compatibility;
pub mod ex93_flexible_parameters;
pub mod ex94_ordering_and_operators;
pub mod ex95_typestate;
pub mod ex97_threads;
pub mod ex99_shared_state;
pub mod ex100_channels;
pub mod ex101_futures_by_hand;
pub mod ex102_pin;
pub mod ex103_cancellation;
pub mod ex104_blocking;
pub mod ex106_layout;
pub mod ex107_allocations;
pub mod ex111_error_design;
pub mod ex112_parse_dont_validate;
pub mod ex113_builders;
pub mod ex116_docs;
pub mod ex118_macro_rules;
pub mod ex119_macro_patterns;
