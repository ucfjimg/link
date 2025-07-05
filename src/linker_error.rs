#[derive(Debug)]
pub struct LinkerError {
    what: String,
}

impl LinkerError {
    pub fn new(what: &str ) -> Self {
        LinkerError { what: what.to_owned() }
    }
}

impl std::fmt::Display for LinkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.what)
    }
}

impl std::error::Error for LinkerError {
}

impl From<std::io::Error> for LinkerError {
    fn from(e: std::io::Error) -> Self {
        LinkerError::new(e.to_string().as_str())
    }
}
