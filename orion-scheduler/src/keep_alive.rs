#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ImageSpec {
    pub source: Option<String>,
    pub digest: Option<String>,
}

#[cfg(target_os = "linux")]
mod platform {
    use std::sync::Arc;

    use anyhow::Result;
    use qlean::{ImageConfig, InteractiveShell, Machine, MachineConfig};
    use tokio::sync::Mutex;

    use super::ImageSpec;

    impl From<ImageSpec> for ImageConfig {
        fn from(spec: ImageSpec) -> Self {
            let mut cfg = ImageConfig::default()
                .with_distro(qlean::Distro::Debian)
                .with_arch(qlean::GuestArch::Amd64);
            if let Some(source) = spec.source {
                cfg = cfg.with_source(source);
            }
            if let Some(digest) = spec.digest {
                cfg = cfg.with_digest(digest);
            }
            cfg
        }
    }

    /// Wrapper around Machine that keeps it alive after initial operations.
    /// This allows multiple operations (deploy, start_orion, etc.) on the same VM.
    pub struct KeepAliveMachine {
        machine: Arc<Mutex<Option<Machine>>>,
    }

    impl KeepAliveMachine {
        /// Create a new VM and keep it alive.
        ///
        /// `image_spec` can be:
        /// - `Some(ImageSpec)` with a local path/URL + digest — uses custom image
        /// - `None` — uses qlean's built-in Debian image
        pub async fn new(
            vm_name: &str,
            image_spec: Option<ImageSpec>,
            disk_gb: Option<u32>,
            cpus: Option<u32>,
            memory_mb: Option<u32>,
        ) -> Result<Self> {
            tracing::info!("[keep-alive] Creating VM: {}", vm_name);

            let config = MachineConfig {
                core: cpus.unwrap_or(4),
                mem: memory_mb.unwrap_or(8192),
                disk: disk_gb,
                ..Default::default()
            };
            tracing::info!(
                "[keep-alive] VM config: cpus={}, memory_mb={}",
                config.core,
                config.mem
            );

            let image = if let Some(spec) = image_spec {
                let cfg: ImageConfig = spec.into();
                if cfg.source.is_some() {
                    tracing::info!("[keep-alive] Using custom image (source set)");
                } else {
                    tracing::info!("[keep-alive] Using default Debian image (no custom config)");
                }
                qlean::Image::new(cfg).await?
            } else {
                tracing::info!("[keep-alive] Using default Debian image");
                qlean::Image::new(
                    ImageConfig::default()
                        .with_distro(qlean::Distro::Debian)
                        .with_arch(qlean::GuestArch::Amd64),
                )
                .await?
            };

            let mut machine = Machine::new(&image, &config).await?;
            machine.init().await?;

            tracing::info!("[keep-alive] VM {} initialized and running", vm_name);

            Ok(Self {
                machine: Arc::new(Mutex::new(Some(machine))),
            })
        }

        /// Execute a command in the VM.
        pub async fn exec(&self, cmd: &str) -> Result<std::process::Output> {
            let mut guard = self.machine.lock().await;
            if let Some(machine) = guard.as_mut() {
                tracing::info!("[keep-alive] Executing: {}", cmd);
                let output = machine.exec(cmd).await?;
                Ok(output)
            } else {
                anyhow::bail!("VM has been shut down")
            }
        }

        /// Upload a file to the VM.
        pub async fn upload(
            &self,
            local: impl AsRef<std::path::Path>,
            remote: impl AsRef<std::path::Path>,
        ) -> Result<()> {
            let mut guard = self.machine.lock().await;
            if let Some(machine) = guard.as_mut() {
                let local_path = local.as_ref();
                let remote_path_str = remote.as_ref().to_string_lossy().into_owned();
                tracing::info!(
                    "[keep-alive] Uploading: {} -> {}",
                    local_path.display(),
                    remote_path_str
                );
                machine.upload(local, remote).await?;
                Ok(())
            } else {
                anyhow::bail!("VM has been shut down")
            }
        }

        /// Shutdown the VM.
        pub async fn shutdown(self) -> Result<()> {
            tracing::info!("[keep-alive] Shutting down VM");
            let mut guard = self.machine.lock().await;
            if let Some(mut machine) = guard.take() {
                machine.shutdown().await?;
                tracing::info!("[keep-alive] VM shutdown complete");
            }
            Ok(())
        }

        /// Get the VM's IP address by running hostname -I inside the VM.
        pub async fn get_ip(&self) -> Result<Option<String>> {
            let output = self.exec("hostname -I | awk '{print $1}'").await?;
            let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if ip.is_empty() {
                Ok(None)
            } else {
                Ok(Some(ip))
            }
        }

        /// Open a dedicated interactive PTY shell (does not block management SSH).
        ///
        /// Temporarily takes the `Machine` out of the mutex so log `exec` is not
        /// blocked for the whole vsock SSH handshake (can take many seconds).
        pub async fn open_interactive_shell(
            &self,
            cols: u32,
            rows: u32,
        ) -> Result<InteractiveShell> {
            let machine = {
                let mut guard = self.machine.lock().await;
                guard
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("VM has been shut down"))?
            };
            tracing::info!("[keep-alive] Opening interactive shell {}x{}", cols, rows);
            let result = machine.open_interactive_shell(cols, rows).await;
            {
                let mut guard = self.machine.lock().await;
                *guard = Some(machine);
            }
            result
        }
    }

    impl Clone for KeepAliveMachine {
        fn clone(&self) -> Self {
            Self {
                machine: Arc::clone(&self.machine),
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use anyhow::Result;

    use super::ImageSpec;

    /// Stub for InteractiveShell on non-Linux (never successfully constructed).
    pub struct InteractiveShell;

    pub struct InteractiveShellReader;
    pub struct InteractiveShellWriter;

    impl InteractiveShell {
        pub fn split(self) -> (InteractiveShellReader, InteractiveShellWriter) {
            (InteractiveShellReader, InteractiveShellWriter)
        }
    }

    impl InteractiveShellReader {
        pub async fn read_chunk(&mut self) -> Result<Option<Vec<u8>>> {
            anyhow::bail!("VM operations require Linux + KVM")
        }
    }

    impl InteractiveShellWriter {
        pub async fn write(&self, _data: &[u8]) -> Result<()> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn resize(&self, _cols: u32, _rows: u32) -> Result<()> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn close(self, _reader: InteractiveShellReader) -> Result<()> {
            anyhow::bail!("VM operations require Linux + KVM")
        }
    }

    /// Stub implementation for non-Linux platforms (macOS/Windows).
    pub struct KeepAliveMachine;

    impl KeepAliveMachine {
        pub async fn new(
            _vm_name: &str,
            _image_spec: Option<ImageSpec>,
            _disk_gb: Option<u32>,
            _cpus: Option<u32>,
            _memory_mb: Option<u32>,
        ) -> Result<Self> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn exec(&self, _cmd: &str) -> Result<std::process::Output> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn upload(
            &self,
            _local: impl AsRef<std::path::Path>,
            _remote: impl AsRef<std::path::Path>,
        ) -> Result<()> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn shutdown(self) -> Result<()> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn get_ip(&self) -> Result<Option<String>> {
            anyhow::bail!("VM operations require Linux + KVM")
        }

        pub async fn open_interactive_shell(
            &self,
            _cols: u32,
            _rows: u32,
        ) -> Result<InteractiveShell> {
            anyhow::bail!("VM operations require Linux + KVM")
        }
    }

    impl Clone for KeepAliveMachine {
        fn clone(&self) -> Self {
            Self
        }
    }
}

pub use platform::KeepAliveMachine;
