use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use godsvagn::{AppState, InnerAppState};
use pgp::composed::{Deserializable, SignedSecretKey};
use rusqlite::{Connection, OpenFlags};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::args_os()
        .nth(1)
        .ok_or("1 argument required: path to config")?;
    let config = std::fs::read_to_string(config_path)?;
    let config: godsvagn::Config = toml::from_str(&config)?;

    let key = SignedSecretKey::from_armor_file(&config.server.key_path)?.0;
    let db = Connection::open_with_flags(&config.server.database_path)?;

    let state = AppState::new(InnerAppState { config, db, key });

    let listener = TcpListener::bind(state.config.server.bind).await?;
    let app = Router::new()
        .route("/upload", post(upload))
        .with_state(state);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn upload(State(state): State<AppState>) -> Result<Json<UploadSuccess>, Error> {
    Ok(Json(UploadSuccess {}))
}

#[derive(serde::Serialize)]
pub struct UploadSuccess {}

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::EXPECTATION_FAILED, "failed").into_response()
    }
}
