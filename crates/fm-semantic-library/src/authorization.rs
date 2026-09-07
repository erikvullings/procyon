use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use fm_domain::WorkspaceId;

use crate::{
    CatalogError, CatalogUsage, LibraryId, RootId, SemanticCatalog, SemanticLibraryPolicy,
    TenantId, UserId,
};

/// Hard administrator-defined library quotas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardQuotas {
    /// Maximum enrolled roots.
    pub max_roots: u64,
    /// Maximum catalogued documents.
    pub max_documents: u64,
    /// Maximum represented source bytes.
    pub max_source_bytes: u64,
    /// Maximum retained normalized-excerpt bytes.
    pub max_extracted_bytes: u64,
    /// Maximum vector bytes.
    pub max_vector_bytes: u64,
}

impl Default for HardQuotas {
    fn default() -> Self {
        Self {
            max_roots: 32,
            max_documents: 1_000_000,
            max_source_bytes: 4 * 1024 * 1024 * 1024 * 1024,
            max_extracted_bytes: 1024 * 1024 * 1024 * 1024,
            max_vector_bytes: 1024 * 1024 * 1024 * 1024,
        }
    }
}

/// Authoritative library consumption measured from durable records.
///
/// There is deliberately no constructor that accepts caller-asserted totals:
/// a client cannot claim zero usage to slip past a hard quota.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuotaUsage {
    roots: u64,
    catalog: CatalogUsage,
}

impl QuotaUsage {
    /// Measures current consumption from the enrolled roots and the catalog.
    ///
    /// # Errors
    ///
    /// Rejects a policy and catalog belonging to different libraries.
    pub fn measure(
        policy: &SemanticLibraryPolicy,
        catalog: &SemanticCatalog,
    ) -> Result<Self, CatalogError> {
        catalog.ensure_policy_library(policy)?;
        Ok(Self {
            roots: u64::try_from(policy.roots().len()).unwrap_or(u64::MAX),
            catalog: catalog.measured_usage(),
        })
    }

    pub(crate) fn projected(
        policy: &SemanticLibraryPolicy,
        catalog: CatalogUsage,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            roots: u64::try_from(policy.roots().len()).unwrap_or(u64::MAX),
            catalog,
        })
    }

    fn with_additional_root(self) -> Result<Self, AuthorizationError> {
        Ok(Self {
            roots: self
                .roots
                .checked_add(1)
                .ok_or(AuthorizationError::QuotaExceeded)?,
            catalog: self.catalog,
        })
    }

    /// Returns enrolled roots.
    #[must_use]
    pub const fn roots(self) -> u64 {
        self.roots
    }

    /// Returns measured catalog consumption.
    #[must_use]
    pub const fn catalog(self) -> CatalogUsage {
        self.catalog
    }
}

pub(crate) fn enforce_quotas(
    quotas: HardQuotas,
    usage: QuotaUsage,
) -> Result<(), AuthorizationError> {
    if usage.roots > quotas.max_roots
        || usage.catalog.documents() > quotas.max_documents
        || usage.catalog.source_bytes() > quotas.max_source_bytes
        || usage.catalog.extracted_bytes() > quotas.max_extracted_bytes
        || usage.catalog.vector_bytes() > quotas.max_vector_bytes
    {
        return Err(AuthorizationError::QuotaExceeded);
    }
    Ok(())
}

/// Administrator-defined provider and enrolment permissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEnrolmentPolicy {
    allowed_providers: BTreeSet<String>,
    users_may_enrol_roots: bool,
}

impl ServerEnrolmentPolicy {
    /// Creates the initial server posture: local roots, administrator-managed.
    #[must_use]
    pub fn local_only() -> Self {
        Self {
            allowed_providers: ["local".to_owned()].into(),
            users_may_enrol_roots: false,
        }
    }

