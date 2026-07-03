use std::{sync::Arc, time::Duration};

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

/// Represents the current state of the VM
#[derive(Debug, Clone)]
pub struct VmInfo {
    pub id: String,
    pub phase: VmPhase,
    pub ip: Option<String>,
    pub created_at: std::time::Instant,
    /// Path to the Orion log file
    pub log_file: Option<String>,
    /// Error message when phase is Failed
    pub error: Option<String>,
}

/// Global state for tracking VM lifecycle
pub struct AppState {
    pub vm: Arc<RwLock<Option<VmInfo>>>,
    pub machine: Arc<RwLock<Option<KeepAliveMachine>>>,
    pub config: SharedConfig,
    /// Single-flight mutex guarding the full VM update sequence
    /// (shutdown existing VM → create new VM → publish to state).
    /// Without this, two concurrent /webhook calls can both pass the
    /// existing-VM check before either stores its new machine, leaking
    /// the earlier qemu process out of `state` and out of `/shutdown`'s reach.
    update_lock: Arc<Mutex<()>>,
}

impl AppState {
    /// Create a new AppState with empty VM and machine slots
    pub fn new(config: SharedConfig) -> Self {
        Self {
            vm: Arc::new(RwLock::new(None)),
            machine: Arc::new(RwLock::new(None)),
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
    /// `KeepAliveMachine::new` and `set_vm`, which would otherwise see an
    /// empty state and leave the freshly-spawned qemu untracked.
    pub async fn lock_update(&self) -> MutexGuard<'_, ()> {
        self.update_lock.lock().await
    }

    /// Like `lock_update`, but bounded so signal handlers don't hang the
    /// process behind a multi-minute create. Returns `None` if the lock
    /// could not be acquired within `timeout`; callers must then fall back
    /// to a force-kill safety net.
    pub async fn try_lock_update(&self, timeout: Duration) -> Option<MutexGuard<'_, ()>> {
        tokio::time::timeout(timeout, self.update_lock.lock())
            .await
            .ok()
    }

    /// Register a VM in provisioning state (no machine handle yet).
    pub async fn set_vm_provisioning(&self, info: VmInfo) {
        let mut vm = self.vm.write().await;
        let mut m = self.machine.write().await;
        *vm = Some(info);
        *m = None;
    }

    /// Set VM info and machine reference together atomically.
    /// Both write locks are held simultaneously so concurrent readers
    /// never observe a half-published state (e.g. `vm = Some` with
    /// `machine = None`), which previously allowed a shutdown racing
    /// `set_vm` to clear the entry while the qemu kept running.
    pub async fn set_vm(&self, info: VmInfo, machine: KeepAliveMachine) {
        let mut vm = self.vm.write().await;
        let mut m = self.machine.write().await;
        *vm = Some(info);
        *m = Some(machine);
    }

    /// Mark the current VM as failed, clearing any machine handle.
    pub async fn set_vm_failed(&self, id: &str, error: String) {
        let mut vm = self.vm.write().await;
        let mut m = self.machine.write().await;
        if let Some(info) = vm.as_mut()
            && info.id == id
        {
            info.phase = VmPhase::Failed;
            info.error = Some(error);
        }
        *m = None;
    }

    /// Clear both VM info and machine reference atomically.
    pub async fn clear_vm(&self) {
        let mut vm = self.vm.write().await;
        let mut m = self.machine.write().await;
        *vm = None;
        *m = None;
    }

    /// Get a clone of the current VM info if any
    pub async fn get_vm(&self) -> Option<VmInfo> {
        let vm = self.vm.read().await;
        vm.clone()
    }

    /// Get a clone of the current machine reference if any
    pub async fn get_machine(&self) -> Option<KeepAliveMachine> {
        let m = self.machine.read().await;
        m.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_set_clear() {
        let config = Arc::new(tokio::sync::RwLock::new(crate::config::Config::new(
            "/tmp".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/ssh_key.pub".to_string(),
            Default::default(),
        )));
        let state = AppState::new(config);
        assert!(state.get_vm().await.is_none());
        assert!(state.get_machine().await.is_none());
    }

    #[tokio::test]
    async fn test_provisioning_to_failed() {
        let config = Arc::new(tokio::sync::RwLock::new(crate::config::Config::new(
            "/tmp".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/orion".to_string(),
            "/tmp/ssh_key.pub".to_string(),
            Default::default(),
        )));
        let state = AppState::new(config);
        let info = VmInfo {
            id: "orion-vm-1".to_string(),
            phase: VmPhase::Provisioning,
            ip: None,
            created_at: std::time::Instant::now(),
            log_file: None,
            error: None,
        };
        state.set_vm_provisioning(info).await;
        let vm = state.get_vm().await.unwrap();
        assert_eq!(vm.phase, VmPhase::Provisioning);
        assert!(state.get_machine().await.is_none());

        state
            .set_vm_failed("orion-vm-1", "deploy failed".to_string())
            .await;
        let vm = state.get_vm().await.unwrap();
        assert_eq!(vm.phase, VmPhase::Failed);
        assert_eq!(vm.error.as_deref(), Some("deploy failed"));
    }
}
