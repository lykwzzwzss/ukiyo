/*
 * Built on example code from:
 * https://github.com/omjadas/hudsucker/blob/main/examples/log.rs
 */

use once_cell::sync::Lazy;
use std::{path::PathBuf, str::FromStr, sync::Mutex};

use hudsucker::{
  async_trait::async_trait,
  certificate_authority::RcgenAuthority,
  hyper::{Body, Request, Response, StatusCode},
  *,
};
use rcgen::*;

use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use rustls_pemfile as pemfile;
use tauri::http::Uri;

#[cfg(windows)]
use registry::{Data, Hive, Security};


struct RunningProxy {
  port: u16,
  task: tokio::task::JoinHandle<()>,
  shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}
#[derive(Default)]
struct ProxyState { running: Option<RunningProxy>, generation: u64 }
static STATE: Lazy<Mutex<ProxyState>> = Lazy::new(|| Mutex::new(ProxyState::default()));
static START_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

pub async fn start_managed_proxy(port: u16, certificate_path: String) -> Result<(), String> {
  let _start = START_LOCK.lock().await;
  let (previous, generation) = {
    let mut state = STATE.lock().map_err(|e| e.to_string())?;
    if let Some(running) = &state.running {
      if running.port == port && running.shutdown.is_some() && !running.task.is_finished() { return Ok(()); }
    }
    (state.running.take(), state.generation)
  };
  if let Some(mut previous) = previous {
    if let Some(shutdown) = previous.shutdown.take() { let _ = shutdown.send(()); }
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut previous.task).await.is_err() {
      previous.task.abort();
      let _ = previous.task.await;
    }
  }
  let mut state = STATE.lock().map_err(|e| e.to_string())?;
  if state.generation != generation { return Err("Proxy connection was cancelled".into()); }
  let (shutdown, signal) = tokio::sync::oneshot::channel();
  let task = match create_proxy(port, certificate_path, signal, generation) {
    Ok(task) => task,
    Err(error) => { restore_system_proxy()?; return Err(error); }
  };
  if let Err(error) = enable_system_proxy(port) { task.abort(); return Err(error); }
  state.running = Some(RunningProxy { port, task, shutdown: Some(shutdown) });
  Ok(())
}

pub fn game_proxy_url() -> Result<Option<String>, String> {
  let state = STATE.lock().map_err(|e| e.to_string())?;
  Ok(state.running.as_ref().filter(|running| running.shutdown.is_some() && !running.task.is_finished())
    .map(|running| format!("http://127.0.0.1:{}", running.port)))
}

pub fn stop_managed_proxy() -> Result<(), String> {
  let mut state = STATE.lock().map_err(|e| e.to_string())?;
  state.generation = state.generation.wrapping_add(1);
  if let Some(running) = &mut state.running {
    if let Some(shutdown) = running.shutdown.take() { let _ = shutdown.send(()); }
  }
  restore_system_proxy()
}

#[cfg(windows)]
fn enable_system_proxy(port: u16) -> Result<(), String> { connect_to_proxy(port) }
#[cfg(windows)]
fn restore_system_proxy() -> Result<(), String> { disconnect_from_proxy() }
#[cfg(not(windows))]
fn enable_system_proxy(port: u16) -> Result<(), String> { connect_to_proxy(port); Ok(()) }
#[cfg(not(windows))]
fn restore_system_proxy() -> Result<(), String> { disconnect_from_proxy(); Ok(()) }

// Global ver for getting server address.
static SERVER: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("http://localhost:443".to_string()));
#[derive(Clone)]
struct ProxyHandler;

#[tauri::command]
pub fn set_proxy_addr(addr: String) -> Result<(), String> {
  let addr = addr.trim().trim_end_matches('/');
  let uri = Uri::from_str(addr).map_err(|e| e.to_string())?;
  if !matches!(uri.scheme_str(), Some("http") | Some("https")) || uri.host().is_none()
    || uri.path() != "/" || uri.query().is_some() {
    return Err("请输入有效的服务器地址和端口".into());
  }
  *SERVER.lock().map_err(|e| e.to_string())? = addr.to_string();
  Ok(())
}

