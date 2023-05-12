use squeeze::Limiter;
use uuid::Uuid;

struct Service<L> {
    id: Uuid,
    server_limiter: Limiter<L>,
    endpoints: Vec<Endpoint>,
    latency: (), // TODO: latency distribution
}

struct Endpoint {
    id: Uuid,
    dependencies: Vec<Uuid>,
}

struct Source {}

struct Sink {
    id: Uuid,
}

struct Queue {
    id: Uuid,
}

struct Exchange {
    id: Uuid,
}

struct Message {
    id: Uuid,
    source: Uuid,
    destination: Uuid,
}
