use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, ErrorKind as IoErrorKind, Seek, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use axum::{
    Router,
    body::Body,
    extract::{Query, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::post,
};
use bytes::Bytes;
use futures_util::StreamExt;
use jsonwebtoken::{DecodingKey, Validation, jwk::JwkSet};
use parsedeb::RequiredFields;
use rand::{Rng, distr::Alphabetic};
use reqwest::StatusCode;
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc::Receiver as MpscReceiver},
};

#[derive(serde::Deserialize, Debug)]
pub struct Config {
    pub server: ServerConfig,
}

#[derive(serde::Deserialize, Debug)]
pub struct ServerConfig {
    bind: SocketAddr,
    deb_directory: PathBuf,
    repo_directory: PathBuf,
    audiences: Box<[String]>,
    keyfile: PathBuf,
    #[serde(default = "default_repogen")]
    repogen_command: String,
}

fn default_repogen() -> String {
    "godsvagn-repogen".to_owned()
}

#[derive(argh::FromArgs)]
#[argh(description = "Generate a valid debian repository from a directory full of .deb files")]
struct Args {
    #[argh(option, short = 'c')]
    /// config file for godsvagn
    config: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = argh::from_env();
    let config = std::fs::read_to_string(&args.config)?;
    let config: Config = toml::from_str(&config)?;

    let http = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let jwks: JwkSet = http
        .get("https://token.actions.githubusercontent.com/.well-known/jwks")
        .send()
        .await?
        .json()
        .await?;

    let listener = TcpListener::bind(&config.server.bind).await?;

    let state = AppState {
        jwks: Arc::new(jwks),
        file_ops_pending: Arc::new(Mutex::new(())),
        config: Arc::new(config),
        config_path: args.config.into(),
    };

    let app = Router::new()
        .route("/upload", post(upload))
        .route("/regenerate", post(regenerate))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            claim_validator,
        ))
        .with_state(state);

    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    file_ops_pending: Arc<Mutex<()>>,
    jwks: Arc<JwkSet>,
    config: Arc<Config>,
    config_path: Arc<Path>,
}

#[derive(Debug, serde::Deserialize, Clone)]
#[allow(unused)]
struct Claims {
    aud: String, // Optional. Audience
    exp: usize, // Required (validate_exp defaults to true in validation). Expiration time (as UTC timestamp)
    iat: usize, // Optional. Issued at (as UTC timestamp)
    iss: String, // Optional. Issuer
    nbf: usize, // Optional. Not Before (as UTC timestamp)
    sub: String, // Optional. Subject (whom token refers to)
    #[serde(flatten)]
    more: HashMap<String, String>,
}