fn is_game_host(uri: &Uri) -> bool {
  let host = uri.host().unwrap_or("").to_ascii_lowercase();
  ["hoyoverse.com", "mihoyo.com", "yuanshen.com", "starrails.com", "bhsr.com", "bh3.com", "honkaiimpact3.com", "zenlesszonezero.com"]
    .iter().any(|domain| host == *domain || host.strip_suffix(domain).map_or(false, |prefix| prefix.ends_with('.')))
}

#[async_trait]
impl HttpHandler for ProxyHandler {
  async fn handle_request(
    &mut self,
    _ctx: &HttpContext,
    mut req: Request<Body>,
  ) -> RequestOrResponse {
    if is_game_host(req.uri()) {
      // Handle CONNECTs
      if req.method().as_str() == "CONNECT" {
        let builder = Response::builder()
          .header("DecryptEndpoint", "Created")
          .status(StatusCode::OK);
        let res = builder.body(()).unwrap();

        // Respond to CONNECT
        *res.body()
      } else {
        let uri_path_and_query = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
        // Create new URI.
        if let Ok(new_uri) = Uri::from_str(&format!("{}{}", SERVER.lock().unwrap_or_else(|e| e.into_inner()), uri_path_and_query)) {
          *req.uri_mut() = new_uri;
        }
      }
    }

    req.into()
  }

  async fn handle_response(
    &mut self,
    _context: &HttpContext,
    response: Response<Body>,
  ) -> Response<Body> {
    response
  }

  async fn should_intercept(&mut self, _ctx: &HttpContext, _req: &Request<Body>) -> bool {
    is_game_host(_req.uri())
  }
}

/**
 * Starts an HTTP(S) proxy server.
 */
fn create_proxy(
  proxy_port: u16,
  certificate_path: String,
  shutdown: tokio::sync::oneshot::Receiver<()>,
  generation: u64,
) -> Result<tokio::task::JoinHandle<()>, String> {
  let cert_path = PathBuf::from(certificate_path);
  let key_path = cert_path.join("private.key");
  let ca_path = cert_path.join("cert.crt");
  if !key_path.exists() || !ca_path.exists() {
    generate_ca_files(cert_path.parent().ok_or("Invalid certificate directory")?)?;
  }
  let key_bytes = fs::read(key_path).map_err(|e| e.to_string())?;
  let cert_bytes = fs::read(ca_path).map_err(|e| e.to_string())?;
  let private_key = rustls::PrivateKey(pemfile::pkcs8_private_keys(&mut key_bytes.as_slice())
    .map_err(|e| e.to_string())?.into_iter().next().ok_or("Missing private key")?);
  let ca_cert = rustls::Certificate(pemfile::certs(&mut cert_bytes.as_slice())
    .map_err(|e| e.to_string())?.into_iter().next().ok_or("Missing certificate")?);
  let authority = RcgenAuthority::new(private_key, ca_cert, 1000).map_err(|e| e.to_string())?;
  // Bind before changing system settings; only local clients need this listener.
  let listener = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], proxy_port))).map_err(|e| e.to_string())?;
  listener.set_nonblocking(true).map_err(|e| e.to_string())?;
  let server = hudsucker::hyper::Server::from_tcp(listener).map_err(|e| e.to_string())?;
  let proxy = ProxyBuilder::new().with_server(server).with_rustls_client().with_ca(authority)
    .with_http_handler(ProxyHandler).build();
  Ok(tokio::spawn(async move {
    let (graceful, signal) = tokio::sync::oneshot::channel();
    let serving = proxy.start(async { let _ = signal.await; });
    tokio::pin!(serving);
    let result = match futures_util::future::select(&mut serving, shutdown).await {
      futures_util::future::Either::Left((result, _)) => Some(result),
      futures_util::future::Either::Right((_, _)) => {
        let _ = graceful.send(());
        tokio::time::timeout(std::time::Duration::from_secs(2), &mut serving).await.ok()
      }
    };
    if let Some(Err(error)) = result {
      eprintln!("Proxy stopped unexpectedly: {}", error);
      let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
      if state.generation == generation {
        state.generation = state.generation.wrapping_add(1);
        if let Err(e) = restore_system_proxy() { eprintln!("Proxy rollback failed: {}", e); }
      }
    }
  }))
}

