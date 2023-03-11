use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, TypedHeader, WebSocketUpgrade,
    },
    http::Response,
    response::{sse::Event, sse::Sse, Html, IntoResponse},
    routing::get,
    Json, Router, Server,
};
use futures::stream::{self, Stream};
use serde::Serialize;
use sysinfo::{CpuExt, System, SystemExt};
use tokio::sync::broadcast;
use tokio_stream::StreamExt as _;

type Snapshot = Vec<f32>;
type MemorySnapshot = MemoryUsage;

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel::<Snapshot>(1);

    let (txm, _) = broadcast::channel::<MemorySnapshot>(1);

    tracing_subscriber::fmt::init();
    let app_state = AppState {
        tx: tx.clone(),
        txm: txm.clone(),
        sys: Arc::new(Mutex::new(System::new())),
    };

    let router = Router::new()
        .route("/", get(root_get))
        .route("/index.mjs", get(indexmjs_get))
        .route("/index.css", get(indexcss_get))
        .route("/realtime/cpus", get(realtime_cpus_get))
        .route("/realtime/memory", get(memory_get))
        .route("/sse", get(sse_handler))
        .route("/memory-sse", get(sse_handler_memory))
        .with_state(app_state.clone());

    // Update CPU usage in the background
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        loop {
            sys.refresh_cpu();
            let v: Vec<_> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();
            let _ = tx.send(v);
            std::thread::sleep(System::MINIMUM_CPU_UPDATE_INTERVAL);
        }
    });

    // Update MEMORY usage in the background
    tokio::task::spawn_blocking(move || {
        let mut sys = System::new();
        loop {
            sys.refresh_memory();

            let memory_usage = MemoryUsage {
                freeMemory: sys.free_memory() / 1024 / 1024,
                usedMemory: sys.used_memory() / 1024 / 1024,
                totalMemory: sys.total_memory() / 1024 / 1024,
                availableMemory: sys.available_memory() / 1024 / 1024,
            };

            let _ = txm.send(memory_usage);
            //std::thread::sleep(System::MINI);
        }
    });

    let server = Server::bind(&"0.0.0.0:7032".parse().unwrap()).serve(router.into_make_service());
    let addr = server.local_addr();
    println!("Listening on {addr}");
    server.await.unwrap();
}
#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<Snapshot>,
    txm: broadcast::Sender<MemorySnapshot>,
    sys: Arc<Mutex<System>>,
}

#[axum::debug_handler]
async fn root_get() -> impl IntoResponse {
    let markup = tokio::fs::read_to_string("src/index.html").await.unwrap();
    Html(markup)
}

#[axum::debug_handler]
async fn indexmjs_get() -> impl IntoResponse {
    let javascript = tokio::fs::read_to_string("src/index.mjs").await.unwrap();

    Response::builder()
        .header("content-type", "text/javascript;charset=utf-8")
        .body(javascript)
        .unwrap()
}

#[axum::debug_handler]
async fn indexcss_get() -> impl IntoResponse {
    let markup = tokio::fs::read_to_string("src/index.css").await.unwrap();

    Response::builder()
        .header("content-type", "text/css;charset=utf-8")
        .body(markup)
        .unwrap()
}

#[axum::debug_handler]
async fn realtime_cpus_get(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|ws: WebSocket| async { realtime_cpus_stream(state, ws).await })
}

async fn realtime_cpus_stream(app_state: AppState, mut ws: WebSocket) {
    let mut rx = app_state.tx.subscribe();

    while let Ok(msg) = rx.recv().await {
        ws.send(Message::Text(serde_json::to_string(&msg).unwrap()))
            .await
            .unwrap();
    }
}
#[allow(non_snake_case)]
#[derive(Serialize, Clone)]
struct MemoryUsage {
    freeMemory: u64,
    usedMemory: u64,
    totalMemory: u64,
    availableMemory: u64,
}

#[axum::debug_handler]
async fn memory_get(State(state): State<AppState>) -> impl IntoResponse {
    let mut sys = state.sys.lock().unwrap();

    sys.refresh_memory();
    let memory_usage = MemoryUsage {
        freeMemory: sys.free_memory() / 1024 / 1024,
        usedMemory: sys.used_memory() / 1024 / 1024,
        totalMemory: sys.total_memory() / 1024 / 1024,
        availableMemory: sys.available_memory() / 1024 / 1024,
    };

    // let free_memory = sys.free_memory();
    // let used_memory = sys.used_memory();

    // let total_memory = sys.total_memory();
    // let memory = sys.available_memory();

    //"Hi from Axum!"
    Json(memory_usage)
}

#[axum::debug_handler]
async fn sse_handler_memory(
    State(state): State<AppState>,
    TypedHeader(user_agent): TypedHeader<headers::UserAgent>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut rx = state.txm.subscribe();
        while let Ok(msg) = rx.recv().await {
            let payload = serde_json::to_string(&msg).unwrap();
            yield Ok(Event::default().data(payload));
        }

    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}

#[axum::debug_handler]
async fn sse_handler(
    TypedHeader(user_agent): TypedHeader<headers::UserAgent>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    println!("`{}` connected", user_agent.as_str());

    // A `Stream` that repeats an event every second
    let stream = stream::repeat_with(|| Event::default().data("hi!"))
        .map(Ok)
        .throttle(Duration::from_secs(1));

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}
