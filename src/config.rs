#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub request_timeout_ms: u64,
}

impl Config {
    pub fn load() -> Self {
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        let request_timeout_ms = std::env::var("REQUEST_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(5000);

        Self {
            port,
            request_timeout_ms,
        }
    }
}
