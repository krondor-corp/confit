use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("confit.toml not found in current directory or any parent")]
    ConfigNotFound,

    #[error("{0}")]
    Lookup(String),

    #[error("{0}")]
    Runtime(String),

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse config: {0}")]
    Toml(#[from] toml::de::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
