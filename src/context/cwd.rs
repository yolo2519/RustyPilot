use std::env;

#[derive(Clone, Default)]
pub struct CurrentDir {
    pub path: String,
}

impl CurrentDir {
    pub fn capture() -> Option<Self> {
        let path = env::current_dir().ok()?.to_string_lossy().to_string();
        Some(Self { path })
    }
}
