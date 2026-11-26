//! Start one child, share its readiness and reap it on close or cancellation.

use roomkit_ai::ProviderError;
use std::{
    ffi::OsString,
    net::{Ipv4Addr, TcpListener},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::Child,
    sync::Mutex,
    time::{sleep, timeout},
};

use crate::{LlamaCppConfig, binary, diagnostics::StartupLog, error};

pub(crate) struct Server {
    config: LlamaCppConfig,
    pub(crate) port: u16,
    child: Mutex<Option<(Child, StartupLog)>>,
    bundle: Option<std::path::PathBuf>,
}

impl Server {
    pub(crate) fn new(
        config: LlamaCppConfig,
        port: u16,
        bundle: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            config,
            bundle,
            port,
            child: Mutex::new(None),
        }
    }

    pub(crate) async fn start(&self) -> Result<(), ProviderError> {
        let mut slot = self.child.lock().await;
        if let Some((child, _log)) = slot.as_mut() {
            if child.try_wait().map_err(binary::io_error)?.is_none() {
                return Ok(());
            }
            *slot = None;
        }
        let args = self.arguments()?;
        let executable = binary::resolve(&self.config).await?;
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, self.port))
            .map_err(|_| error(format!("loopback port {} is unavailable", self.port)))?;
        let mut cmd = executable.command(self.bundle.as_deref())?;
        cmd.args(args)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        drop(reservation);
        let mut child = cmd
            .spawn()
            .map_err(|e| error(format!("cannot start llama-server: {e}")))?;
        // The child stays local until ready. Dropping a cancelled start kills it.
        let mut log = StartupLog::watch(&mut child);
        let result = timeout(self.config.startup_timeout, self.wait_ready(&mut child))
            .await
            .unwrap_or_else(|_| {
                Err(error(
                    "llama-server did not become ready before startup_timeout",
                ))
            });
        if let Err(failure) = result {
            stop_child(&mut child).await;
            log.drain().await;
            return Err(log.explain(failure));
        }
        *slot = Some((child, log));
        Ok(())
    }

    fn arguments(&self) -> Result<Vec<OsString>, ProviderError> {
        let cfg = &self.config;
        let (switch, model) = if Path::new(&cfg.model)
            .extension()
            .is_some_and(|ext| ext == "gguf")
        {
            let path = binary::expand(Path::new(&cfg.model));
            if !path.is_file() {
                return Err(error("local GGUF model file does not exist"));
            }
            ("-m", path.into_os_string())
        } else {
            ("-hf", OsString::from(&cfg.model))
        };
        let mut args: Vec<OsString> = vec![
            switch.into(),
            model,
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string().into(),
            "--jinja".into(),
            "-c".into(),
            cfg.context_size.to_string().into(),
        ];
        if let Some(layers) = cfg.gpu_layers {
            args.extend(["-ngl".into(), layers.to_string().into()]);
        }
        args.extend(cfg.extra_args.iter().map(OsString::from));
        Ok(args)
    }

    async fn wait_ready(&self, child: &mut Child) -> Result<(), ProviderError> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .tls_backend_preconfigured(
                roomkit_providers_http::tls_config()
                    .map_err(|_| error("cannot configure health client"))?,
            )
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| error("cannot create health client"))?;
        let url = format!("http://127.0.0.1:{}/health", self.port);
        loop {
            if let Some(status) = child.try_wait().map_err(binary::io_error)? {
                return Err(error(format!(
                    "llama-server exited during startup ({status}); verify the model and binary are compatible"
                )));
            }
            if client
                .get(&url)
                .send()
                .await
                .is_ok_and(|r| r.status() == reqwest::StatusCode::OK)
            {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    pub(crate) async fn close(&self) {
        let mut slot = self.child.lock().await;
        if let Some((mut child, _log)) = slot.take() {
            stop_child(&mut child).await;
        }
    }
}

/// Reap the process even when it ignores the graceful termination signal.
async fn stop_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id().and_then(|id| i32::try_from(id).ok()) {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        if kill(Pid::from_raw(pid), Signal::SIGTERM).is_ok()
            && matches!(
                timeout(Duration::from_secs(10), child.wait()).await,
                Ok(Ok(_))
            )
        {
            return;
        }
    }
    drop(child.kill().await);
}
