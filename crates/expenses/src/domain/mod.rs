//! The pure core of the expenses module: no I/O, no async, time only through `Clock` (CONCEPT §3, rule 4).

#![deny(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
