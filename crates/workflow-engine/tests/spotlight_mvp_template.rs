use workflow_engine::WorkflowTemplate;

#[test]
fn spotlight_mvp_template_matches_bootstrap_task_flow() {
    let template = WorkflowTemplate::spotlight_mvp();

    assert_eq!(template.key, "spotlight_mvp_task_flow");
    assert_eq!(template.version, 1);
    assert_eq!(
        template.states,
        vec![
            "open",
            "claimed",
            "running",
            "agent_done",
            "pending_acceptance",
        ]
    );
}

#[test]
fn spotlight_mvp_template_starts_open_and_ends_pending_acceptance() {
    let template = WorkflowTemplate::spotlight_mvp();

    assert_eq!(template.states.first().map(String::as_str), Some("open"));
    assert_eq!(
        template.states.last().map(String::as_str),
        Some("pending_acceptance")
    );
    assert!(
        template.states.windows(2).all(|pair| pair[0] != pair[1]),
        "workflow template should not repeat adjacent states"
    );
}
