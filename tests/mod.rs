use std::{cmp::Reverse, collections::BinaryHeap, time::Duration};

use rand::{prelude::Distribution, rngs::SmallRng, Rng, SeedableRng};
use statrs::distribution::{Erlang, Exp};

use squeeze::{
    limit::{AIMDLimit, LimitAlgorithm},
    Limiter, ReadingResult, Timer,
};
use tokio::time::Instant;

struct Simulation {
    duration: Duration,
    client: Client,
    server: Server,
}

enum LimitWrapper {
    Aimd(AIMDLimit),
}
impl LimitAlgorithm for LimitWrapper {
    fn initial_limit(&self) -> usize {
        match self {
            LimitWrapper::Aimd(l) => l.initial_limit(),
        }
    }
    fn update(&self, reading: squeeze::Reading) -> usize {
        match self {
            LimitWrapper::Aimd(l) => l.update(reading),
        }
    }
}

struct Client {
    /// Poisson process, exponential interarrival times.
    interarrival: Exp,
}

struct Server {
    limiter: Limiter<LimitWrapper>,

    latency: Erlang,

    /// Range: [0, 1)
    failure_rate: f64,
}

struct Request<'t> {
    latency: Duration,
    timer: Timer<'t>,

    /// Limit state before the request was made.
    limit_state: LimitState,
}

struct RequestResult {
    result: ReadingResult,

    /// Limit state after the request ended.
    limit_state: LimitState,
}

#[derive(Debug)]
struct Event<'t> {
    time: Instant,
    typ: EventType<'t>,
}
#[derive(Debug)]
enum EventType<'t> {
    StartRequest,
    EndRequest {
        start_time: Instant,
        original_limit_state: LimitState,
        timer: Timer<'t>,
    },
}

struct Summary {
    event_log: Vec<EventLogEntry>,
    requests: Vec<RequestSummary>,
}

#[derive(Debug)]
struct RequestSummary {
    start_time: Instant,
    start_state: LimitState,
    end_state: LimitState,
    end_time: Instant,
    latency: Duration,
    result: ReadingResult,
}

#[derive(Debug, Clone, Copy)]
struct LimitState {
    limit: usize,
    available: usize,
}

#[derive(Debug)]
enum EventLogEntry {
    Accepted(LimitState),
    Rejected(LimitState),
    Finished(ReadingResult, LimitState),
}

impl Client {
    /// Create a client which sends `rps` requests per second on average.
    fn new_with_rps(rps: f64) -> Self {
        Self {
            interarrival: Exp::new(rps).unwrap(),
        }
    }

    fn next_arrival_in(&mut self, rng: &mut SmallRng) -> Duration {
        let dt = self.interarrival.sample(rng);
        Duration::from_secs_f64(dt)
    }
}

impl Server {
    /// Create a server with a concurrency limiter, a latency distribution and a failure rate.
    ///
    /// The latency is calculated according to the number of tasks needed to be performed and the
    /// average rate of completion of these tasks (per second).
    fn new(limiter: Limiter<LimitWrapper>, tasks: u64, task_rate: f64, failure_rate: f64) -> Self {
        assert!((0.0..=1.0).contains(&failure_rate));
        Self {
            limiter,
            latency: Erlang::new(tasks, task_rate).unwrap(),
            failure_rate,
        }
    }

    /// Start processing a request.
    fn start(&self, rng: &mut SmallRng) -> Result<Request, LimitState> {
        let limit_state = LimitState {
            limit: self.limiter.limit(),
            available: self.limiter.available(),
        };
        self.limiter
            .try_acquire()
            .map(|timer| Request {
                latency: Duration::from_secs_f64(self.latency.sample(rng)),
                timer,
                limit_state,
            })
            .ok_or(limit_state)
    }

    /// Finish processing a request.
    async fn end(&self, timer: Timer<'_>, rng: &mut SmallRng) -> RequestResult {
        let result = if rng.gen_range(0.0..=1.0) > self.failure_rate {
            ReadingResult::Success
        } else {
            ReadingResult::Overload
        };

        self.limiter.record_reading(timer, result).await;

        RequestResult {
            result,
            limit_state: LimitState {
                limit: self.limiter.limit(),
                available: self.limiter.available(),
            },
        }
    }
}

impl PartialEq for Event<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.time.eq(&other.time)
    }
}
impl Eq for Event<'_> {}
impl PartialOrd for Event<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Event<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

impl Simulation {
    async fn run(&mut self) -> Summary {
        tokio::time::pause();
        let start = Instant::now();

        let seed = rand::random();
        let mut rng = SmallRng::seed_from_u64(seed);
        println!("Seed: {seed}");

        // Priority queue of events (min heap).
        let mut queue = BinaryHeap::new();
        queue.push(Reverse(Event {
            time: start,
            typ: EventType::StartRequest,
        }));

        let mut requests = vec![];
        let mut event_log = vec![];

        while let Some(Reverse(event)) = queue.pop() {
            let dt = event.time.duration_since(Instant::now());
            tokio::time::advance(dt).await;
            let current_time = Instant::now();

            match event.typ {
                EventType::StartRequest => {
                    match self.server.start(&mut rng) {
                        Ok(req) => {
                            queue.push(Reverse(Event {
                                time: current_time + req.latency,
                                typ: EventType::EndRequest {
                                    start_time: current_time,
                                    timer: req.timer,
                                    original_limit_state: req.limit_state,
                                },
                            }));
                            event_log.push(EventLogEntry::Accepted(req.limit_state));
                        }
                        Err(limit_state) => {
                            event_log.push(EventLogEntry::Rejected(limit_state));
                        }
                    };

                    if current_time.duration_since(start) < self.duration {
                        let dt = self.client.next_arrival_in(&mut rng);
                        let event = Event {
                            time: current_time + dt,
                            typ: EventType::StartRequest,
                        };
                        queue.push(Reverse(event));
                    }
                }

                EventType::EndRequest {
                    start_time,
                    timer,
                    original_limit_state,
                } => {
                    let result = self.server.end(timer, &mut rng).await;
                    requests.push(RequestSummary {
                        start_time,
                        end_time: current_time,
                        latency: current_time.duration_since(start_time),
                        start_state: original_limit_state,
                        end_state: result.limit_state,
                        result: result.result,
                    });
                    event_log.push(EventLogEntry::Finished(result.result, result.limit_state));
                }
            }
        }

        Summary {
            event_log,
            requests,
        }
    }
}

impl Summary {
    fn requests(&self) -> usize {
        self.event_log
            .iter()
            .filter(|el| matches!(el, EventLogEntry::Accepted(_) | EventLogEntry::Rejected(_)))
            .count()
    }
    fn rejected(&self) -> usize {
        self.event_log
            .iter()
            .filter(|el| matches!(el, EventLogEntry::Rejected(_)))
            .count()
    }
}

#[tokio::test]
async fn test() {
    let mut simulation = Simulation {
        duration: Duration::from_secs(1),

        client: Client::new_with_rps(20.0),
        server: Server::new(
            Limiter::new(LimitWrapper::Aimd(
                AIMDLimit::new_with_limit(20)
                    .decrease_factor(0.9)
                    .increase_by(1),
            )),
            2,
            10.0,
            0.01,
        ),
    };
    let summary = simulation.run().await;

    println!("{:#?}", summary.event_log);

    println!("Requests: {}", summary.requests());
    println!("Rejected: {}", summary.rejected());
}
