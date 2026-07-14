use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::{Mutex, MutexGuard, RwLock};

use crate::{config::SharedConfig, keep_alive::KeepAliveMachine};

/// Lifecycle phase of a VM managed by the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmPhase {
    Provisioning,
    Running,
    Failed,
}

impl VmPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            VmPhase::Provisioning => "provisioning",
            VmPhase::Running => "running",
            VmPhase::Failed => "failed",
        }
    }
}

/// Represents the current state of one VM (keyed by domain in AppState).
#[derive(Debug, Clone)]
pub struct VmInfo {
    pub id: String,
    /// Host parsed from `server_ws` (uniqueness key).
    pub domain: String,
    /// Optional label from webhook `target` field.
    pub target: String,
    pub phase: VmPhase,
    pub ip: Option<String>,
    pub created_at: std::time::Instant,
    /// Path to the Orion log file
    pub log_file: Option<String>,
    /// Error message when phase is Failed
    pub error: Option<String>,
}

pub struct VmEntry {
    pub info: VmInfo,
    pub machine: Option<KeepAliveMachine>,
}

/// Global state for tracking multiple VMs (one per domain).
pub struct AppState {
    /// key = domain (`server_ws` host)
    pub vms: Arc<RwLock<HashMap<String, VmEntry>>>,
    pub config: SharedConfig,
    /// Single-flight mutex guarding the full VM update sequence
    /// (shutdown existing slot for a domain → create new VM → publish to state).
    /// Without this, two concurrent /webhook calls for the same domain can both
    /// pass the conflict check before either stores its new machine, leaking
    /// the earlier qemu process out of `state` and out of `/shutdown`'s reach.
    /// Coarse (global) for MVP; can later become a per-domain Mutex.
    update_lock: Arc<Mutex<()>>,
}

impl AppState {
    /// Create a new AppState with an empty VM map.
    pub fn new(config: SharedConfig) -> Self {
        Self {
            vms: Arc::new(RwLock::new(HashMap::new())),
            config,
            update_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Acquire the single-flight update lock. Callers MUST hold the returned
    /// guard for the entire shutdown/create/set sequence so concurrent
    /// /webhook requests serialize and never produce orphan VMs.
    ///
    /// `/shutdown` and signal-triggered teardown must also hold this guard
    /// to avoid running between an in-flight create's
    /// `KeepAliveMachine::new` and `set_vm`, which would otherwise miss the
    /// freshly-spawned qemu and leave it untracked.
    pub async fn lock_update(&self) -> MutexGuard<'_, ()> {
        self.update_lock.lock().await
    }

    /// Like `lock_update`, but bounded so signal handlers don't hang the
    /// process behind a multi-minute create. Returns `None` if the lock
    /// could not be acquired within `timeout`; callers must then fall back
    /// to the run-dir qemu reap safety net.
    pub async fn try_lock_update(&self, timeout: Duration) -> Option<MutexGuard<'_, ()>> {
        tokio::time::timeout(timeout, self.update_lock.lock())
            .await
            .ok()
    }

    pub async fn list_vms(&self) -> Vec<VmInfo> {
        let vms = self.vms.read().await;
        vms.values().map(|e| e.info.clone()).collect()
    }

    pub async fn vm_count(&self) -> usize {
        self.vms.read().await.len()
    }

    pub async fn get_vm_by_domain(&self, domain: &str) -> Option<VmInfo> {
        let vms = self.vms.read().await;
        vms.get(domain).map(|e| e.info.clone())
    }

    pub async fn get_vm_by_id(&self, id: &str) -> Option<VmInfo> {
        let vms = self.vms.read().await;
        vms.values()
            .find(|e| e.info.id == id)
            .map(|e| e.info.clone())
    }

    pub async fn get_machine_by_domain(&self, domain: &str) -> Option<KeepAliveMachine> {
        let vms = self.vms.read().await;
        vms.get(domain).and_then(|e| e.machine.clone())
    }

    pub async fn get_machine_by_id(&self, id: &str) -> Option<KeepAliveMachine> {
        let vms = self.vms.read().await;
        vms.values()
            .find(|e| e.info.id == id)
            .and_then(|e| e.machine.clone())
    }

