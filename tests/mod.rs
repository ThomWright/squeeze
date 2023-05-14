use std::{cmp::Reverse, collections::BinaryHeap, time::Duration};

use rand::{prelude::Distribution, rngs::SmallRng, Rng, SeedableRng};
use statrs::distribution::{Erlang, Exp};

use squeeze::{
    limit::{AIMDLimit, LimitAlgorithm},
    Limiter, ReadingResult, Timer,
};
use tokio::time::Instant;

struct Client {
    /// Poisson process, exponential interarrival times.
    interarrival: Exp,
}

struct Server<T> {
    limiter: Limiter<T>,

    latency: Erlang,

    /// Range: [0, 1)
    failure_rate: f64,
}

struct Request<'t> {
    latency: Duration,
    timer: Timer<'t>,

    /// Limit available when before request was made.
    limit: usize,
    /// Available concurrency remaining before the request was made.
    available: usize,
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
        limit: usize,
        available: usize,
        start_time: Instant,
        timer: Timer<'t>,
    },
}

#[derive(Debug)]
struct Result {
    start_time: Instant,
    end_time: Instant,
    limit: usize,
    available: usize,
    latency: Duration,
    result: ReadingResult,
}

impl Client {
    /// Create a client which sends `rps` requests per second on average.
    fn new(rps: f64) -> Self {
        Self {
            interarrival: Exp::new(rps).unwrap(),
        }
    }

    fn next_arrival_in(&mut self, rng: &mut SmallRng) -> Duration {
        let dt = self.interarrival.sample(rng);
        Duration::from_secs_f64(dt)
    }
}

impl<T> Server<T>
where
    T: LimitAlgorithm,
{
    /// Create a server with a concurrency limiter, a latency distribution and a failure rate.
    ///
    /// The latency is calculated according to the number of tasks needed to be performed and the
    /// average rate of completion of these tasks (per second).
    fn new(limiter: Limiter<T>, tasks: u64, task_rate: f64, failure_rate: f64) -> Self {
        assert!((0.0..=1.0).contains(&failure_rate));
        Self {
            limiter,
            latency: Erlang::new(tasks, task_rate).unwrap(),
            failure_rate,
        }
    }

    ///
    fn start(&self, rng: &mut SmallRng) -> Option<Request> {
        let limit = self.limiter.limit();
        let available = self.limiter.available();
        self.limiter.try_acquire().map(|timer| Request {
            latency: Duration::from_secs_f64(self.latency.sample(rng)),
            timer,
            limit,
            available,
        })
    }

    async fn end(&self, timer: Timer<'_>, rng: &mut SmallRng) -> ReadingResult {
        let result = if rng.gen_range(0.0..=1.0) > self.failure_rate {
            ReadingResult::Success
        } else {
            ReadingResult::Overload
        };

        self.limiter.record_reading(timer, result).await;

        result
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

/// Precondition: time has been paused (can't seem to assert this).
async fn simulate(max_time: Duration) -> Vec<Result> {
    let start = Instant::now();

    let seed = rand::random();
    let mut rng = SmallRng::seed_from_u64(seed);
    println!("Seed: {seed}");

    let mut client = Client::new(10.0);
    let server = Server::new(
        Limiter::new(
            AIMDLimit::new_with_limit(10)
                .decrease_factor(0.9)
                .increase_by(1),
        ),
        2,
        10.0,
        0.01,
    );

    // Priority queue of events (min heap).
    let mut queue = BinaryHeap::new();
    queue.push(Reverse(Event {
        time: start,
        typ: EventType::StartRequest,
    }));

    let mut results = vec![];

    while let Some(Reverse(event)) = queue.pop() {
        tokio::time::advance(event.time.duration_since(Instant::now())).await;
        let current_time = Instant::now();

        match event.typ {
            EventType::StartRequest => {
                if let Some(req) = server.start(&mut rng) {
                    queue.push(Reverse(Event {
                        time: current_time + req.latency,
                        typ: EventType::EndRequest {
                            start_time: current_time,
                            timer: req.timer,
                            limit: req.limit,
                            available: req.available,
                        },
                    }));
                }
            }

            EventType::EndRequest {
                start_time,
                timer,
                limit,
                available,
            } => {
                let result = server.end(timer, &mut rng).await;
                results.push(Result {
                    start_time,
                    end_time: current_time,
                    latency: current_time.duration_since(start_time),
                    limit,
                    available,
                    result,
                });
            }
        }

        if current_time.duration_since(start) < max_time {
            let dt = client.next_arrival_in(&mut rng);
            let event = Event {
                time: current_time + dt,
                typ: EventType::StartRequest,
            };
            queue.push(Reverse(event));
        }
    }

    results
}

#[tokio::test(start_paused = true)]
async fn test() {
    let results = simulate(Duration::from_secs(5)).await;

    println!("{results:#?}");
}
