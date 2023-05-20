use std::{cmp::Reverse, collections::BinaryHeap, time::Duration};

use itertools::Itertools;
use rand::{prelude::Distribution, rngs::SmallRng, Rng, SeedableRng};
use statrs::{
    distribution::{Erlang, Exp},
    statistics::Distribution as StatDist,
};
use tokio::time::Instant;

use squeeze::{
    limit::{AimdLimit, LimitAlgorithm},
    Limiter, ReadingResult, Timer,
};

mod iter_ext;

use iter_ext::MeanExt;

struct Simulation {
    duration: Duration,
    client: Client,
    server: Server,
}

enum LimitWrapper {
    Aimd(AimdLimit),
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

/// Latency is calculated according to the number of tasks needing to be performed and the
/// average rate of completion of these tasks (per second).
struct LatencyProfile {
    tasks: u64,
    task_rate: f64,
}

struct Request<'t> {
    latency: Duration,
    timer: Timer<'t>,

    /// Limiter state just after the request started.
    limit_state: LimitState,
}

struct RequestResult {
    result: ReadingResult,

    /// Limiter state just after the request finished.
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
        /// Limiter state just after the request started.
        original_limit_state: LimitState,
        timer: Timer<'t>,
    },
}

struct Summary {
    started_at: Instant,
    event_log: Vec<EventLogEntry>,
    requests: Vec<RequestSummary>,
}

#[derive(Debug)]
struct RequestSummary {
    start_time: Instant,
    /// Limiter state just after this request was accepted/rejected.
    start_state: LimitState,
    /// Limiter state just after this request finished.
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

    fn rps(&self) -> f64 {
        self.interarrival.rate()
    }
}

impl Server {
    /// Create a server with a concurrency limiter, a latency distribution and a failure rate.
    fn new(
        limiter: Limiter<LimitWrapper>,
        latency_profile: LatencyProfile,
        failure_rate: f64,
    ) -> Self {
        assert!((0.0..=1.0).contains(&failure_rate));
        Self {
            limiter,
            latency: Erlang::from(latency_profile),
            failure_rate,
        }
    }

    /// Start processing a request.
    fn start(&self, rng: &mut SmallRng) -> Result<Request, LimitState> {
        self.limiter
            .try_acquire()
            .map(|timer| Request {
                latency: Duration::from_secs_f64(self.latency.sample(rng)),
                timer,
                limit_state: LimitState {
                    limit: self.limiter.limit(),
                    available: self.limiter.available(),
                },
            })
            .ok_or(LimitState {
                limit: self.limiter.limit(),
                available: self.limiter.available(),
            })
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

    fn mean_latency(&self) -> f64 {
        self.latency.mean().unwrap()
    }
}

impl From<LatencyProfile> for Erlang {
    fn from(lp: LatencyProfile) -> Self {
        Erlang::new(lp.tasks, lp.task_rate).unwrap()
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

impl<'t> EventType<'_> {
    fn type_name(&self) -> String {
        match self {
            EventType::StartRequest => "StartRequest".to_string(),
            EventType::EndRequest { .. } => "EndRequest".to_string(),
        }
    }
}

impl Simulation {
    async fn run(&mut self) -> Summary {
        tokio::time::pause();
        let start = Instant::now();

        let seed = rand::random();
        let mut rng = SmallRng::seed_from_u64(seed);
        println!("Seed: {seed}\n");

        // Priority queue of events (min heap).
        let mut queue = BinaryHeap::new();
        queue.push(Reverse(Event {
            time: start,
            typ: EventType::StartRequest,
        }));

        let mut requests = vec![];
        let mut event_log = vec![];

        let mut current_time = start;
        while let Some(Reverse(event)) = queue.pop() {
            let dt = event.time.duration_since(current_time);
            tokio::time::advance(dt).await;
            current_time = Instant::now();

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

        requests.sort_by(|r1, r2| r1.start_time.cmp(&r2.start_time));
        Summary {
            started_at: start,
            event_log,
            requests,
        }
    }
}

impl LimitState {
    fn concurrency(&self) -> usize {
        self.limit - self.available
    }
}

impl EventLogEntry {
    fn limit_state(&self) -> LimitState {
        match self {
            EventLogEntry::Accepted(ls) => *ls,
            EventLogEntry::Rejected(ls) => *ls,
            EventLogEntry::Finished(_, ls) => *ls,
        }
    }
}

impl Summary {
    fn total_requests(&self) -> usize {
        self.event_log
            .iter()
            .filter(|el| matches!(el, EventLogEntry::Accepted(_) | EventLogEntry::Rejected(_)))
            .count()
    }
    fn total_rejected(&self) -> usize {
        self.event_log
            .iter()
            .filter(|el| matches!(el, EventLogEntry::Rejected(_)))
            .count()
    }
    fn mean_latency(&self) -> Duration {
        self.requests.iter().map(|r| r.latency).sum::<Duration>() / self.total_requests() as u32
    }
    fn max_concurrency(&self) -> usize {
        self.event_log
            .iter()
            .map(|log| log.limit_state().concurrency())
            .max()
            .unwrap_or(0)
    }
    fn mean_interarrival_time(&self) -> Duration {
        self.requests
            .iter()
            .map(|r| r.start_time)
            .tuple_windows()
            .map(|(a, b)| b - a)
            .mean()
    }

    fn print_summary(&self) {
        // println!("{:#?}", summary.event_log);
        // println!("{:#?}", summary.requests);

        println!("Summary");
        println!("=======");

        println!("Requests: {}", self.total_requests());
        println!("Rejected: {}", self.total_rejected());

        println!(
            "Mean interarrival time: {:#?}",
            self.mean_interarrival_time()
        );

        println!("Mean latency: {:#?}", self.mean_latency());
        println!("Max. concurrency: {:#?}", self.max_concurrency());
    }
}

#[tokio::test]
async fn test() {
    let client = Client::new_with_rps(100.0);

    let server = Server::new(
        Limiter::new(LimitWrapper::Aimd(
            AimdLimit::new_with_initial_limit(10)
                .with_max_limit(20)
                .decrease_factor(0.9)
                .increase_by(1),
        )),
        LatencyProfile {
            tasks: 2,
            task_rate: 10.0,
        },
        0.01,
    );

    println!("Client");
    println!("======");
    println!("RPS: {}", client.rps());
    println!();
    println!("Server");
    println!("======");
    println!("Mean latency: {}", server.mean_latency());
    println!();
    // TODO: print limiter info

    let mut simulation = Simulation {
        duration: Duration::from_secs(1),

        client,
        server,
    };

    let summary = simulation.run().await;

    summary.print_summary();
}
