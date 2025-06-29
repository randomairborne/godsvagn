pub mod generate_files;

use std::{net::SocketAddr, ops::Deref, sync::Arc};

use parking_lot::Mutex;

use pgp::composed::SignedSecretKey;

use filemeta::FileMeta;

#[derive(Clone)]
pub struct AppState(Arc<InnerAppState>);

impl Deref for AppState {
    type Target = InnerAppState;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl AppState {
    pub fn new(inner: InnerAppState) -> Self {
        Self(Arc::new(inner))
    }
}

#[derive(Debug)]
pub struct InnerAppState {
    pub db: Mutex<rusqlite::Connection>,
    pub key: SignedSecretKey,
    pub config: Config,
}

#[derive(serde::Deserialize, Debug)]
pub struct Config {
    pub server: Server,
    pub release: ConfigReleaseMetadata,
}

#[derive(serde::Deserialize, Debug)]
pub struct Server {
    pub bind: SocketAddr,
    pub key_path: String,
    pub database_path: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct ConfigReleaseMetadata {
    pub origin: String,
    pub label: String,
    pub suite: String,
    pub codename: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sql error")]
    Sql(#[from] rusqlite::Error),
    #[error("deb parse error: {0}")]
    ParseDeb(#[from] parsedeb::ParseError),
}
