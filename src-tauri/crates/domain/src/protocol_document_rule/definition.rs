use super::{
    DocumentAction, DocumentCondition, DomainError, ListenerId, ProtocolDirection,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleDraft, ProtocolDocumentRuleId,
    ProtocolDocumentRuleWire, ProtocolPackageRef, ProtocolRuleStage, Revision,
};

impl ProtocolDocumentRuleDefinition {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn restore_from_unified(
        rule_id: ProtocolDocumentRuleId,
        revision: Revision,
        name: String,
        enabled: bool,
        priority: i32,
        created_order: u64,
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        stage: ProtocolRuleStage,
        conditions: Vec<DocumentCondition>,
        actions: Vec<DocumentAction>,
    ) -> Result<Self, DomainError> {
        Self::from_wire(ProtocolDocumentRuleWire {
            rule_id,
            revision,
            name,
            enabled,
            priority,
            created_order,
            listener_id,
            package,
            stage,
            conditions,
            actions,
        })
    }
}

impl ProtocolRuleStage {
    #[must_use]
    pub const fn direction(self) -> ProtocolDirection {
        match self {
            Self::AppToProxy | Self::ProxyToUpstream => ProtocolDirection::Upstream,
            Self::UpstreamToProxy | Self::ProxyToApp => ProtocolDirection::Downstream,
        }
    }
}

impl DocumentAction {
    /// 是否会修改 Document；Workspace 用它强制要求对应方向开启 Encode。
    #[must_use]
    pub const fn modifies_document(&self) -> bool {
        matches!(self, Self::SetField { .. } | Self::ClearField { .. })
    }
}

impl ProtocolDocumentRuleDefinition {
    /// Returns the stable rule identity.
    #[must_use]
    pub const fn id(&self) -> ProtocolDocumentRuleId {
        self.rule_id
    }

    #[must_use]
    pub const fn rule_id(&self) -> ProtocolDocumentRuleId {
        self.rule_id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.priority
    }

    #[must_use]
    pub const fn created_order(&self) -> u64 {
        self.created_order
    }

    #[must_use]
    pub const fn listener_id(&self) -> ListenerId {
        self.listener_id
    }

    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    #[must_use]
    pub const fn direction(&self) -> ProtocolDirection {
        self.stage.direction()
    }

    #[must_use]
    pub const fn stage(&self) -> ProtocolRuleStage {
        self.stage
    }

    #[must_use]
    pub fn conditions(&self) -> &[DocumentCondition] {
        &self.conditions
    }

    #[must_use]
    pub fn actions(&self) -> &[DocumentAction] {
        &self.actions
    }

    #[must_use]
    pub fn modifies_document(&self) -> bool {
        self.actions.iter().any(DocumentAction::modifies_document)
    }

    /// 返回不携带身份、revision 和创建顺序的可编辑 Draft。
    #[must_use]
    pub fn to_draft(&self) -> ProtocolDocumentRuleDraft {
        ProtocolDocumentRuleDraft {
            name: self.name.clone(),
            enabled: self.enabled,
            priority: self.priority,
            listener_id: self.listener_id,
            package: self.package.clone(),
            stage: self.stage,
            conditions: self.conditions.clone(),
            actions: self.actions.clone(),
        }
    }
}
