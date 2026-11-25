//! Panic guard for every hook entry point.
//!
//! A hook runs on the client's threads. A panic must never unwind into Discord's frames, so each
//! entry runs under [`Breaker::guard`]: the panic is caught, logged, the hook is switched off for
//! good and the caller gets the fallback (the original behaviour: pass the traffic through).

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Where hook problems are written (the loader log in the DLL, a buffer in tests).
pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Breaker {
    name: &'static str,
    tripped: AtomicBool,
}

impl Breaker {
    pub const fn new(name: &'static str) -> Breaker {
        Breaker {
            name,
            tripped: AtomicBool::new(false),
        }
    }

    pub fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::Relaxed)
    }

    /// Runs `f`; on a panic, or when the breaker already tripped, returns `fallback()` instead.
    pub fn guard<R>(&self, log: &LogFn, fallback: impl FnOnce() -> R, f: impl FnOnce() -> R) -> R {
        if self.is_tripped() {
            return fallback();
        }
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(r) => r,
            Err(payload) => {
                self.tripped.store(true, Ordering::Relaxed);
                let why = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic".into());
                // A logger that panics too must not escape either.
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    log(&format!("hook {}: panic: {why}; hook disabled", self.name));
                }));
                fallback()
            }
        }
    }

    /// Like [`Breaker::guard`] for a body that can fail on bad input: an `Err` (malformed state)
    /// is logged and falls back for this one call, without switching the hook off.
    pub fn guard_result<R>(
        &self,
        log: &LogFn,
        fallback: impl FnOnce() -> R,
        f: impl FnOnce() -> Result<R, String>,
    ) -> R {
        let mut failed = None;
        let r = self.guard(
            log,
            || None,
            || match f() {
                Ok(r) => Some(r),
                Err(e) => {
                    failed = Some(e);
                    None
                }
            },
        );
        if let Some(e) = failed {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                log(&format!("hook {}: {e}; passed through", self.name));
            }));
        }
        r.unwrap_or_else(fallback)
    }
}
