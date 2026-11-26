//! Drain both child pipes without putting prompts, arguments or credentials into provider errors.

use roomkit_ai::ProviderError;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
    task::JoinSet,
    time::timeout,
};

pub(crate) struct StartupLog {
    reasons: Arc<Mutex<Vec<&'static str>>>,
    readers: JoinSet<()>,
}

impl StartupLog {
    pub(crate) fn watch(child: &mut Child) -> Self {
        let mut log = Self {
            reasons: Arc::default(),
            readers: JoinSet::new(),
        };
        if let Some(reader) = child.stdout.take() {
            log.read(reader);
        }
        if let Some(reader) = child.stderr.take() {
            log.read(reader);
        }
        log
    }

    fn read(&mut self, mut reader: impl AsyncRead + Unpin + Send + 'static) {
        let reasons = Arc::clone(&self.reasons);
        self.readers.spawn(async move {
            let mut buf = [0_u8; 2048];
            let mut line = Vec::new();
            while let Ok(n) = reader.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                for &byte in buf.iter().take(n) {
                    if byte == b'\n' {
                        remember(&line, &reasons);
                        line.clear();
                    } else if line.len() < 2048 {
                        line.push(byte);
                    }
                }
            }
            remember(&line, &reasons);
        });
    }

    pub(crate) async fn drain(&mut self) {
        // A custom server may have passed a pipe to a longer-lived process.
        let _drained = timeout(Duration::from_millis(250), async {
            while self.readers.join_next().await.is_some() {}
        })
        .await;
    }

    pub(crate) fn explain(&self, mut failure: ProviderError) -> ProviderError {
        if let Ok(reasons) = self.reasons.lock()
            && !reasons.is_empty()
        {
            failure.message.push_str(": ");
            failure.message.push_str(&reasons.join("; "));
        }
        failure
    }
}

fn remember(line: &[u8], reasons: &Mutex<Vec<&'static str>>) {
    let lower = String::from_utf8_lossy(line).to_ascii_lowercase();
    let known: &[(&str, &str)] = &[
        (
            "out of memory",
            "not enough memory for the model or its context",
        ),
        ("cudamalloc failed", "GPU memory allocation failed"),
        (
            "unknown model architecture",
            "this server does not recognize the model architecture",
        ),
        (
            "unsupported model architecture",
            "this server does not support the model architecture",
        ),
        (
            "loading shared libraries",
            "a required shared library could not be loaded",
        ),
        (
            "library not loaded",
            "a required shared library could not be loaded",
        ),
        ("failed to load model", "failed to load the model"),
        ("cannot load model", "failed to load the model"),
        ("failed to download", "model download failed"),
        ("401 unauthorized", "model download requires authentication"),
        ("403 forbidden", "access to the model download was denied"),
        ("404 not found", "model download was not found"),
        (
            "invalid argument",
            "llama-server rejected a command-line argument",
        ),
        (
            "address already in use",
            "the requested port is already in use",
        ),
    ];
    if let Some((_, reason)) = known.iter().find(|(needle, _)| lower.contains(needle))
        && let Ok(mut found) = reasons.lock()
        && !found.contains(reason)
    {
        found.push(reason);
    }
}
