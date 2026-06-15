//! Global registry mapping invocation handles to host contexts.
//!
//! When a plugin command is invoked, the host creates an [`FfiHostCtx`](super::ffi_host_ctx::FfiHostCtx) with a
//! unique handle, registers the underlying [`PoiseHostCtx`] here, and passes
//! the handle + callback table to the plugin. FFI callbacks use the handle to
//! look up the context and perform operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use crate::bot::host_ctx::PoiseHostCtx;

static REGISTRY: OnceLock<Mutex<HashMap<u64, Arc<PoiseHostCtx>>>> = OnceLock::new();
static SYSTEM_CTX: Mutex<Option<Arc<PoiseHostCtx>>> = Mutex::new(None);
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn registry() -> &'static Mutex<HashMap<u64, Arc<PoiseHostCtx>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registers a host context and returns a unique handle.
pub fn register(ctx: Arc<PoiseHostCtx>) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    registry().lock().unwrap().insert(handle, ctx);
    handle
}

/// Looks up a host context by handle.
pub fn get(handle: u64) -> Option<Arc<PoiseHostCtx>> {
    registry().lock().unwrap().get(&handle).cloned()
}

/// Removes a host context from the registry.
pub fn unregister(handle: u64) {
    registry().lock().unwrap().remove(&handle);
}

/// Sets the system-wide headless context for event/task dispatch.
pub fn set_system_ctx(ctx: Arc<PoiseHostCtx>) {
    *SYSTEM_CTX.lock().unwrap() = Some(ctx);
}

/// Returns the system-wide headless context.
pub fn system_ctx() -> Option<Arc<PoiseHostCtx>> {
    SYSTEM_CTX.lock().unwrap().clone()
}

/// Resets all global state for test isolation.
///
/// Clears the handle registry, system context, and handle counter.
/// Only intended for use in integration tests.
#[doc(hidden)]
pub fn reset_for_test() {
    registry().lock().unwrap().clear();
    *SYSTEM_CTX.lock().unwrap() = None;
    NEXT_HANDLE.store(1, Ordering::SeqCst);
}
