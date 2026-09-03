//! Configuration schema, layered loading, effective views, and private writes.

mod edit;
mod load;
mod location;
mod runtime;
mod schema;
mod validate;
mod view;

pub(crate) use crate::secure_fs::has_private_permissions;
pub use crate::secure_fs::{create_private_file, ensure_private_directory};

pub use edit::{SetupDocument, create_setup_template, set_file_value, unset_file_value};
pub use location::{ConfigError, ConfigLocation, EditError};
pub(crate) use runtime::{
    AnysearchRuntimeConfig, ClassifierRuntimeConfig, Context7RuntimeConfig,
    DocsSearchProviderConfig, DocsSearchRuntimeConfig, ExaRuntimeConfig, JournalRuntimeConfig,
    LogLevel, MainSearchProviderConfig, MainSearchRuntimeConfig, OpenAiCompatibleRuntimeConfig,
    RuntimeConfig, SeamEntry, VerticalSearchRuntimeConfig, WebFetchProviderConfig,
    WebFetchRuntimeConfig, WebSearchRuntimeConfig, XaiRuntimeConfig, docs_provider_config,
    main_provider_config, runtime_config, web_provider_config,
};
pub use view::{EffectiveConfigView, effective_view, effective_view_json};
