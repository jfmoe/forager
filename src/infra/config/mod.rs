//! Configuration schema, layered loading, effective views, and private writes.

mod edit;
mod load;
mod location;
mod route_config;
mod runtime;
mod schema;
mod seam_config;
mod validate;
mod view;

pub use crate::secure_fs::{
    create_private_file, ensure_private_directory, has_private_permissions,
};

pub use edit::{SetupDocument, create_setup_template, set_file_value, unset_file_value};
pub use location::{ConfigError, ConfigLocation, EditError};
pub(crate) use route_config::{
    BrowserRouteRuntimeConfig, BrowserRuntimeConfig, HnRouteRuntimeConfig, HttpRouteRuntimeConfig,
    KeyedHttpRouteRuntimeConfig, ModelRouteRuntimeConfig, PolymarketRouteRuntimeConfig,
};
pub(crate) use runtime::{
    JournalRuntimeConfig, LogLevel, PlatformRouteConfig, PlatformRoutesRuntimeConfig,
    PlatformRuntimeConfig, RuntimeConfig, platform_route_config, provider_keys_key, runtime_config,
};
#[cfg(test)]
pub(crate) use runtime::{default_platform_routes, platform_order_key, runtime_config_from_toml};
#[cfg(test)]
pub(crate) use schema::{BrowserRoute, is_leaf};
pub(crate) use seam_config::{
    DocsSearchProviderConfig, DocsSearchRuntimeConfig, ExaRuntimeConfig, MainSearchProviderConfig,
    MainSearchRuntimeConfig, OpenaiRuntimeConfig, SeamEntry, WebFetchProviderConfig,
    WebFetchRuntimeConfig, WebSearchRuntimeConfig, XaiRuntimeConfig,
};
pub use view::{effective_view, effective_view_json};
