use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use nanoid::nanoid;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use std::{env, sync::Arc};
use tokio::net::TcpListener;

struct AppState {
    pool: Pool<Postgres>,
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
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(e) => {
            if e.not_found() {
                Ok(())
            } else {
                Err(e)
            }
        }
    }?;
    let state = Arc::new(AppState {
        pool: PgPoolOptions::new()
            .connect(&env::var("DATABASE_URL")?)
            .await?,
    });
    let app = Router::new()
        .route("/id", get(generate_id))
        .with_state(state);
    let addr = env::var("ADDR").unwrap_or("127.0.0.1:3000".to_string());
    let listener = TcpListener::bind(&addr).await?;
    println!("listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

const DIRECTORY: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

async fn generate_id(State(state): State<Arc<AppState>>) -> Result<String, AppError> {
    let mut id;
    loop {
        id = nanoid!(10, &DIRECTORY);
        let ids = sqlx::query!("SELECT COUNT(*) FROM ids WHERE id = $1", id)
            .fetch_one(&state.pool)
            .await?;
        if ids.count.unwrap() == 0 {
            break;
        }
    }
    sqlx::query!("INSERT INTO ids (id) VALUES ($1)", id)
        .execute(&state.pool)
        .await?;
    Ok(id)
}