/**
 * Connects to the local HTTP(S) proxy server.
 */
#[cfg(windows)]
struct ProxySnapshot { server: Option<Data>, enabled: Option<Data> }
#[cfg(windows)]
static ORIGINAL_PROXY: Lazy<Mutex<Option<ProxySnapshot>>> = Lazy::new(|| Mutex::new(None));

#[cfg(windows)]
fn read_registry_value(settings: &registry::RegKey, name: &str) -> Result<Option<Data>, String> {
  match settings.value(name) {
    Ok(value) => Ok(Some(value)),
    Err(registry::value::Error::NotFound(..)) => Ok(None),
    Err(e) => Err(e.to_string()),
  }
}

#[cfg(windows)]
fn restore_value(settings: &registry::RegKey, name: &str, value: &Option<Data>) -> Result<(), String> {
  match value {
    Some(value) => settings.set_value(name, value).map_err(|e| e.to_string()),
    None => match settings.delete_value(name) {
      Ok(()) | Err(registry::value::Error::NotFound(..)) => Ok(()),
      Err(e) => Err(e.to_string()),
    },
  }
}

#[cfg(windows)]
fn notify_proxy_change() {
  #[link(name = "wininet")]
  extern "system" { fn InternetSetOptionW(handle: isize, option: u32, buffer: *const core::ffi::c_void, length: u32) -> i32; }
  unsafe { InternetSetOptionW(0, 39, std::ptr::null(), 0); InternetSetOptionW(0, 37, std::ptr::null(), 0); }
}

#[cfg(windows)]
pub fn connect_to_proxy(proxy_port: u16) -> Result<(), String> {
  let settings = Hive::CurrentUser.open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", Security::Read | Security::Write)
    .map_err(|e| e.to_string())?;
  let mut original = ORIGINAL_PROXY.lock().map_err(|e| e.to_string())?;
  if original.is_none() {
    *original = Some(ProxySnapshot {
      server: read_registry_value(&settings, "ProxyServer")?,
      enabled: read_registry_value(&settings, "ProxyEnable")?,
    });
  }
  let server = format!("http=127.0.0.1:{};https=127.0.0.1:{}", proxy_port, proxy_port);
  let result = settings.set_value("ProxyServer", &Data::String(server.as_str().try_into().map_err(|_| "Invalid proxy server string".to_string())?));
  let result = result.map_err(|e| e.to_string()).and_then(|_| settings.set_value("ProxyEnable", &Data::U32(1)).map_err(|e| e.to_string()));
  if result.is_err() {
    if let Some(snapshot) = original.as_ref() {
      restore_value(&settings, "ProxyServer", &snapshot.server)?;
      restore_value(&settings, "ProxyEnable", &snapshot.enabled)?;
    }
    *original = None;
  }
  notify_proxy_change();
  result
}

#[cfg(windows)]
pub fn disconnect_from_proxy() -> Result<(), String> {
  let mut original = ORIGINAL_PROXY.lock().map_err(|e| e.to_string())?;
  if let Some(snapshot) = original.as_ref() {
    let settings = Hive::CurrentUser.open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", Security::Read | Security::Write)
      .map_err(|e| e.to_string())?;
    restore_value(&settings, "ProxyServer", &snapshot.server)?;
    restore_value(&settings, "ProxyEnable", &snapshot.enabled)?;
    *original = None;
    notify_proxy_change();
  }
  Ok(())
}

