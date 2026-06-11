use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingFile {
    pub language: String,
    pub project_root: String,
}
