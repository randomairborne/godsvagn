#[derive(serde::Deserialize, Debug)]
pub struct Config {
    pub release: ConfigReleaseMetadata,
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
    #[error("deb parse error: {0}")]
    ParseDeb(#[from] parsedeb::ParseError),
}
