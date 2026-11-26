//! Typed locators: `Found<T>` for the hook targets of the in-process transport.
//!
//! A locator resolves one named entry of the `signatures/*.toml` table of the running build
//! (format: `signatures/README.md`) and tags the address with what it is. The "unique or off" rule
//! of [`crate::signatures`] applies: zero matches, several matches and an address outside the
//! module are all failures, and each failure disables only the hook that needed the locator.
//!
//! Entry names this module asks for:
//!
//! | Name | Target | Used by |
//! |:---|:---|:---|
//! | `http_send` | entry of `ReqwestTransport::send` (inline hook) | REST requests |
//! | `http_poll` | `.rdata` slot of the `poll` entry of the response future's vtable | REST responses |
//! | `rust_alloc`, `rust_dealloc` | the host's allocator shims | replacing a body |
//! | `gateway_registry` | the event registry table | Gateway dispatch |
//! | `gateway_send` | entry of the outgoing frame encoder (`gateway_send_encode` in the recon notes) | Gateway send |

use crate::signatures::{ResolveError, Signature};
use std::fmt;

pub const HTTP_SEND: &str = "http_send";
pub const HTTP_POLL: &str = "http_poll";
pub const RUST_ALLOC: &str = "rust_alloc";
pub const RUST_DEALLOC: &str = "rust_dealloc";
pub const GATEWAY_REGISTRY: &str = "gateway_registry";
pub const GATEWAY_SEND: &str = "gateway_send";

/// Why a locator found nothing usable.
#[derive(Debug, PartialEq, Eq)]
pub enum LocatorError {
    /// The signature table of this build has no entry of that name.
    Missing(&'static str),
    /// The entry exists but did not resolve to exactly one place.
    Unresolved(&'static str, ResolveError),
    /// It resolved to an address outside the module image.
    OutOfImage(&'static str, u32),
}

impl fmt::Display for LocatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocatorError::Missing(n) => write!(f, "{n}: no signature for this build"),
            LocatorError::Unresolved(n, e) => write!(f, "{n}: {e}"),
            LocatorError::OutOfImage(n, rva) => write!(f, "{n}: rva {rva:#x} is outside the image"),
        }
    }
}

impl std::error::Error for LocatorError {}

/// A located address (an RVA) tagged with what it points at.
#[derive(Debug, PartialEq, Eq)]
pub struct Found<T> {
    pub rva: u32,
    pub target: T,
}

/// Entry of the transport's `send` function.
#[derive(Debug, PartialEq, Eq)]
pub struct HttpSend;
/// Slot of `poll` in the response future's vtable.
#[derive(Debug, PartialEq, Eq)]
pub struct HttpPoll;
/// The host's `__rust_alloc` and `__rust_dealloc` (the second is `dealloc_rva`).
#[derive(Debug, PartialEq, Eq)]
pub struct Allocator {
    pub dealloc_rva: u32,
}
/// The gateway event registry.
#[derive(Debug, PartialEq, Eq)]
pub struct GatewayRegistry;
/// The encoder of outgoing gateway frames (2.2.4): takes a typed command, not JSON.
#[derive(Debug, PartialEq, Eq)]
pub struct GatewaySend;

/// What a locator scans: the signatures of the selected build and the client's `.text`.
pub struct Scan<'a> {
    pub sigs: &'a [Signature],
    pub text: &'a [u8],
    pub text_rva: u32,
    /// `SizeOfImage`: every result must lie below it.
    pub image_len: u32,
}

impl Scan<'_> {
    fn rva(&self, name: &'static str) -> Result<u32, LocatorError> {
        let sig = self
            .sigs
            .iter()
            .find(|s| s.name == name)
            .ok_or(LocatorError::Missing(name))?;
        let rva = sig
            .resolve(self.text, self.text_rva)
            .map_err(|e| LocatorError::Unresolved(name, e))?;
        if rva >= self.image_len {
            return Err(LocatorError::OutOfImage(name, rva));
        }
        Ok(rva)
    }

    pub fn http_send(&self) -> Result<Found<HttpSend>, LocatorError> {
        Ok(Found {
            rva: self.rva(HTTP_SEND)?,
            target: HttpSend,
        })
    }

    pub fn http_poll(&self) -> Result<Found<HttpPoll>, LocatorError> {
        Ok(Found {
            rva: self.rva(HTTP_POLL)?,
            target: HttpPoll,
        })
    }

    /// Both shims or neither: a body allocated by one and freed by the wrong one corrupts the heap.
    pub fn allocator(&self) -> Result<Found<Allocator>, LocatorError> {
        let alloc = self.rva(RUST_ALLOC)?;
        let dealloc_rva = self.rva(RUST_DEALLOC)?;
        Ok(Found {
            rva: alloc,
            target: Allocator { dealloc_rva },
        })
    }

    pub fn gateway_registry(&self) -> Result<Found<GatewayRegistry>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_REGISTRY)?,
            target: GatewayRegistry,
        })
    }

    pub fn gateway_send(&self) -> Result<Found<GatewaySend>, LocatorError> {
        Ok(Found {
            rva: self.rva(GATEWAY_SEND)?,
            target: GatewaySend,
        })
    }
}

/// Every locator of the in-process transport, each resolved on its own.
#[derive(Debug)]
pub struct HookPlan {
    pub send: Result<Found<HttpSend>, LocatorError>,
    pub poll: Result<Found<HttpPoll>, LocatorError>,
    pub allocator: Result<Found<Allocator>, LocatorError>,
    pub registry: Result<Found<GatewayRegistry>, LocatorError>,
    pub gateway_send: Result<Found<GatewaySend>, LocatorError>,
}

impl HookPlan {
    pub fn locate(scan: &Scan<'_>) -> HookPlan {
        HookPlan {
            send: scan.http_send(),
            poll: scan.http_poll(),
            allocator: scan.allocator(),
            registry: scan.gateway_registry(),
            gateway_send: scan.gateway_send(),
        }
    }

    /// A plan with every locator missing: no hook is on.
    pub fn none() -> HookPlan {
        HookPlan::locate(&Scan {
            sigs: &[],
            text: &[],
            text_rva: 0,
            image_len: 0,
        })
    }

    pub fn requests(&self) -> bool {
        self.send.is_ok()
    }

    pub fn responses(&self) -> bool {
        self.poll.is_ok()
    }

    /// Without the allocator a changed body cannot be handed back; a local reply still can.
    pub fn can_replace_body(&self) -> bool {
        self.allocator.is_ok()
    }

    pub fn gateway(&self) -> bool {
        self.registry.is_ok()
    }

    /// One line per locator that failed, for the log.
    pub fn failures(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut note = |e: Option<&LocatorError>, off: &str| {
            if let Some(e) = e {
                out.push(format!("{e}; {off} stays off"));
            }
        };
        note(self.send.as_ref().err(), "REST request hook");
        note(self.poll.as_ref().err(), "REST response hook");
        note(self.allocator.as_ref().err(), "body replacement");
        note(self.registry.as_ref().err(), "Gateway hook");
        note(self.gateway_send.as_ref().err(), "Gateway send hook");
        out
    }
}
