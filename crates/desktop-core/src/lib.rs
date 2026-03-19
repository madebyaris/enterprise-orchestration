use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use control_server::{app, AppState};
use observability::EventBus;
use persistence::OrchestratorStore;
use security::SecretManager;
use serde::Serialize;
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    task::JoinHandle,
};

#[derive(Debug, Clone)]
pub struct DesktopConfig {
    pub control_bind: String,
    pub control_port: u16,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub secret_service_name: String,
}

impl DesktopConfig {
    pub fn from_env() -> Self {
        let data_dir = std::env::var("ORCH_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".orchestrator"));
        let db_path = std::env::var("ORCH_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("orchestrator.db"));

        Self {
            control_bind: std::env::var("ORCH_CONTROL_BIND").unwrap_or_else(|_| "127.0.0.1".into()),
            control_port: std::env::var("ORCH_CONTROL_PORT")
                .ok()
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(42420),
            data_dir,
            db_path,
            secret_service_name: "enterprise-orchestration".into(),
        }
    }
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopStatus {
    pub control_server_running: bool,
    pub control_url: Option<String>,
    pub data_dir: String,
    pub db_path: String,
    pub secret_backend: String,
}

struct ControlServerHandle {
    url: String,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
pub struct DesktopRuntime {
    config: DesktopConfig,
    store: OrchestratorStore,
    events: EventBus,
    secrets: SecretManager,
    server: Arc<Mutex<Option<ControlServerHandle>>>,
}

impl DesktopRuntime {
    pub fn new(config: DesktopConfig, secrets: SecretManager) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir).with_context(|| {
            format!(
                "failed to create desktop data directory {}",
                config.data_dir.display()
            )
        })?;

        let store = OrchestratorStore::open(&config.db_path)?;
        Ok(Self {
            config,
            store,
            events: EventBus::default(),
            secrets,
            server: Arc::new(Mutex::new(None)),
        })
    }

    pub fn with_memory_secrets(config: DesktopConfig) -> Result<Self> {
        Self::new(config, SecretManager::new_memory())
    }

    pub fn with_keyring_secrets(config: DesktopConfig) -> Result<Self> {
        Self::new(
            config.clone(),
            SecretManager::new_keyring(config.secret_service_name.clone()),
        )
    }

    pub async fn start_control_server(&self) -> Result<String> {
        let mut guard = self.server.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.url.clone());
        }

        let address = format!("{}:{}", self.config.control_bind, self.config.control_port);
        let listener = TcpListener::bind(&address)
            .await
            .with_context(|| format!("failed to bind control server at {address}"))?;
        let local_addr = listener.local_addr()?;
        let url = format!("http://{}", local_addr);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let router = app(AppState {
            store: self.store.clone(),
            events: self.events.clone(),
        });

        let task = tokio::spawn(async move {
            let server = axum::serve(listener, router.into_make_service()).with_graceful_shutdown(
                async move {
                    let _ = shutdown_rx.await;
                },
            );

            if let Err(error) = server.await {
                tracing::error!("control server terminated with error: {error}");
            }
        });

        *guard = Some(ControlServerHandle {
            url: url.clone(),
            shutdown: Some(shutdown_tx),
            task,
        });

        Ok(url)
    }

    pub async fn stop_control_server(&self) -> Result<()> {
        let mut guard = self.server.lock().await;
        if let Some(mut handle) = guard.take() {
            if let Some(shutdown) = handle.shutdown.take() {
                let _ = shutdown.send(());
            }
            let _ = handle.task.await;
        }

        Ok(())
    }

    pub async fn status(&self) -> DesktopStatus {
        let guard = self.server.lock().await;
        DesktopStatus {
            control_server_running: guard.is_some(),
            control_url: guard.as_ref().map(|handle| handle.url.clone()),
            data_dir: self.config.data_dir.to_string_lossy().into_owned(),
            db_path: self.config.db_path.to_string_lossy().into_owned(),
            secret_backend: self.secrets.backend_name().to_string(),
        }
    }

    pub fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.secrets.set_secret(key, value)
    }

    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        self.secrets.get_secret(key)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
    };

    use super::{DesktopConfig, DesktopRuntime};

    fn temp_config() -> DesktopConfig {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let data_dir = env::temp_dir().join(format!("desktop-core-test-{unique}"));

        DesktopConfig {
            control_bind: "127.0.0.1".into(),
            control_port: 0,
            db_path: data_dir.join("orchestrator.db"),
            data_dir,
            secret_service_name: "enterprise-orchestration-tests".into(),
        }
    }

    #[tokio::test]
    async fn starts_and_stops_control_server() {
        let runtime = DesktopRuntime::with_memory_secrets(temp_config()).expect("runtime");
        let url = runtime.start_control_server().await.expect("start server");
        let status = runtime.status().await;
        assert!(status.control_server_running);
        assert_eq!(status.control_url.as_deref(), Some(url.as_str()));

        let host = url.trim_start_matches("http://");
        let mut stream = TcpStream::connect(host).await.expect("connect");
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("request");

        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .expect("response timeout")
            .expect("read response");
        let response_text = String::from_utf8(response).expect("utf8");

        assert!(response_text.contains("\"status\":\"ok\""));

        runtime.stop_control_server().await.expect("stop server");
        let stopped = runtime.status().await;
        assert!(!stopped.control_server_running);
    }

    #[test]
    fn uses_secret_manager_without_plaintext_files() {
        let runtime = DesktopRuntime::with_memory_secrets(temp_config()).expect("runtime");

        runtime
            .set_secret("openai_api_key", "secret-value")
            .expect("set secret");
        assert_eq!(
            runtime
                .get_secret("openai_api_key")
                .expect("get secret")
                .as_deref(),
            Some("secret-value")
        );
    }
}
