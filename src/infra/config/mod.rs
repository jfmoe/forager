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

pub(crate) use crate::secure_fs::has_private_permissions;
pub use crate::secure_fs::{create_private_file, ensure_private_directory};

pub use edit::{SetupDocument, create_setup_template, set_file_value, unset_file_value};
pub use location::{ConfigError, ConfigLocation, EditError};
pub(crate) use route_config::{
    HnRouteRuntimeConfig, HttpRouteRuntimeConfig, KeyedHttpRouteRuntimeConfig,
    ModelRouteRuntimeConfig, PolymarketRouteRuntimeConfig, ProcessRouteRuntimeConfig,
    RedditRouteRuntimeConfig,
};
pub(crate) use runtime::{
    ClassifierRuntimeConfig, JournalRuntimeConfig, LogLevel, PlatformRouteConfig,
    PlatformRuntimeConfig, RuntimeConfig, platform_route_config, provider_keys_key, runtime_config,
};
#[cfg(test)]
pub(crate) use runtime::{PlatformRoutesRuntimeConfig, platform_order_key};
#[cfg(test)]
pub(crate) use schema::is_leaf;
pub(crate) use seam_config::{
    Context7RuntimeConfig, DocsSearchProviderConfig, DocsSearchRuntimeConfig, ExaRuntimeConfig,
    MainSearchProviderConfig, MainSearchRuntimeConfig, OpenaiRuntimeConfig, SeamEntry,
    WebFetchProviderConfig, WebFetchRuntimeConfig, WebSearchRuntimeConfig, XaiRuntimeConfig,
    docs_provider_config, main_provider_config, web_provider_config,
};
pub use view::{EffectiveConfigView, effective_view, effective_view_json};
