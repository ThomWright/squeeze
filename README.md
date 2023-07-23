# Squeeze

Dynamic congestion-based concurrency limits for controlling backpressure.

[Documentation](./docs/index.md)

## What is this?

A Rust library to dynamically control concurrency limits. Several algorithms are included, mostly based on TCP congestion control. These detect signs of overload by observing and reacting to either load-based failures (loss) or latency (delay). Beyond the limit, additional requests to perform work can be rejected. These rejections can be detected by upstream limiters as load-based failures, acting as an effective form of backpressure.

In general, systems serving clients by doing jobs have a finite number of resources. For example, an HTTP server might have 4 CPU cores available. When these resources become heavily utilised, queues begin to form, and job latency increases. If these queues continue to grow, the system becomes effectively overloaded and unable to respond to job requests in a reasonable time.

These systems can only process so many jobs concurrently. For a purely CPU-bound job, the server above might only be able to process about 4 jobs concurrently. Reality is much more complex, however, and therefore this number is much harder to predict.

Concurrency limits can help protect a system from becoming overloaded, and these limits can be automatically set by observing and responding to the behaviour of the system.

See the [documentation](./docs/background.md) for more details.

## Goals

This library aims to:

1. Achieve optimally high throughput and low latency.
2. Shed load and apply backpressure in response to congestion or overload.
3. Fairly distribute available resources between independent clients with zero coordination.

## Limit algorithms

The congestion-based algorithms come in several flavours:

- Loss-based – respond to failed jobs (overload)
- Delay-based – respond to increases in latency (congestion)

| Algorithm                         | Feedback                    | Response | [Fairness](https://en.wikipedia.org/wiki/Fairness_measure)                                       |
|-----------------------------------|-----------------------------|----------|--------------------------------------------------------------------------------------------------|
| [AIMD](src/limit/aimd.rs)         | Loss (implicit or explicit) | AIMD     | Fair, but can out-compete delay-based algorithms                                                 |
| [Gradient](src/limit/gradient.rs) | Delay (implicit)            | AIMD     | TODO: ?                                                                                          |
| [Vegas](src/limit/vegas.rs)       | Delay (implicit)            | AIAD     | [Proportional](https://en.wikipedia.org/wiki/Proportional-fair_scheduling) until overload (loss) |

### Example topology

The example below shows two applications using limiters on the client (output) and on the server (input), using different algorithms for each.

![Example topology](docs/assets/example-topology.png)

### Caveats

TODO:

- Loss-based algorithms require a reliable signal for load-based errors.
  - If configured to reduce concurrency for non-load-based errors, they can exacerbate availability problems when these errors occur.
- Delay-based algorithms work more reliably with predictable latency.
  - For example, short bursts of increased latency from GC pauses could cause an outsized reduction in concurrency limits.

## FAQ

> Does this require coordination between multiple processes?

No! The congestion avoidance is based on TCP congestion control algorithms which are designed to work independently. In TCP, each transmitting socket independently detects congestion and reacts accordingly.

## Installing, running and testing

TODO:

## Prior art

- Netflix
  - [concurrency-limits](https://github.com/Netflix/concurrency-limits)
  - [Performance Under Load](https://netflixtechblog.medium.com/performance-under-load-3e6fa9a60581)

## Further reading

- [Wikipedia -- TCP congestion control](https://en.wikipedia.org/wiki/TCP_congestion_control)
- [AWS -- Using load shedding to avoid overload](https://aws.amazon.com/builders-library/using-load-shedding-to-avoid-overload/)
- [Sarah-Marie Nothling -- Load Series: Throttling vs Loadshedding](https://sarahnothling.wordpress.com/2019/05/12/load-series-throttling-vs-loadshedding/)
- [Myntra Engineering -- Adaptive Throttling of Indexing for Improved Query Responsiveness](https://medium.com/myntra-engineering/adaptive-throttling-of-indexing-for-improved-query-responsiveness-b3ac949e76c9)
- [TCP Congestion Control: A Systems Approach](https://tcpcc.systemsapproach.org/index.html)
- [LWN -- Delay-gradient congestion control (CDG)](https://lwn.net/Articles/645115/)
- [Strange Loop -- Stop Rate Limiting! Capacity Management Done Right](https://www.youtube.com/watch?v=m64SWl9bfvk)

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
