//! Stopping a task. The TUI, a test or `SIGINT` sets the flag, and the model stream, the agent
//! loop and the running tool read it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Set by the `SIGINT` handler. A signal handler may only touch an atomic like this one.
static SIGINT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    signal: bool,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// A cancel that `SIGINT` also sets. It installs the handler for this process.
    pub fn on_sigint() -> Self {
        install_sigint();
        Self {
            flag: Arc::default(),
            signal: true,
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst) || (self.signal && SIGINT.load(Ordering::SeqCst))
    }
}

extern "C" fn on_sigint(_: libc::c_int) {
    SIGINT.store(true, Ordering::SeqCst);
}

fn install_sigint() {
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_sigint as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        // SA_RESETHAND: the first SIGINT stops the task, and a second one ends the process, so
        // a run stuck where the flag is not read can still be stopped. SA_RESTART: a read or a
        // wait that the signal interrupts starts again instead of failing.
        action.sa_flags = libc::SA_RESETHAND | libc::SA_RESTART;
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    }
}