    /// Domain for a vm_id, if tracked.
    pub async fn domain_for_vm_id(&self, id: &str) -> Option<String> {
        let vms = self.vms.read().await;
        vms.values()
            .find(|e| e.info.id == id)
            .map(|e| e.info.domain.clone())
    }

    /// Register a VM in provisioning state (no machine handle yet).
    pub async fn set_vm_provisioning(&self, info: VmInfo) {
        let domain = info.domain.clone();
        let mut vms = self.vms.write().await;
        vms.insert(
            domain,
            VmEntry {
                info,
                machine: None,
            },
        );
    }

    /// Set VM info and machine reference together for one domain.
    /// A single map write makes readers never observe a half-published entry
    /// (e.g. Running with `machine = None`), which previously allowed a
    /// shutdown racing `set_vm` to clear the slot while qemu kept running.
    pub async fn set_vm(&self, info: VmInfo, machine: KeepAliveMachine) {
        let domain = info.domain.clone();
        let mut vms = self.vms.write().await;
        vms.insert(
            domain,
            VmEntry {
                info,
                machine: Some(machine),
            },
        );
    }

    /// Mark the VM for `domain` as failed (if `id` still matches), clearing
    /// any machine handle so later cleanup does not double-shutdown.
    pub async fn set_vm_failed(&self, domain: &str, id: &str, error: String) {
        let mut vms = self.vms.write().await;
        if let Some(entry) = vms.get_mut(domain)
            && entry.info.id == id
        {
            entry.info.phase = VmPhase::Failed;
            entry.info.error = Some(error);
            entry.machine = None;
        }
    }

    /// Remove a domain slot and return its machine for shutdown.
    /// If the slot had no machine (Provisioning/Failed tombstone), the map
    /// entry is still removed and `None` is returned.
    pub async fn take_machine_by_domain(&self, domain: &str) -> Option<(VmInfo, KeepAliveMachine)> {
        let mut vms = self.vms.write().await;
        let entry = vms.remove(domain)?;
        match entry.machine {
            Some(m) => Some((entry.info, m)),
            None => None,
        }
    }

    /// Clear a domain slot without shutting down a machine (caller already did,
    /// or there was only a tombstone).
    pub async fn clear_domain(&self, domain: &str) {
        let mut vms = self.vms.write().await;
        vms.remove(domain);
    }

    /// Drain every tracked machine for process-exit / `/shutdown/all` teardown.
    pub async fn take_all_machines(&self) -> Vec<(VmInfo, KeepAliveMachine)> {
        let mut vms = self.vms.write().await;
        let mut out = Vec::new();
        for (_, entry) in vms.drain() {
            if let Some(m) = entry.machine {
                out.push((entry.info, m));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AppState {
        let config = Arc::new(tokio::sync::RwLock::new(crate::config::Config::new(
            "/tmp".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/ssh_key.pub".to_string(),
            Default::default(),
        )));
        AppState::new(config)
    }

    fn sample_info(domain: &str, id: &str, phase: VmPhase) -> VmInfo {
        VmInfo {
            id: id.to_string(),
            domain: domain.to_string(),
            target: "t".to_string(),
            phase,
            ip: None,
            created_at: std::time::Instant::now(),
            log_file: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn two_domains_coexist() {
        let state = test_state();
        state
            .set_vm_provisioning(sample_info("orion.a.com", "vm-a", VmPhase::Provisioning))
            .await;
        state
            .set_vm_provisioning(sample_info("orion.b.com", "vm-b", VmPhase::Provisioning))
            .await;
        assert_eq!(state.vm_count().await, 2);
        assert_eq!(
            state.get_vm_by_domain("orion.a.com").await.unwrap().id,
            "vm-a"
        );
        assert_eq!(
            state.get_vm_by_id("vm-b").await.unwrap().domain,
            "orion.b.com"
        );
    }

    #[tokio::test]
    async fn provisioning_to_failed() {
        let state = test_state();
        state
            .set_vm_provisioning(sample_info(
                "orion.a.com",
                "orion-vm-1",
                VmPhase::Provisioning,
            ))
            .await;
        state
            .set_vm_failed("orion.a.com", "orion-vm-1", "deploy failed".to_string())
            .await;
        let vm = state.get_vm_by_domain("orion.a.com").await.unwrap();
        assert_eq!(vm.phase, VmPhase::Failed);
        assert_eq!(vm.error.as_deref(), Some("deploy failed"));
    }
}
