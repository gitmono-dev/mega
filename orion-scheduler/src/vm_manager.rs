use std::path::PathBuf;

use anyhow::Result;
use tracing::info;

use crate::{
    config::{TargetConfig, expand_tilde},
    keep_alive::KeepAliveMachine,
};

/// The target directory inside the VM guest OS where Orion is deployed
const ORION_TARGET_DIR: &str = "/home/orion/orion-runner";

/// Swap file path and size bounds for Orion build VMs (16 GB RAM by default).
const VM_SWAP_FILE: &str = "/swapfile";
const VM_SWAP_TARGET_SIZE_GB: u32 = 8;
const VM_SWAP_MIN_SIZE_GB: u32 = 1;
const VM_SWAP_FREE_SPACE_RESERVE_GB: u32 = 1;

/// Create and enable a swap file if not already active.
async fn setup_vm_swap(machine: &KeepAliveMachine) -> Result<()> {
    info!(
        "[orion-deploy] Configuring up to {} GB swap at {}",
        VM_SWAP_TARGET_SIZE_GB, VM_SWAP_FILE
    );
    let cmd = format!(
        r#"set -e
swap_active=0
if swapon --show --noheadings 2>/dev/null | grep -q '{swap_file}'; then
  echo 'swap already active'
  swap_active=1
elif [ -f '{swap_file}' ]; then
  swapon '{swap_file}'
  swap_active=1
else
  target_kb=$(( {target_gb} * 1024 * 1024 ))
  min_kb=$(( {min_gb} * 1024 * 1024 ))
  reserve_kb=$(( {reserve_gb} * 1024 * 1024 ))
  avail_kb=$(df -Pk "$(dirname '{swap_file}')" | awk 'NR == 2 {{ print $4 }}')
  if [ -z "$avail_kb" ]; then
    echo 'unable to determine available disk space for swap' >&2
    exit 1
  fi
  usable_kb=$(( avail_kb - reserve_kb ))
  if [ "$usable_kb" -lt "$min_kb" ]; then
    echo "skipping swap: only ${{avail_kb}} KiB available, reserving ${{reserve_kb}} KiB"
  else
    if [ "$usable_kb" -gt "$target_kb" ]; then
      swap_kb="$target_kb"
    else
      swap_kb="$usable_kb"
    fi
    echo "creating ${{swap_kb}} KiB swap file"
    fallocate -l "${{swap_kb}}K" '{swap_file}' 2>/dev/null \
      || dd if=/dev/zero of='{swap_file}' bs=1024 count="$swap_kb" status=none
    chmod 600 '{swap_file}'
    mkswap '{swap_file}'
    swapon '{swap_file}'
    swap_active=1
  fi
fi
if [ "$swap_active" -eq 1 ]; then
  grep -qF '{swap_file}' /etc/fstab 2>/dev/null \
    || echo '{swap_file} none swap sw 0 0' >> /etc/fstab
fi
swapon --show
free -h | head -2
"#,
        swap_file = VM_SWAP_FILE,
        target_gb = VM_SWAP_TARGET_SIZE_GB,
        min_gb = VM_SWAP_MIN_SIZE_GB,
        reserve_gb = VM_SWAP_FREE_SPACE_RESERVE_GB,
    );
    let output = machine.exec(&cmd).await?;
    info!(
        "[orion-deploy] Swap setup output:\n{}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    if !output.status.success() {
        anyhow::bail!(
            "swap setup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Upload a single file to the VM via SFTP
async fn upload_file(
    machine: &KeepAliveMachine,
    local: impl AsRef<std::path::Path>,
    remote: impl AsRef<std::path::Path>,
) -> Result<()> {
    let local_path = local.as_ref();
    let remote_path_str = remote.as_ref().to_string_lossy().into_owned();
    info!(
        "[orion-deploy] Uploading file: {} -> {}",
        local_path.display(),
        remote_path_str
    );
    machine.upload(local, remote).await?;
    Ok(())
}

/// Deploy Orion files to the VM.
/// Note: ORION_TARGET_DIR is a fixed path inside the VM guest OS, not a host path.
pub async fn deploy_orion_in_vm(
    machine: &KeepAliveMachine,
    orion_source_dir: &str,
    orion_binary_path: &str,
) -> Result<()> {
    info!("[orion-deploy] Starting Orion deployment");

    // Step 1: Create target directory
    info!("[orion-deploy] Creating directory: {}", ORION_TARGET_DIR);
    machine
        .exec(&format!("mkdir -p {}", ORION_TARGET_DIR))
        .await?;

    // Step 2: Deploy runner-config files
    let runner_config = PathBuf::from(orion_source_dir).join("runner-config");
    let files = ["run.sh", "scorpio.toml", "preflight.sh", "cleanup.sh"];

    for file in &files {
        let local_path = runner_config.join(file);
        if local_path.exists() {
            info!("[orion-deploy] Uploading config file: {}", file);
            upload_file(
                machine,
                &local_path,
                format!("{}/{}", ORION_TARGET_DIR, file),
            )
            .await?;
        } else {
            info!(
                "[orion-deploy] Skipping missing file: {}",
                local_path.display()
            );
        }
    }

    // Step 3: Deploy .env file
    let env_source = runner_config.join(".env.prod");
    if env_source.exists() {
        info!("[orion-deploy] Uploading .env file");
        upload_file(machine, &env_source, format!("{}/.env", ORION_TARGET_DIR)).await?;
    } else {
        info!("[orion-deploy] Skipping missing .env file");
    }

    // Step 4: Deploy systemd service file
    let service_source = PathBuf::from(orion_source_dir)
        .join("systemd")
        .join("orion-runner.service");
    if service_source.exists() {
        info!("[orion-deploy] Uploading systemd service file");
        upload_file(
            machine,
            &service_source,
            "/etc/systemd/system/orion-runner.service",
        )
        .await?;
    } else {
        info!("[orion-deploy] Skipping missing service file");
    }

    // Step 5: Upload orion binary (large file, ~500MB)
    let orion_binary = PathBuf::from(orion_binary_path);
    if orion_binary.exists() {
        let size = std::fs::metadata(&orion_binary)
            .map(|m| m.len())
            .unwrap_or(0)
            / 1024
            / 1024;
        info!("[orion-deploy] Uploading Orion binary ({} MB)...", size);
        upload_file(
            machine,
            &orion_binary,
            format!("{}/orion", ORION_TARGET_DIR),
        )
        .await?;
        info!("[orion-deploy] Orion binary uploaded successfully");
    } else {
        return Err(anyhow::anyhow!(
            "Orion binary not found at {:?}",
            orion_binary
        ));
    }

    // Step 6: Set permissions
    info!("[orion-deploy] Setting executable permissions");
    machine
        .exec(&format!("chmod +x {}/run.sh", ORION_TARGET_DIR))
        .await?;
    machine
        .exec(&format!("chmod +x {}/preflight.sh", ORION_TARGET_DIR))
        .await?;
    machine
        .exec(&format!("chmod +x {}/cleanup.sh", ORION_TARGET_DIR))
        .await?;
    machine
        .exec(&format!("chmod +x {}/orion", ORION_TARGET_DIR))
        .await?;
    info!("[orion-deploy] Setting capabilities on orion binary");
    machine
        .exec("setcap cap_dac_read_search+ep /home/orion/orion-runner/orion")
        .await?;
    info!("[orion-deploy] Reloading systemd daemon");
    machine.exec("systemctl daemon-reload").await?;

    info!("[orion-deploy] Orion deployment completed");
    Ok(())
}

/// Start Orion service in the VM
/// Returns the Orion service logs on success
pub async fn start_orion_in_vm(machine: &KeepAliveMachine) -> Result<String> {
    info!("[orion-deploy] Starting Orion service");

    setup_vm_swap(machine).await?;

    // Step 1: Create Scorpio directories
    info!("[orion-deploy] Creating Scorpio directories");
    machine.exec("mkdir -p /data/scorpio/store").await?;
    machine
        .exec("mkdir -p /data/scorpio/antares/{upper,cl,mnt}")
        .await?;
    machine.exec("mkdir -p /workspace/mount").await?;
    info!("[orion-deploy] Setting ownership on data directories");
    machine.exec("chown -R orion:orion /data/scorpio").await?;
    machine
        .exec("chown -R orion:orion /workspace/mount")
        .await?;

    // Step 2: Start service
    info!("[orion-deploy] Starting orion-runner service via systemctl");
    let start_result = machine.exec("systemctl start orion-runner").await?;
    info!(
        "[orion-deploy] Orion service start command executed, exit code: {}",
        start_result.status
    );

    // Step 3: Verify service started
    info!("[orion-deploy] Verifying Orion service status");
    let status_result = machine.exec("systemctl is-active orion-runner").await?;
    let status = String::from_utf8_lossy(&status_result.stdout)
        .trim()
        .to_string();
    info!("[orion-deploy] Orion service status: {}", status);

    let mut logs = String::new();

    if status == "active" {
        info!("[orion-deploy] Orion service started successfully");
        let logs_result = machine
            .exec("journalctl -u orion-runner --no-pager -n 50 2>&1 || echo 'journalctl failed'")
            .await;
        if let Ok(logs_output) = logs_result {
            logs = String::from_utf8_lossy(&logs_output.stdout).into_owned();
        }
    } else {
        info!("[orion-deploy] Orion service may have issues, fetching logs");
        let logs_result = machine
            .exec("journalctl -u orion-runner --no-pager -n 50 2>&1 || echo 'journalctl failed'")
            .await;
        if let Ok(logs_output) = logs_result {
            logs = String::from_utf8_lossy(&logs_output.stdout).into_owned();
            info!("[orion-deploy] Orion service logs:\n{}", logs);
        }
    }

    // Step 4: Get Orion process info
    info!("[orion-deploy] Checking Orion process");
    let process_result = machine
        .exec("pgrep -a orion || echo 'Orion process not found'")
        .await?;
    let process_info = String::from_utf8_lossy(&process_result.stdout)
        .trim()
        .to_string();
    info!("[orion-deploy] Orion process info: {}", process_info);

    if !logs.is_empty() {
        logs.push_str("\n\n[Orion Process Info]\n");
        logs.push_str(&process_info);
    }

    info!("[orion-deploy] Orion startup sequence completed");
    Ok(logs)
}

/// Inject additional SSH public keys into the VM for debugging access
pub async fn inject_ssh_keys(machine: &KeepAliveMachine, ssh_public_key_path: &str) -> Result<()> {
    info!("[ssh] Injecting SSH keys for debugging access");

    // Read the extra public key from a file
    let extra_key_path = expand_tilde(ssh_public_key_path);
    let extra_key = if extra_key_path.exists() {
        tokio::fs::read_to_string(extra_key_path)
            .await?
            .trim()
            .to_string()
    } else {
        info!(
            "[ssh] No extra SSH key found at {:?}, skipping",
            extra_key_path
        );
        return Ok(());
    };

    // Ensure /root/.ssh directory exists
    machine
        .exec("mkdir -p /root/.ssh && chmod 700 /root/.ssh")
        .await?;

    // Append the extra key to authorized_keys (avoiding duplicates)
    let add_key_cmd = format!(
        r#"grep -qF '{}' /root/.ssh/authorized_keys || echo '{}' >> /root/.ssh/authorized_keys"#,
        extra_key, extra_key
    );
    machine.exec(&add_key_cmd).await?;

    // Set correct permissions
    machine.exec("chmod 600 /root/.ssh/authorized_keys").await?;

    info!("[ssh] SSH key injection completed");
    Ok(())
}

/// Replace environment variables based on target configuration
pub async fn replace_env_vars_in_vm(
    machine: &KeepAliveMachine,
    target_config: &TargetConfig,
    target_name: &str,
) -> Result<()> {
    let server_ws = &target_config.server_ws;
    let scorpio_base_url = &target_config.scorpio_base_url;
    let scorpio_lfs_url = &target_config.scorpio_lfs_url;

    info!(
        "[env] Replacing environment variables for target: {}",
        target_name
    );
    info!("[env] SERVER_WS -> {}", server_ws);
    info!("[env] scorpio.toml base_url -> {}", scorpio_base_url);
    info!("[env] scorpio.toml lfs_url -> {}", scorpio_lfs_url);

    // Replace .env SERVER_WS
    let sed_cmd = format!(
        r#"sed -i 's|^SERVER_WS=.*|SERVER_WS="{}"|' /home/orion/orion-runner/.env"#,
        server_ws
    );
    info!("[env] Executing: {}", sed_cmd);
    machine.exec(&sed_cmd).await?;

    // Replace scorpio.toml base_url
    let sed_cmd = format!(
        r#"sed -i 's|base_url = ".*"|base_url = "{}"|' /home/orion/orion-runner/scorpio.toml"#,
        scorpio_base_url
    );
    info!("[env] Executing: {}", sed_cmd);
    machine.exec(&sed_cmd).await?;

    // Replace scorpio.toml lfs_url
    let sed_cmd = format!(
        r#"sed -i 's|lfs_url = ".*"|lfs_url = "{}"|' /home/orion/orion-runner/scorpio.toml"#,
        scorpio_lfs_url
    );
    info!("[env] Executing: {}", sed_cmd);
    machine.exec(&sed_cmd).await?;

    if let Some(retain) = target_config.retain_antares_mounts {
        let value = if retain { "1" } else { "0" };
        info!("[env] ORION_RETAIN_ANTARES_MOUNTS -> {}", value);
        // Upsert: replace existing line or append if missing.
        let upsert = format!(
            r#"grep -q '^ORION_RETAIN_ANTARES_MOUNTS=' /home/orion/orion-runner/.env \
&& sed -i 's|^ORION_RETAIN_ANTARES_MOUNTS=.*|ORION_RETAIN_ANTARES_MOUNTS="{value}"|' /home/orion/orion-runner/.env \
|| echo 'ORION_RETAIN_ANTARES_MOUNTS="{value}"' >> /home/orion/orion-runner/.env"#
        );
        info!("[env] Executing upsert for ORION_RETAIN_ANTARES_MOUNTS");
        machine.exec(&upsert).await?;
    }

    // Verify replacements
    let verify_env = machine
        .exec("grep SERVER_WS /home/orion/orion-runner/.env")
        .await?;
    info!(
        "[env] .env SERVER_WS: {}",
        String::from_utf8_lossy(&verify_env.stdout).trim()
    );

    let verify_base_url = machine
        .exec("grep base_url /home/orion/orion-runner/scorpio.toml")
        .await?;
    info!(
        "[env] scorpio.toml base_url: {}",
        String::from_utf8_lossy(&verify_base_url.stdout).trim()
    );

    if target_config.retain_antares_mounts.is_some() {
        let verify_retain = machine
            .exec("grep ORION_RETAIN_ANTARES_MOUNTS /home/orion/orion-runner/.env || true")
            .await?;
        info!(
            "[env] .env ORION_RETAIN_ANTARES_MOUNTS: {}",
            String::from_utf8_lossy(&verify_retain.stdout).trim()
        );
    }

    info!(
        "[env] Environment variable replacement completed for target: {}",
        target_name
    );
    Ok(())
}
