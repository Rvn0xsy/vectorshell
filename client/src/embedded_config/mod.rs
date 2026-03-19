pub const SERVER_URL: &str = match option_env!("VECTOR_SERVER_URL") {
    Some(value) => value,
    None => "ws://127.0.0.1:8080/ws",
};

pub const AUTH_TOKEN: &str = match option_env!("VECTOR_AUTH_TOKEN") {
    Some(value) => value,
    None => "vectorshell-secret",
};

pub const RECONNECT_INTERVAL_SECS: u64 = 5;

pub const INSECURE_TLS_RAW: &str = match option_env!("VECTOR_INSECURE_TLS") {
    Some(value) => value,
    None => "false",
};
