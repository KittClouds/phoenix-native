use super::*;

pub(super) fn read_state(
    shared: &KernelShared,
) -> Result<std::sync::RwLockReadGuard<'_, KernelState>, KernelError> {
    shared
        .state
        .read()
        .map_err(|_| KernelError::Poisoned("state read"))
}

pub(super) fn write_state(
    shared: &KernelShared,
) -> Result<std::sync::RwLockWriteGuard<'_, KernelState>, KernelError> {
    shared
        .state
        .write()
        .map_err(|_| KernelError::Poisoned("state write"))
}

pub(super) fn checked_revision(current: u64) -> Result<u64, KernelError> {
    current
        .checked_add(1)
        .ok_or(KernelError::CoordinatorUnavailable)
}

pub(super) fn receipt(sequence: u64, revision: u64, outcome: KernelOutcome) -> CommandReceipt {
    CommandReceipt {
        sequence,
        kernel_revision: revision,
        outcome,
    }
}
