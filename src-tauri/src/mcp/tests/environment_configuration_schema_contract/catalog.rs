use std::collections::BTreeSet;

#[test]
fn old_read_tool_catalog_and_budgets_remain_unchanged() {
    assert_eq!(
        super::super::super::protocol::MAX_TOOL_INPUT_BYTES,
        256 * 1024
    );
    assert_eq!(
        super::super::super::protocol::MAX_LOGICAL_OUTPUT_BYTES,
        8 * 1024 * 1024
    );
    let active = super::super::super::protocol::tools();
    let environment_names = [
        "mcp_environment_capabilities",
        "environment_candidate_create",
        "environment_candidate_status",
        "environment_candidate_cancel",
        "environment_candidate_apply",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let existing_reads = active
        .iter()
        .filter(|tool| !environment_names.contains(tool.name.as_ref()))
        .collect::<Vec<_>>();
    assert_eq!(existing_reads.len(), 34);
    assert!(existing_reads.iter().all(|tool| {
        tool.annotations
            .as_ref()
            .is_some_and(|a| a.read_only_hint == Some(true))
    }));
}
