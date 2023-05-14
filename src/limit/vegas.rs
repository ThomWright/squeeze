//! Estimates queuing delay.
//!
//! [Understanding TCP Vegas: Theory and Practice](https://www.cs.princeton.edu/techreports/2000/616.pdf)
//!
//! - Delay-based
//! - Very reactive, adjusts limit on every reading
//! - Additive increase/decrease