async fn claim_validator(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Error> {
    let jwt = request
        .headers()
        .get("openid-token")
        .ok_or(Error::MissingHeader)?
        .to_str()?
        .trim();
    let header = jsonwebtoken::decode_header(jwt)?;
    let relevant_jwk = state
        .jwks
        .find(&header.kid.ok_or(Error::NoKeyId)?)
        .ok_or(Error::UnknownJwk)?;
    let key = DecodingKey::from_jwk(relevant_jwk)?;
    let mut validator = Validation::new(jsonwebtoken::Algorithm::RS256);
    validator.set_audience(&state.config.server.audiences);
    validator.set_issuer(&["https://token.actions.githubusercontent.com"]);

    let claims: Claims = jsonwebtoken::decode(jwt, &key, &validator)?.claims;
    request.extensions_mut().insert(claims);
    Ok(next.run(request).await)
}

async fn regenerate(State(state): State<AppState>) -> Result<(), Error> {
    let guard = state.file_ops_pending.lock().await;
    let output_tmp = tempfile::tempdir()?;
    let mut cmd = tokio::process::Command::new(state.config.server.repogen_command.as_str());
    cmd.arg("--config").arg(state.config_path.as_os_str());
    cmd.arg("--output-dir").arg(output_tmp.path());
    cmd.arg("--input-dir")
        .arg(&state.config.server.deb_directory);
    cmd.arg("--keyfile").arg(&state.config.server.keyfile);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let output = cmd.spawn()?.wait().await?;
    if output.success() {
        let dir_suffix: String = rand::rng()
            .sample_iter(Alphabetic)
            .take(16)
            .map(char::from)
            .collect();
        let to_delete = std::env::temp_dir().join(dir_suffix);
        std::fs::rename(&state.config.server.repo_directory, &to_delete)?;
        std::fs::rename(output_tmp, &state.config.server.repo_directory)?;
        std::fs::remove_dir_all(&to_delete)?;
    } else {
        return Err(Error::GenerateFailed);
    }
    drop(guard);
    Ok(())
}

#[derive(serde::Deserialize)]
pub struct UploadQuery {
    #[serde(default = "falsey")]
    ignore_exists: bool,
}

fn falsey() -> bool {
    false
}

async fn upload(
    State(state): State<AppState>,
    Query(UploadQuery { ignore_exists }): Query<UploadQuery>,
    body: Body,
) -> Result<(), Error> {
    let (output_tx, output) = tokio::sync::oneshot::channel();
    // this block drops the body sender to prevent a deadlock of the background task
    // waiting for more data, while the "no more data" signal sent by dropping bytes_tx will
    // never be sent because it would be waiting on output.await
    {
        let (bytes_tx, bytes_rx) = tokio::sync::mpsc::channel(50);
        let deb_dir = state.config.server.deb_directory.clone();
        std::thread::spawn(move || {
            let o = deb_channel_to_storage(bytes_rx, &deb_dir);
            if let Err(e) = output_tx.send(o) {
                eprintln!("Failed to send output to parent thread: {e:?}");
            }
        });
        let mut body_stream = body.into_data_stream();
        while let Some(d) = body_stream.next().await.transpose()? {
            bytes_tx.send(d).await.map_err(Error::InvalidSend)?;
        }
    }
    match output.await.map_err(|_| Error::BackgroundCrashed)? {
        Err(Error::AlreadyExists) if ignore_exists => Ok(()),
        v => v,
    }
}

fn deb_channel_to_storage(
    mut bytes_rx: MpscReceiver<Bytes>,
    deb_directory: &Path,
) -> Result<(), Error> {
    let mut tmp = tempfile::tempfile()?;
    while let Some(val) = bytes_rx.blocking_recv() {
        tmp.write_all(&val)?;
    }
    tmp.rewind()?;
    find_location_and_move_deb_to_storage(tmp, deb_directory)?;
    Ok(())
}

fn find_location_and_move_deb_to_storage(
    mut work_file: File,
    deb_directory: &Path,
) -> Result<(), Error> {
    let (values, _raw) = parsedeb::deb_to_control(&work_file)?;
    let RequiredFields {
        package: name,
        architecture,
        version,
        ..
    } = RequiredFields::from_map(&values).ok_or(Error::MissingField)?;

    let outfile_path = deb_directory.join(format!(
        "{architecture}/{name}_{version}_{architecture}.deb"
    ));
    let outfile = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(outfile_path)
        .map_err(|e| {
            if matches!(e.kind(), IoErrorKind::AlreadyExists) {
                Error::AlreadyExists
            } else {
                Error::Io(e)
            }
        })?;

    work_file.rewind()?;
    std::io::copy(&mut BufReader::new(work_file), &mut BufWriter::new(outfile))?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("invalid jwt")]
    NoKeyId,
    #[error("invalid jwt")]
    UnknownJwk,
    #[error("missing header")]
    MissingHeader,
    #[error("missing controlfile field")]
    MissingField,
    #[error("background task crashed")]
    BackgroundCrashed,
    #[error("regenerate failed")]
    GenerateFailed,
    #[error("already exists")]
    AlreadyExists,
    #[error("invalid jwt")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("invalid header")]
    HeaderIsInvalidStr(#[from] reqwest::header::ToStrError),
    #[error("i/o error")]
    Io(#[from] std::io::Error),
    #[error("i/o channel error")]
    InvalidSend(tokio::sync::mpsc::error::SendError<Bytes>),
    #[error("body error")]
    Axum(#[from] axum::Error),
    #[error("invalid deb file: {0}")]
    DebParse(#[from] parsedeb::Error),
    #[error("task panicked")]
    TaskPanic(#[from] tokio::task::JoinError),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        eprintln!("{self:?}");
        (StatusCode::BAD_REQUEST, self.to_string()).into_response()
    }
}
