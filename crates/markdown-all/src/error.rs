use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("section not found: {0}")]
    SectionNotFound(String),

    #[error("field not found: {0}")]
    FieldNotFound(String),

    #[error("table not found at index {0}")]
    TableNotFound(usize),

    #[error("cell not found: column={col}, row={row}")]
    CellNotFound { col: String, row: usize },

    #[error("frontmatter parse error: {0}")]
    FrontmatterParse(String),

    #[error("no frontmatter in document")]
    NoFrontmatter,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
