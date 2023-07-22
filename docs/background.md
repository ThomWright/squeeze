# Theory and background

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
  - Load-based request failures e.g.
    - Explicit: HTTP 429, gRPC RESOURCE_EXHAUSTED
    - Implicit: timeouts, gRPC DEADLINE_EXCEEDED

TODO: When shouldn't you use it?

## Resources, queueing and Little's Law

All systems have hard limits on the amount of concurrency they can support. The available concurrency is limited by resources such as CPU, memory, disk or network bandwidth, thread pools or connection pools. For most of these (memory being one exception), when they saturate, queues start to build up.

Little's Law can be applied to steady state systems (that is, systems where the queues are not growing due to overload):

`L = λW` where

- `L` = number of jobs in a stationary system
- `λ` = the long-term average effective arrival rate
- `W` = the average time that the system takes to process a job

E.g. for requests to a server, `concurrency = RPS * average latency`.

A single CPU has a natural concurrency limit of 1 before jobs start queueing and latency increases. If we know that jobs take 10ms on average, then the CPU can handle 100 RPS on average.

For complex systems, we generally do not know the concurrency limits.

## Rate limits vs concurrency limits

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

## Static vs dynamic limits

TODO:

- Static
  - How do you know what to set it to? Even if you can work out a good number, what if that changes? E.g. a downstream system increases/decreases capacity, or the workload changes?
- Dynamic
  - Can automatically detect and respond to overload

## Circuit breakers vs throttling

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

## On congestion detection

- TCP congestion control
  - Delay-based (RTT in TCP, latency here) or loss-based (packet loss in TCP, errors caused by load here)

## Delay-based vs loss-based

- Delay-based - latency
  - TODO:
- Loss-based - request failures
  - TODO:

## Symptoms vs causes

- We need a way to detect overload.
- Causes: Resources such as CPU, memory, threads, connections, bandwidth are the underlying bottlenecks
  - It can be hard to predict what the bottleneck will be and what effect it will have, especially in large, complex systems.
- Symptoms: Instead, we can measure _symptoms_ - increased latency (our own, or from other systems) or failures from overloaded systems
- In the spirit of [alerting on symptoms, not causes](https://docs.google.com/document/d/199PqyG3UsyXlwieHaqbGiWVa8eMWi8zzAn0YfcApr8Q/edit)

## Explicit vs implicit backpressure signalling

- Push vs pull
  - Push – e.g. clients sending requests to servers
  - Pull – e.g. consumers pulling messages off a queue
- For backpressure to work in a push-based system, upstream systems need to know when to stop sending traffic.
- A couple of ways to implement this:
  - Explicit: downstream systems sending extra data in responses about how loaded the system is which the upstream system can use as an indicator of load
    - e.g. ECN in TCP
    - Requires more coupling between services
    - "Please don't give me any more work, I'm very busy"
  - Implicit: upstream systems can detect increased load or certain error responses e.g. HTTP 429 or gRPC RESOURCE_EXHAUSTED
    - "My colleague looks very overworked, perhaps I won't give them any more tasks to do"

## Server-side vs client-side

- Client-side
  - Compete with each other – algorithm needs to be fair
    - Loss-based can "muscle out" delay-based algorithms
  - Load balanced across multiple servers
    - Assume load balancers are working well
  - Prefer loss-based? Why?
    - Easy to use rejected requests as a backpressure signal, especially for offline/batch applications.
- Server-side
  - Prefer delay-based? Why?

## Per-operation limiting

- Imagine a server with three operations:
  1. Read resource by ID
  2. Full text search for resources
  3. Write new resource
- One per service is simple, and protects the whole service if it is overloaded. If all operations have similar latency characteristics it can work well. But a wide range of potential latencies could make it unpredictable when using a delay-based limiter.
- Using separate limiters per operation could work more predictably, but they would need to fairly distribute the capacity between them. E.g. it would be bad if operation 1 could take all capacity and not leave enough for the others.
- A fair delay-based limiter such as Vegas can work well here.

## Limiters everywhere vs at the top

- TODO: Need to think about this one
- End-to-end principle, same as for retries
