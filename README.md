# Squeeze

Dynamic congestion-based concurrency limits for controlling backpressure.

## Roadmap

- [ ] Limit algorithms
  - [ ] Loss-based
    - [x] AIMD
  - [ ] Delay-based
    - [ ] Vegas
    - [ ] Gradient descent
  - [ ] Combined loss- and delay-based
- [ ] Tests
  - [ ] ...?
- [ ] Simulator:
  - [ ] Topology
    - [ ] `Source` and `Sink` interfaces?
    - [ ] `LoadSource -> Option<ClientLimiter> -> Option<ServerLimiter> -> Server`?
    - [ ] `Server -> *Servers`?
  - [ ] LoadSource - cycle through behaviours, e.g. 100 RPS for 10 seconds, 0 RPS for 2 seconds
  - [ ] Results
    - [ ] Each node keep track of own metrics?
    - [ ] Graphs
- [ ] Limiter
  - [ ] Rejection delay
    - Option to add delay before rejecting jobs. Intended to slow down clients, e.g. RabbitMQ retries.
  - [ ] Static partitioning
    - How possible would it be to partition somewhat dynamically? E.g. on customer IDs?
  - [ ] LIFO
    - Optimise for latency
- [ ] Documentation
  - [ ] README
  - [ ] Rustdoc `#![warn(missing_docs)]`

## What is this?

TODO: Why would someone want to use this? What problem does it solve, and how?

- How do we get maximum goodput (successful throughput) in an overloaded system?
  - Overload, goodput vs throughput
  - Need to throttle or shed load in order to protect systems from overload
  - Ideally, 1. we send the amount of traffic that the system can handle, but no more, and 2. the system can shed any excess load with a minimum of work.
- How do we prevent upstream systems from overloading downstream systems?
  - Backpressure
  - E.g. `External API -> Internal service -> Database`
  - Need to be able to _propagate_ throttling
    - Reject, drop or pause (slow down)
    - Systems need a way to communicate backpressure
- How do we detect load or overload?
  - Increases in latency
  - Load-based request failures, e.g. HTTP 429 or gRPC RESOURCE_EXHAUSTED

The Netflix one-liner:

> Java Library that implements and integrates concepts from TCP congestion control to auto-detect concurrency limits for services in order to achieve optimal throughput with optimal latency.

TODO: When shouldn't you use it?

## Shop analogy

TODO:

- Imagine a busy supermarket. Most of the time the rate of people arriving at the store is sustainable. People go in and come out again fairly quickly. Internally, queues are usually short.
- At peak times, the number of people arriving every minute is too high. The cashiers can't keep up, queues start to form.
- The supermarket has fancy _autoscaling_: as the queue sizes increase, more cashiers work the checkouts. There is, of course, a fixed number of cashiers and checkouts. They _could_ keep all checkouts open at all times to keep queues down and reduce people's shopping time, but this costs a lot of money. It's not worth it most of the time.
- For a while, this is OK. But if the queues get too long, people get frustrated. Sometimes, after waiting too long they simply give up and leave without any shopping.
- When it gets _really_ bad, the queues fill up the whole shop. People can't get in or out, everything gets stuck. People who do get in find a completely clogged shop and leave immediately.
- Sometimes, capacity is reduced unexpectedly. Cashiers are sick, checkout registers aren't working.
- To avoid this situation, a bouncer is hired to stand outside. The bouncer doesn't know what's going on inside the shop, because communication is bad. But they do know when people enter and exit.
- After running a few tests, they observed that if more than ten people per minute enter the shop, things start to slow down, so they tell the bouncer to keep to this limit.
- This kind of works, mostly. Except in some situations. Around Christmas it seems to be worse, and also when capacity is reduced.
- After some investigation, they learn that around Christmas time, people take longer to shop. Instead of customers doing their normal shop where they get the same things every week, they do a bigger shop which takes longer. They do some measurements and find that on average customers take 10 minutes to do a normal shop, but at Christmas it takes 15 minutes.
- They do some little calculations: 10 people per minute \* 10 minutes each = 100 people in the shop at once. 10 people per minute \* 15 minutes each = 150 people in the shop at once. This is why everything is getting clogged up.
- So they change their approach. Instead of letting in 10 people per minute, they let a maximum of 100 people in the shop at a time.
- This makes things better. Even when people take longer in the shop, they don't get long queues like before. It also helps with the capacity problem, for the most part. On days with fewer cashiers, checking out takes a bit longer. When they reach the 100 person limit, it's a one-in-one-out situation. If people spend longer in the shop, fewer people per minute are let in.
- One day though, there's a problem with the milk. Normally there's two shelves, two meters wide, full of milk. On busy days, several people can pick up milk at the same time and move on. But not today: some of the fridges aren't working. Everything gets squeezed into the available fridge space, and there's only room for one person to pick up milk at once. A queue starts to form.
- The layout of the shop is designed to handle queues near the checkouts, but not the milk. As the queue gets bigger, it blocks the whole aisle. Even people who don't need milk are getting blocked. Everything slows down. Those who manage to finish their shopping spent much longer in the shop. Many people leave without finishing. Those who leave are quickly replaced. The queue doesn't go down.
- Soon, the milk on the shelf runs out. Someone needs to restock it, but they can't get access because they're blocked by the queue. It's a deadlock.
- The shop managers have another think. Even by limiting the number of people in the shop, they couldn't avoid it getting clogged up. But, they think, maybe the bouncer could have been smarter. The bouncer could have observed that shoppers were taking longer, and that many were leaving without shopping. This might be a sign that things aren't going well inside the shop, and fewer people should be let inside.
- They start writing some algorithms. In general: when people take longer in the shop, or people start leaving without any shopping, the limit is reduced. When the time spent in the shop goes down again, or people stop leaving empty-handed, the limit gets increased again.
- This works well, and is so successful they want to launch it in their flagship shop. This shop is so large, it has two entrances: one for pedestrians and one for cars.
- They wonder if this might pose a problem. Their bouncers, already known for not communicating with shop staff, also won't communicate with each other. Will this system still work?
- They try it. They funnel as many people as they can through entrance one, until the shop is at capacity. This is working as before. Then, they open the second entrance. Both bouncers start seeing increased wait times, and people leaving without any shopping. They reduce their limits. Eventually, they both settle to around the same number. The system works.

## Limit algorithms

TODO: table comparison of the different algorithms

- Input: delay, loss or both
- Increase/decrease characteristics - additive, multiplicative or otherwise

## How should I set concurrency limits?

TODO:

- Example: database-bound service with 10 connections
- Example: I/O-bound service running on a single core

## Background

### Resources, queueing and Little's Law

All systems have hard limits on the amount of concurrency they can support. The available concurrency is limited by resources such as CPU, memory, disk or network bandwidth, thread pools or connection pools. For most of these (memory being one exception), when they saturate, queues start to build up.

Little's Law can be applied to steady state systems (that is, systems where the queues are not growing due to overload):

`L = λW` where

- `L` = number of jobs in a stationary system
- `λ` = the long-term average effective arrival rate
- `W` = the average time that the system takes to process a job

E.g. for requests to a server, `concurrency = RPS * average latency`.

A single CPU has a natural concurrency limit of 1 before jobs start queueing and latency increases. If we know that jobs take 10ms on average, then the CPU can handle 100 RPS on average.

For complex systems, we generally do not know the concurrency limits.

### Rate limits vs concurrency limits

TODO:

- Rate is one dimensional
  - Doesn't consider how long things take (how expensive the work is for each request)
- Concurrency is a better measure of throughput, considering both rate and latency
  - Reacts to changes in both rate and latency, that is: more jobs, or more expensive jobs
- This is the difference between:
  1. How many people should we let into the nightclub per minute?
  2. How many people should we allow in the nightclub at once?
- Example:
  - Load tests revealed that at rates much higher than 100 RPS, the system starts to get overloaded.
  - Expected average latency = 100ms
  - Arrival rate = 80 RPS
  - RPS – limit = 100 RPS
    - Imagine an unexpected situation causes the server to get overloaded. Perhaps the some expensive requests are being made, or capacity has been reduce because of a crash.
    - Latency goes up 10x to 1s. 80 RPS is still allowed through. Rate limiting doesn't protect the system.
  - Concurrency – limit = 10
    - Imagine the same scenario. `L = λW = 80 * 1 = 80`, which is much higher than 10. At this RPS and latency, 7/8 requests would be rejected, reducing load on the server. As load reduces, latency goes down, allowing more requests per second through.