#[tauri::command]
pub fn generate_ca_files(path: &Path) -> Result<(), String> {
  static INSTALLED: Lazy<Mutex<std::collections::HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(std::collections::HashSet::new()));
  let mut installed = INSTALLED.lock().map_err(|e| e.to_string())?;
  let cert_dir = path.join("ca");
  let cert_path = cert_dir.join("cert.crt");
  if cert_path.is_file() && cert_dir.join("private.key").is_file() {
    if !installed.contains(&cert_path) {
      #[cfg(windows)]
      install_ca_files(&cert_path)?;
      #[cfg(not(windows))]
      install_ca_files(&cert_path);
      installed.insert(cert_path);
    }
    return Ok(());
  }
  let mut params = CertificateParams::default();
  let mut details = DistinguishedName::new();

  // Set certificate details.
  details.push(DnType::CommonName, "Cultivation");
  details.push(DnType::OrganizationName, "Grasscutters");
  details.push(DnType::CountryName, "CN");
  details.push(DnType::LocalityName, "CN");

  // Set details in the parameter.
  params.distinguished_name = details;
  // Set other properties.
  params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
  params.key_usages = vec![
    KeyUsagePurpose::DigitalSignature,
    KeyUsagePurpose::KeyCertSign,
    KeyUsagePurpose::CrlSign,
  ];

  // Create certificate.
  let cert = Certificate::from_params(params).map_err(|e| e.to_string())?;
  let cert_crt = cert.serialize_pem().map_err(|e| e.to_string())?;
  let private_key = cert.serialize_private_key_pem();
  fs::create_dir_all(&cert_dir).map_err(|e| e.to_string())?;
  let cert_path = cert_dir.join("cert.crt");
  fs::write(&cert_path, cert_crt).map_err(|e| e.to_string())?;
  fs::write(cert_dir.join("private.key"), private_key).map_err(|e| e.to_string())?;
  #[cfg(windows)]
  install_ca_files(&cert_path)?;
  #[cfg(not(windows))]
  install_ca_files(&cert_path);
  installed.insert(cert_path);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn routes_only_actual_game_domains() {
    assert!(is_game_host(&"https://dispatch.mihoyo.com/path".parse().unwrap()));
    assert!(is_game_host(&"dispatch.yuanshen.com:12401".parse().unwrap()));
    assert!(!is_game_host(&"https://mihoyo.com.example.org/path".parse().unwrap()));
    assert!(!is_game_host(&"https://example.org/?next=mihoyo.com".parse().unwrap()));
  }

  #[test]
  fn occupied_port_and_shutdown_release_listener() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    runtime.block_on(async {
      let root = std::env::temp_dir().join(format!("cultivation-proxy-test-{}", std::process::id()));
      fs::create_dir_all(&root).unwrap();
      let cert = Certificate::from_params(CertificateParams::default()).unwrap();
      fs::write(root.join("cert.crt"), cert.serialize_pem().unwrap()).unwrap();
      fs::write(root.join("private.key"), cert.serialize_private_key_pem()).unwrap();
      let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
      let port = occupied.local_addr().unwrap().port();
      let (_sender, receiver) = tokio::sync::oneshot::channel();
      assert!(create_proxy(port, root.to_string_lossy().into(), receiver, u64::MAX).is_err());
      drop(occupied);
      let (sender, receiver) = tokio::sync::oneshot::channel();
      let task = create_proxy(port, root.to_string_lossy().into(), receiver, u64::MAX).unwrap();
      sender.send(()).unwrap();
      tokio::time::timeout(std::time::Duration::from_secs(4), task).await.unwrap().unwrap();
      let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
      drop(listener);
      // This test owns the uniquely named temporary directory.
      fs::remove_dir_all(root).unwrap();
    });
  }
}

/*
 * Attempts to install the certificate authority's certificate into the Root CA store.
 */
#[cfg(windows)]
pub fn install_ca_files(cert_path: &Path) -> Result<(), String> {
  use std::os::windows::process::CommandExt;
  let output = std::process::Command::new("certutil.exe")
    .args(["-user", "-addstore", "Root"]).arg(cert_path)
    .creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
  if output.status.success() { Ok(()) }
  else { Err(format!("Certificate installation failed: {}", String::from_utf8_lossy(&output.stderr))) }
}

