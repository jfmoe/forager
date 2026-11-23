//! The declared shape of a platform route. A route's module declares it once, beside its adapter:
//! how the provider factory builds it from its runtime configuration, the request checks planning
//! runs before it chooses the route, the platform operations it serves through the seam traits,
//! and what doctor and `forager browser login` add for it. The factory looks the declaration up
//! and knows nothing else about the route. A Platform Operation that only the route serves is
//! declared beside it as an operation route.

use std::future::Future;
use std::pin::Pin;

use reqwest::Client;

use super::browser::LoginPage;
use super::key_account::KeyAccount;
use super::{PlatformFetch, PlatformSearch, ProviderId};
use crate::config::PlatformRoutesRuntimeConfig;
use crate::net::RetryPolicy;
use crate::types::{Deadline, PlatformFetchRequest, PlatformSearchRequest, ProviderError};

/// What the provider factory hands every route it builds.
#[derive(Clone)]
pub(crate) struct RouteContext {
    pub(crate) client: Client,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) deadline: Deadline,
}

/// A named doctor check of one endpoint that a route combines beyond the one its probe reaches.
pub(crate) type ExtraCheck = (&'static str, Result<(), ProviderError>);

/// A platform route built from its declaration, as the platform operations of the catalog reach
/// it.
pub(crate) trait PlatformRoute: Send + Sync {
    /// The route as a platform search, or `None` when its adapter checks no search request.
    fn into_search(self: Box<Self>) -> Option<Box<dyn PlatformSearch>>;

    /// The route as a platform fetch, or `None` when its adapter checks no fetch request.
    fn into_fetch(self: Box<Self>) -> Option<Box<dyn PlatformFetch>>;

    /// Checks each extra endpoint the route combines; doctor runs them after the route's probe
    /// passed. Most routes combine none.
    fn extra_checks(&self) -> Pin<Box<dyn Future<Output = Vec<ExtraCheck>> + Send + '_>> {
        Box::pin(std::future::ready(Vec::new()))
    }

    /// Checks the account of every configured key, or `None` for a route whose registration
    /// declares no account probe.
    fn accounts(&self) -> Option<Pin<Box<dyn Future<Output = Vec<KeyAccount>> + Send + '_>>> {
        None
    }
}

/// The one declaration of a platform route, over its runtime configuration `C`.
pub(crate) struct RouteDeclaration<C> {
    pub(crate) build: fn(C, RouteContext) -> Box<dyn PlatformRoute>,
    pub(crate) adapter: RouteAdapter,
}

/// Checks a platform search request without sending it.
pub(crate) type SearchSupport = fn(&PlatformSearchRequest) -> Result<(), String>;

/// Checks a platform fetch request without sending it.
pub(crate) type FetchSupport = fn(&PlatformFetchRequest) -> Result<(), String>;

/// The answer of a route's inherent method to one request.
pub(crate) type OperationFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

/// How a Platform Operation that one route serves runs on that route, declared in the route's
/// module: the route, where its runtime configuration is, the check of the operation's request
/// `Q` that planning runs before it chooses the route, and the route's inherent method that
/// answers the input `I` it runs on. The configuration it hands that method is always the
/// route's own, so the method needs no check of which route it got.
pub(crate) struct OperationRoute<C: 'static, Q: ?Sized + 'static, I: ?Sized + 'static, T: 'static> {
    pub(crate) route: ProviderId,
    pub(crate) config: fn(&PlatformRoutesRuntimeConfig) -> &C,
    pub(crate) support: fn(&Q) -> Result<(), String>,
    pub(crate) run: for<'a> fn(C, RouteContext, &'a I) -> OperationFuture<'a, T>,
}

/// The request check of an operation whose requests need none beyond what parsing guarantees.
#[expect(
    clippy::unnecessary_wraps,
    reason = "matches the request check of an operation route"
)]
pub(crate) fn any_request<R: ?Sized>(_: &R) -> Result<(), String> {
    Ok(())
}

/// The half of a route declaration that needs no configuration: what planning, the platform
/// checklist and `forager browser login` read before any route is built.
#[derive(Clone, Copy)]
pub(crate) struct RouteAdapter {
    /// `None` when the route serves no platform search.
    pub(crate) search_support: Option<SearchSupport>,
    /// `None` when the route serves no platform fetch.
    pub(crate) fetch_support: Option<FetchSupport>,
    /// The login page logic of a browser route.
    pub(crate) login_page: Option<LoginPage>,
}
