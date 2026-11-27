//! Environment shared by the server and the hardware probe it runs before startup.
use std::{env, ffi::OsStr};
use tokio::process::Command;

pub(crate) fn child_command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear();
    // Exact names keep host credentials out of children. Library paths may
    // subsequently be adjusted by the platform loader for the chosen build.
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "LOCALAPPDATA",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
        "HF_HOME",
        "HF_HUB_CACHE",
        "HUGGINGFACE_HUB_CACHE",
        "HF_HUB_OFFLINE",
        "LLAMA_CACHE",
        "CUDA_VISIBLE_DEVICES",
        "CUDA_DEVICE_ORDER",
        "CUDA_CACHE_PATH",
        "HIP_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
        "GGML_METAL_PATH_RESOURCES",
        "VK_ICD_FILENAMES",
        "VK_DRIVER_FILES",
        "OMP_NUM_THREADS",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "CURL_CA_BUNDLE",
    ] {
        if let Some(value) = env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_child_keeps_its_path_and_leaves_host_variables_behind() -> std::io::Result<()> {
        assert!(env::var_os("CARGO_MANIFEST_DIR").is_some());
        let status = child_command("sh")
            .args([
                "-c",
                r#"test -n "$PATH" && test -z "${CARGO_MANIFEST_DIR+x}${ROOMKIT_EXECUTOR_REDIS_URL+x}${ROOMKIT_SESSION_GROUP_KEY+x}""#,
            ])
            .status()
            .await?;
        assert!(status.success());
        Ok(())
    }
}
