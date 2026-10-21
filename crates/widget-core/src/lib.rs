//! Pure, I/O-free logic for the UniFi widget: response parsing, billing-cycle
//! maths, CGNAT detection and settings validation. HTTP and UI live elsewhere.

pub mod history;
pub mod ip;
pub mod settings;
pub mod unifi;
pub mod usage;

/// Timezone type used by the billing cycle, re-exported for callers.
pub use chrono_tz::Tz;
