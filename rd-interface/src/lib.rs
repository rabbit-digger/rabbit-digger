pub use address::{Address, AddressDomain, IntoAddress};
pub use context::Context;
pub use error::{Error, ErrorContext, ErrorWithContext, Result, NOT_ENABLED, NOT_IMPLEMENTED};
pub use interface::*;
pub use rd_derive::{rd_config, Config};
pub use registry::Registry;
pub use schemars;
pub use serde_json::Value;
pub use util::{CombineNet, NotImplementedNet};

mod address;
pub mod config;
pub mod constant;
pub mod context;
pub mod error;
mod interface;
mod macros;
pub mod registry;
pub mod util;

/// Prelude for easy defining `Config` struct.
pub mod prelude {
    pub use rd_derive::rd_config;
    pub use schemars;
}
