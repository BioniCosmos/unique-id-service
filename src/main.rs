use anyhow::{Context, anyhow};
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use nanoid::{alphabet::HEX_LOWERCASE, nanoid};
use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{BufRead, BufReader, Write as _},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

struct AppState {
    ids: HashSet<String>,
    store: File,
}

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(value: E) -> Self {
        Self(value.into())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let store = File::options()
        .create(true)
        .append(true)
        .read(true)
        .open(
            Path::new(
                &env::var("STATE_DIRECTORY")
                    .context("failed to read environment variable `STATE_DIRECTORY`")?,
            )
            .join("store"),
        )
        .context("failed to open the store file")?;
    let ids = BufReader::new(&store)
        .lines()
        .collect::<Result<_, _>>()
        .context("failed to read store")?;
    let state = Arc::new(Mutex::new(AppState { ids, store }));
    let app = Router::new()
        .route("/id", get(generate_id))
        .with_state(state);
    let addr = env::var("ADDR").unwrap_or("127.0.0.1:3000".to_string());
    let listener = TcpListener::bind(&addr).await?;
    println!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn generate_id(State(state): State<Arc<Mutex<AppState>>>) -> Result<String, AppError> {
    let AppState { ids, store } = &mut *state.lock().unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() <= end {
        let id = nanoid!(1, &HEX_LOWERCASE);
        if ids.insert(id.clone()) {
            writeln!(store, "{id}")?;
            return Ok(id);
        }
    }
    Err(AppError(anyhow!("generation time exceeded")))
}