    /// Creates an explicit administrator policy.
    #[must_use]
    pub fn new(
        allowed_providers: impl IntoIterator<Item = String>,
        users_may_enrol_roots: bool,
    ) -> Self {
        Self {
            allowed_providers: allowed_providers.into_iter().collect(),
            users_may_enrol_roots,
        }
    }

    /// Reports whether a provider can be enrolled.
    #[must_use]
    pub fn allows_provider(&self, provider_id: &str) -> bool {
        self.allowed_providers.contains(provider_id)
    }

    /// Reports whether non-administrator users may enrol roots.
    #[must_use]
    pub const fn users_may_enrol_roots(&self) -> bool {
        self.users_may_enrol_roots
    }
}

/// Authenticated tenant/library/user triple checked before catalog access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessContext {
    /// Tenant boundary.
    pub tenant_id: TenantId,
    /// Library boundary.
    pub library_id: LibraryId,
    /// Authenticated user boundary.
    pub user_id: UserId,
}

/// Retrieval request whose complete authorization scope is enforced in core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticQueryRequest {
    access: AccessContext,
    workspace_id: WorkspaceId,
    root_ids: BTreeSet<RootId>,
}

impl SemanticQueryRequest {
    /// Creates a tenant/library/user/workspace/root-scoped query.
    #[must_use]
    pub fn new(
        access: AccessContext,
        workspace_id: WorkspaceId,
        root_ids: impl IntoIterator<Item = RootId>,
    ) -> Self {
        Self {
            access,
            workspace_id,
            root_ids: root_ids.into_iter().collect(),
        }
    }

    /// Returns the authenticated access context.
    #[must_use]
    pub const fn access(&self) -> &AccessContext {
        &self.access
    }

    /// Returns the active workspace.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// Returns explicitly requested roots.
    #[must_use]
    pub const fn root_ids(&self) -> &BTreeSet<RootId> {
        &self.root_ids
    }
}

impl AccessContext {
    /// Creates an access context.
    #[must_use]
    pub const fn new(tenant_id: TenantId, library_id: LibraryId, user_id: UserId) -> Self {
        Self {
            tenant_id,
            library_id,
            user_id,
        }
    }
}

/// Administrator-defined tenant library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantLibrary {
    tenant_id: TenantId,
    library_id: LibraryId,
    administrators: BTreeSet<UserId>,
    authorized_users: BTreeSet<UserId>,
    enrolment_policy: ServerEnrolmentPolicy,
    quotas: HardQuotas,
}

impl TenantLibrary {
    /// Creates a tenant library with explicit users and administrators.
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        library_id: LibraryId,
        administrators: impl IntoIterator<Item = UserId>,
        authorized_users: impl IntoIterator<Item = UserId>,
        enrolment_policy: ServerEnrolmentPolicy,
        quotas: HardQuotas,
    ) -> Self {
        let administrators: BTreeSet<_> = administrators.into_iter().collect();
        let mut authorized_users: BTreeSet<_> = authorized_users.into_iter().collect();
        authorized_users.extend(administrators.iter().cloned());
        Self {
            tenant_id,
            library_id,
            administrators,
            authorized_users,
            enrolment_policy,
            quotas,
        }
    }

    /// Returns hard quotas.
    #[must_use]
    pub const fn quotas(&self) -> HardQuotas {
        self.quotas
    }

    /// Returns enrolment permissions.
    #[must_use]
    pub const fn enrolment_policy(&self) -> &ServerEnrolmentPolicy {
        &self.enrolment_policy
    }

    /// Reports whether a user is an administrator.
    #[must_use]
    pub fn is_administrator(&self, user_id: &UserId) -> bool {
        self.administrators.contains(user_id)
    }
}

/// Retrieval authorization configuration for server mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPolicy {
    libraries: BTreeMap<(TenantId, LibraryId), TenantLibrary>,
}

