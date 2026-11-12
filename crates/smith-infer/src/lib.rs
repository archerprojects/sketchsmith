//! # smith-infer
//!
//! SketchSmith — inference engine | v1.0.0 | Thursday 12 November 2026
//!
//! Owns inference candidates and priorities, from-point tracking, Shift and
//! arrow-key locks, the picking structures, and the measurement-box parser and
//! formatter.
//!
//! Ray picking ([`pick`]), the face BVH ([`bvh`]), selection picking
//! ([`select`]), the inference engine ([`infer`]), and the typed-value grammar
//! and unit formatting ([`units`]).

pub mod bvh;
pub mod infer;
pub mod pick;
pub mod select;
pub mod units;
