# Squeeze

Dynamic congestion-based concurrency limits for controlling backpressure.

## On congestion detection

- TCP congestion control
  - Delay-based (RTT in TCP, latency here) or loss-based (packet loss in TCP, errors caused by load here)

## FAQ

> Does this require coordination between multiple processes?

No! The congestion detection is based on TCP congestion control algorithms which are designed to work independently. In TCP, each transmitting socket independently detects congestion and reacts accordingly.

## Prior art

- [Netflix's concurrency limits](https://github.com/Netflix/concurrency-limits)