impl ServerPolicy {
    /// Creates the pre-administration posture: one private library accessible
    /// only to its administrator.
    #[must_use]
    pub fn single_private(
        tenant_id: TenantId,
        library_id: LibraryId,
        administrator: UserId,
        enrolment_policy: ServerEnrolmentPolicy,
        quotas: HardQuotas,
    ) -> Self {
        Self::administrator_defined([TenantLibrary::new(
            tenant_id,
            library_id,
            [administrator],
            [],
            enrolment_policy,
            quotas,
        )])
    }

    /// Creates an administrator-defined multi-tenant policy.
    #[must_use]
    pub fn administrator_defined(libraries: impl IntoIterator<Item = TenantLibrary>) -> Self {
        Self {
            libraries: libraries
                .into_iter()
                .map(|library| ((library.tenant_id.clone(), library.library_id), library))
                .collect(),
        }
    }

    /// Authorizes access and returns the matching hard policy.
    ///
    /// # Errors
    ///
    /// Denies cross-tenant, sibling-library, and unauthorized-user requests.
    pub fn authorize(&self, context: &AccessContext) -> Result<&TenantLibrary, AuthorizationError> {
        if self
            .libraries
            .keys()
            .filter(|(_, library_id)| *library_id == context.library_id)
            .take(2)
            .count()
            > 1
        {
            return Err(AuthorizationError::LibraryDenied);
        }
        if !self
            .libraries
            .keys()
            .any(|(tenant_id, _)| tenant_id == &context.tenant_id)
        {
            return Err(AuthorizationError::TenantDenied);
        }
        let library = self
            .libraries
            .get(&(context.tenant_id.clone(), context.library_id))
            .ok_or(AuthorizationError::LibraryDenied)?;
        if !library.authorized_users.contains(&context.user_id) {
            return Err(AuthorizationError::UserDenied);
        }
        Ok(library)
    }

    /// Authorizes enrolling one more root against administrator policy and
    /// hard quotas measured from durable records.
    ///
    /// The projection is derived here from the policy and catalog; callers
    /// cannot assert their own usage.
    ///
    /// # Errors
    ///
    /// Denies unauthorized users, non-administrator mutations where disabled,
    /// disallowed providers, and any hard-quota breach.
    pub fn authorize_root_enrolment(
        &self,
        context: &AccessContext,
        provider_id: &str,
        policy: &SemanticLibraryPolicy,
        catalog: &SemanticCatalog,
    ) -> Result<&TenantLibrary, AuthorizationError> {
        let library = self.authorize(context)?;
        if context.library_id != catalog.library_id() {
            return Err(AuthorizationError::LibraryDenied);
        }
        if !library.is_administrator(&context.user_id)
            && !library.enrolment_policy.users_may_enrol_roots()
        {
            return Err(AuthorizationError::AdministratorRequired);
        }
        if !library.enrolment_policy.allows_provider(provider_id) {
            return Err(AuthorizationError::ProviderDenied);
        }
        let projected = QuotaUsage::measure(policy, catalog)
            .map_err(|_| AuthorizationError::LibraryDenied)?
            .with_additional_root()?;
        enforce_quotas(library.quotas, projected)?;
        Ok(library)
    }
}

/// Server authorization denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthorizationError {
    /// Tenant does not exist or is outside the authenticated boundary.
    #[error("semantic tenant access denied")]
    TenantDenied,
    /// Library does not belong to the authorized tenant.
    #[error("semantic library access denied")]
    LibraryDenied,
    /// User is not authorized for the library.
    #[error("semantic user access denied")]
    UserDenied,
    /// Requested workspace/root occurrence scope is not authorized.
    #[error("semantic occurrence scope access denied")]
    ScopeDenied,
    /// Enrolment is restricted to administrators.
    #[error("semantic enrolment requires an administrator")]
    AdministratorRequired,
    /// Provider is outside the administrator enrolment policy.
    #[error("semantic provider enrolment is denied")]
    ProviderDenied,
    /// Projected state exceeds a hard administrator quota.
    #[error("semantic library quota exceeded")]
    QuotaExceeded,
}