### Static vs dynamic limits

TODO:

- Static
  - How do you know what to set it to? Even if you can work out a good number, what if that changes? E.g. a downstream system increases/decreases capacity, or the workload changes?
- Dynamic
  - Can automatically detect and respond to overload

### Circuit breakers vs throttling

- Circuit breakers
  - All or nothing - if a circuit breaker sees a downstream system is overloaded it stops all traffic to that system. This is OK for a complete outage, but many cases of overload are likely to be "brownouts" where _some_ traffic could be processed.
- Throttling
  - More responsive to partial outages. Traffic can be reduced to a level the downstream system can handle. Overall availability during a partial outage can be much higher.

Example of a circuit breaker causing a complete outage for a particular API route:

```text
        availability = 0%         overloaded
          v      v                  v
Client -> API -> internal system -> database
                               ^
                      circuit breaker trips
```

### On congestion detection

- TCP congestion control
  - Delay-based (RTT in TCP, latency here) or loss-based (packet loss in TCP, errors caused by load here)

### Delay-based vs loss-based

- Delay-based - latency
  - TODO:
- Loss-based - request failures
  - TODO:

### Symptoms vs causes

- We need a way to detect overload.
- Causes: Resources such as CPU, memory, threads, connections, bandwidth are the underlying bottlenecks
  - It can be hard to predict what the bottleneck will be and what effect it will have, especially in large, complex systems.
- Symptoms: Instead, we can measure _symptoms_ - increased latency (our own, or from other systems) or failures from overloaded systems
- In the spirit of [alerting on symptoms, not causes](https://docs.google.com/document/d/199PqyG3UsyXlwieHaqbGiWVa8eMWi8zzAn0YfcApr8Q/edit)

### Explicit vs implicit backpressure signalling

- Push vs pull
  - Push – e.g. clients sending requests to servers
  - Pull – e.g. consumers pulling messages off a queue
- For backpressure to work in a push-based system, upstream systems need to know when to stop sending traffic.
- A couple of ways to implement this:
  - Explicit: downstream systems sending extra data in responses about how loaded the system is which the upstream system can use as an indicator of load
    - e.g. ECN in TCP
    - Requires more coupling between services
  - Implicit: upstream systems can detect increased load or certain error responses e.g. HTTP 429 or gRPC RESOURCE_EXHAUSTED

### Limiters everywhere vs at the top

- TODO: Need to think about this one
- End-to-end principle, same as for retries

## Caveats

TODO:

- Loss-based algorithms require a reliable signal for load-based errors.
  - If configured to reduce concurrency for non-load-based errors, they can exacerbate unavailability when these errors occur.
- Delay-based algorithms work more reliably with predictable latency.
  - For example, short bursts of increased latency from GC pauses could cause an outsized reduction in concurrency limits.

## FAQ

> Does this require coordination between multiple processes?

No! The congestion detection is based on TCP congestion control algorithms which are designed to work independently. In TCP, each transmitting socket independently detects congestion and reacts accordingly.

## Glossary

- Downstream - system receiving requests or messages
- Upstream - system sending requests or messages

## Installing, running and testing

TODO:

## Prior art

- [Netflix's concurrency-limits](https://github.com/Netflix/concurrency-limits)

## Further reading

- [Wikipedia -- TCP congestion control](https://en.wikipedia.org/wiki/TCP_congestion_control)
- [AWS -- Using load shedding to avoid overload](https://aws.amazon.com/builders-library/using-load-shedding-to-avoid-overload/)
- [Sarah-Marie Nothling -- Load Series: Throttling vs Loadshedding](https://sarahnothling.wordpress.com/2019/05/12/load-series-throttling-vs-loadshedding/)
- [Myntra Engineering -- Adaptive Throttling of Indexing for Improved Query Responsiveness](https://medium.com/myntra-engineering/adaptive-throttling-of-indexing-for-improved-query-responsiveness-b3ac949e76c9)
