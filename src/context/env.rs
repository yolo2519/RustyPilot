#[derive(Clone)]
pub struct Environment {
    pub vars: Vec<(String, String)>,
}

impl Environment {
    pub fn capture() -> Self {
        let vars = std::env::vars().collect();
        Self { vars }
    }
}
