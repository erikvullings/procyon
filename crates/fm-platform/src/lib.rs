//! Platform adapter traits (task 0058).
//!
//! Platform differences are expressed as explicit capabilities
//! (specification §3 rule 10) rather than as conditional compilation at every
//! call site. A fallback implementation keeps browser/server mode and
//! unsupported platforms working.

mod adapter;
mod capabilities;
mod error;
mod fallback;
mod types;

pub use adapter::PlatformAdapter;
pub use capabilities::PlatformCapabilities;
pub use error::PlatformError;
pub use fallback::FallbackPlatformAdapter;
pub use types::{
    ApplicationUninstallPlan, FinderTag, FinderTagColor, MountedVolume, SystemLocation,
    SystemLocationKind, SystemLocationProvider, UninstallCandidate, VolumeCapacity,
    cloud_provider_hint,
};
