//! Delay-based limit which considers the difference in average response times between a short time
//! window and a long window.
//!
//! Changes these values is considered an indicator of a change in load on the system.
//!
//! - Delay-based
//! - Aggressively reduces the limit in response to increased latency
