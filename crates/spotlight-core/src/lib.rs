//! UI-independent core for Spotlight Linux.
//!
//! The crate deliberately contains no GTK types. Providers, ranking, persistence,
//! and cancellation can therefore be tested without a display server and reused by
//! a future out-of-process extension host.

pub mod cancellation;
pub mod desktop_entry;
pub mod history;
pub mod model;
pub mod performance;
pub mod provider;
pub mod providers;
pub mod ranking;
pub mod routing;
pub mod search;
pub mod settings;

pub use cancellation::{CancellationToken, GenerationClock, QueryGeneration};
pub use model::{Action, Icon, ProviderId, SearchQuery, SearchResult};
pub use provider::{Provider, ProviderClass, ProviderDescriptor, ProviderError};
pub use routing::{QueryRoute, route_query};
pub use search::SearchEngine;
