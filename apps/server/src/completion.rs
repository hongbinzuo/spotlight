use platform_core::{
    new_activity, new_runtime_entry, PendingQuestion, RuntimeLogEntry, Task, TaskPriority,
    TaskStateSnapshot, TaskStatus,
};
use uuid::Uuid;

use crate::models::*;
use crate::BoardState;

pub(crate) fn process_task_completion_outputs(state: &mut BoardState, task_id: Uuid) -> bool {
    let Some(source_task) = state.tasks.iter().find(|task| task.id == task_id).cloned() else {
        return false;
    };
    let Some(runtime) = source_task.runtime.as_ref() else {
        return false;
    };
    let Some(report) = extract_task_completion_report(&runtime.log) else {
        return false;
    };
    let summary_text = report
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut auto_created_task_count = 0;
    let mut pending_question_count = 0;
    let mut risk_count = 0;

    for follow_up in &report.follow_ups {
        if !should_auto_create_follow_up_task(follow_up) {
            continue;
        }

        let title = follow_up.title.trim();
        let description = follow_up.description.trim();
        if title.is_empty() || description.is_empty() {
            continue;
        }

        let already_exists = state.tasks.iter().any(|task| {
            task.project_id == source_task.project_id
                && task.source_task_id == Some(source_task.id)
                && task.title == title
        });
        if already_exists {
            continue;
        }

        state.tasks.insert(
            0,
            Task {
                id: Uuid::new_v4(),
                project_id: source_task.project_id,
                title: title.to_string(),
                description: compose_follow_up_task_description(&source_task, follow_up),
                status: TaskStatus::Open,
                priority: parse_follow_up_priority(follow_up.priority.as_deref()),
                labels: Vec::new(),
                creator_user_id: source_task.creator_user_id,
                assignee_user_id: None,
                assignment_mode: platform_core::TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: Some(source_task.id),
                claimed_by: None,
                activities: vec![new_activity(
                    "task.auto_created_from_completion",
                    format!(
                        "该任务由\u{201c}{}\u{201d}完成后的结构化建议自动生成",
                        source_task.title
                    ),
                )],
                runtime: None,
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            },
        );
        auto_created_task_count += 1;
    }

    for question in &report.questions {
        let Some((question_text, context)) = normalize_completion_question(question) else {
            continue;
        };
        let already_exists = state.pending_questions.iter().any(|item| {
            item.source_task_id == source_task.id
                && item.question == question_text
                && item.status != "answered"
        });
        if already_exists {
            continue;
        }

        state.pending_questions.push(PendingQuestion {
            id: Uuid::new_v4(),
            project_id: source_task.project_id,
            source_task_id: source_task.id,
            source_task_title: source_task.title.clone(),
            question: question_text,
            context,
            status: "open".into(),
            answer: None,
            created_at: crate::timestamp_string(),
            answered_at: None,
        });
        pending_question_count += 1;
    }

    if let Some(summary) = summary_text.as_ref() {
        crate::write_memory_revision(
            state,
            MemoryWriteSpec {
                scope_kind: "task",
                scope_id: source_task.id,
                memory_kind: "task_summary",
                stable_key: format!("task_summary/{}", source_task.id),
                tag: format!("task/{}/latest-summary", source_task.id),
                title: format!("任务摘要：{}", source_task.title),
                content: summary.clone(),
                structured_payload: Some(serde_json::json!({
                    "taskId": source_task.id,
                    "projectId": source_task.project_id,
                    "result": report.result.clone(),
                    "summary": summary,
                    "questions": report.questions.len(),
                    "followUps": report.follow_ups.len(),
                    "risks": report.risks.clone(),
                })),
                source_kind: "task_completion_report",
                source_id: Some(source_task.id.to_string()),
                confidence: Some(0.9),
                created_by: source_task.creator_user_id,
            },
        );
    }

    if let Some(task) = state.tasks.iter_mut().find(|task| task.id == task_id) {
        if let Some(summary) = summary_text.as_deref() {
            task.activities.push(new_activity(
                "task.completion_summary",
                format!("任务完成总结：{summary}"),
            ));
        }
        if auto_created_task_count > 0 {
            task.activities.push(new_activity(
                "task.follow_ups_created",
                format!(
                    "已根据 Agent 建议自动创建 {} 个后续任务",
                    auto_created_task_count
                ),
            ));
        }
        if pending_question_count > 0 {
            task.activities.push(new_activity(
                "task.questions_captured",
                format!("已统一收口 {} 个待回答问题", pending_question_count),
            ));
        }
        if !report.risks.is_empty() {
            for risk in report
                .risks
                .iter()
                .map(|risk| risk.trim())
                .filter(|risk| !risk.is_empty())
            {
                task.activities.push(new_activity(
                    "task.completion_risk",
                    format!("完成后风险提示：{risk}"),
                ));
                risk_count += 1;
            }
        }
    }

    let _ = risk_count;
    record_execution_pattern(state, &source_task, &report);
    true
}

pub(crate) fn process_project_session_completion_outputs(
    state: &mut BoardState,
    session_id: Uuid,
) -> bool {
    let Some(project_session) = state
        .project_sessions
        .iter()
        .find(|session| session.id == session_id)
        .cloned()
    else {
        return false;
    };

    let mode = normalize_project_session_mode(Some(&project_session.mode));
    if !matches!(mode, "planner" | "evaluator") {
        return false;
    }

    let Some(report) = extract_task_completion_report(&project_session.log) else {
        return false;
    };

    let role_label = project_session_mode_label(mode);
    let normalized_questions = report
        .questions
        .iter()
        .filter_map(normalize_completion_question)
        .collect::<Vec<_>>();
    let summary_text = report
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{role_label}已完成一轮输出"));

    crate::write_memory_revision(
        state,
        MemoryWriteSpec {
            scope_kind: "project",
            scope_id: project_session.project_id,
            memory_kind: "project_harness_report",
            stable_key: format!("project_harness_report/{mode}"),
            tag: format!(
                "project/{}/harness/{mode}/latest",
                project_session.project_id
            ),
            title: format!("{role_label}报告：{}", project_session.title),
            content: build_project_session_report_content(
                &project_session,
                role_label,
                &summary_text,
                &normalized_questions,
                &report,
            ),
            structured_payload: Some(serde_json::json!({
                "mode": mode,
                "sessionId": project_session.id,
                "title": project_session.title,
                "result": report.result.clone(),
                "summary": summary_text,
                "questions": normalized_questions.iter().map(|(question, context)| {
                    serde_json::json!({
                        "question": question,
                        "context": context,
                    })
                }).collect::<Vec<_>>(),
                "followUps": report.follow_ups.iter().map(|follow_up| {
                    serde_json::json!({
                        "kind": follow_up.kind.as_deref(),
                        "title": follow_up.title,
                        "description": follow_up.description,
                        "priority": follow_up.priority.as_deref(),
                        "canAutoCreateTask": follow_up.can_auto_create_task,
                        "canAutoApply": follow_up.can_auto_apply,
                    })
                }).collect::<Vec<_>>(),
                "risks": report.risks.clone(),
            })),
            source_kind: "project_session_completion_report",
            source_id: Some(project_session.id.to_string()),
            confidence: Some(0.85),
            created_by: None,
        },
    );

    let mut auto_created_task_count = 0;
    for follow_up in &report.follow_ups {
        if !should_auto_create_follow_up_task(follow_up) {
            continue;
        }

        let title = follow_up.title.trim();
        let description = follow_up.description.trim();
        if title.is_empty() || description.is_empty() {
            continue;
        }

        let composed_description = compose_project_session_follow_up_task_description(
            &project_session,
            role_label,
            follow_up,
        );
        let already_exists = state.tasks.iter().any(|task| {
            task.project_id == project_session.project_id
                && task.title == title
                && task.description == composed_description
        });
        if already_exists {
            continue;
        }

        state.tasks.insert(
            0,
            Task {
                id: Uuid::new_v4(),
                project_id: project_session.project_id,
                title: title.to_string(),
                description: composed_description,
                status: TaskStatus::Open,
                priority: parse_follow_up_priority(follow_up.priority.as_deref()),
                labels: vec![format!("harness:{mode}")],
                creator_user_id: None,
                assignee_user_id: None,
                assignment_mode: platform_core::TaskAssignmentMode::PublicQueue,
                requested_agent_id: None,
                source_task_id: None,
                claimed_by: None,
                activities: vec![new_activity(
                    "task.auto_created_from_project_session",
                    format!(
                        "该任务由{}会话“{}”的结构化建议自动生成",
                        role_label, project_session.title
                    ),
                )],
                runtime: None,
                approval: Default::default(),
                acceptance: Default::default(),
                state_snapshot: TaskStateSnapshot::default(),
            },
        );
        auto_created_task_count += 1;
    }

    let question_message = if normalized_questions.is_empty() {
        None
    } else {
        Some(format!(
            "来自{}会话“{}”（{}）的待确认问题：\n{}",
            role_label,
            project_session.title,
            project_session.id,
            normalized_questions
                .iter()
                .enumerate()
                .map(|(index, (question, context))| {
                    context
                        .as_deref()
                        .map(|context| {
                            format!("{}. {}（上下文：{}）", index + 1, question, context)
                        })
                        .unwrap_or_else(|| format!("{}. {}", index + 1, question))
                })
                .collect::<Vec<_>>()
                .join("\n")
        ))
    };

    let mut posted_question_message = false;
    if let Some(content) = question_message {
        let already_exists = state.project_chat_messages.iter().any(|message| {
            message.project_id == project_session.project_id
                && message.user_display_name == role_label
                && message.content == content
        });
        if !already_exists {
            state.project_chat_messages.push(ProjectChatMessage {
                id: Uuid::new_v4(),
                project_id: project_session.project_id,
                user_id: None,
                user_display_name: role_label.into(),
                content,
                at: crate::timestamp_string(),
            });
            posted_question_message = true;
        }
    }

    if let Some(session) = state
        .project_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.log.push(new_runtime_entry(
            "project.session.completion_processed",
            format!(
                "{}报告已自动沉淀：写入最新记忆，创建 {} 个后续任务，收口 {} 个问题{}。",
                role_label,
                auto_created_task_count,
                normalized_questions.len(),
                if posted_question_message {
                    "并同步到项目聊天室"
                } else {
                    ""
                }
            ),
        ));
    }

    true
}

/// 任务完成后自动沉淀执行经验到记忆层。
///
/// 记录的内容包括：
/// - 执行结果（成功/失败）
/// - 执行时长（从第一条活动到最后一条）
/// - 运行时日志条数
/// - 恢复/重试次数
/// - 成功策略关键词（从 summary 中提取）
/// - 风险和失败原因
///
/// 这些经验供后续同类任务认领前参考，是自学习的基础数据。
fn record_execution_pattern(state: &mut BoardState, task: &Task, report: &TaskCompletionReport) {
    let result = report.result.as_deref().map(str::trim).unwrap_or("unknown");
    let summary = report.summary.as_deref().map(str::trim).unwrap_or("");

    // 计算执行维度
    let activity_count = task.activities.len();
    let runtime_log_count = task.runtime.as_ref().map(|rt| rt.log.len()).unwrap_or(0);
    let recovery_count = task
        .activities
        .iter()
        .filter(|a| {
            matches!(
                a.kind.as_str(),
                "task.watchdog_recovered"
                    | "task.auto_retry_queued"
                    | "task.runtime_session_lost"
                    | "task.auto_resumed"
            )
        })
        .count();
    let had_errors = task
        .runtime
        .as_ref()
        .and_then(|rt| rt.last_error.as_deref())
        .is_some()
        || task.activities.iter().any(|a| a.kind == "runtime.error");

    // 提取任务类型标签（从标题推断）
    let task_type = infer_task_type(&task.title);

    // 从时间戳推算大致执行时长
    let duration_hint = estimate_duration(&task.activities);

    let pattern_content = format!(
        "任务类型：{task_type}\n\
结果：{result}\n\
摘要：{summary}\n\
活动条数：{activity_count}\n\
运行日志条数：{runtime_log_count}\n\
恢复/重试次数：{recovery_count}\n\
是否有错误：{had_errors}\n\
大致时长：{duration_hint}\n\
风险：{risks}\n\
后续任务数：{follow_up_count}",
        risks = if report.risks.is_empty() {
            "无".into()
        } else {
            report.risks.join("; ")
        },
        follow_up_count = report.follow_ups.len(),
    );

    let structured = serde_json::json!({
        "task_type": task_type,
        "result": result,
        "summary": summary,
        "activity_count": activity_count,
        "runtime_log_count": runtime_log_count,
        "recovery_count": recovery_count,
        "had_errors": had_errors,
        "duration_hint": duration_hint,
        "risk_count": report.risks.len(),
        "follow_up_count": report.follow_ups.len(),
    });

    crate::handlers::write_memory_revision(
        state,
        MemoryWriteSpec {
            scope_kind: "project",
            scope_id: task.project_id,
            memory_kind: "execution_pattern",
            stable_key: format!("execution_pattern/{}", task_type),
            tag: format!("project/{}/execution-patterns", task.project_id),
            title: format!("执行模式：{task_type}"),
            content: pattern_content,
            structured_payload: Some(structured),
            source_kind: "task_completion_auto",
            source_id: Some(task.id.to_string()),
            confidence: Some(0.8),
            created_by: task.creator_user_id,
        },
    );
}

/// 从任务标题推断任务类型
fn infer_task_type(title: &str) -> &'static str {
    let lower = title.to_lowercase();
    if lower.contains("编译") || lower.contains("build") || lower.contains("重启") {
        "编译重启"
    } else if lower.contains("探索") || lower.contains("explore") || lower.contains("扫描") {
        "项目探索"
    } else if lower.contains("测试") || lower.contains("test") {
        "测试补充"
    } else if lower.contains("修复") || lower.contains("fix") || lower.contains("bug") {
        "缺陷修复"
    } else if lower.contains("重构") || lower.contains("refactor") || lower.contains("拆分") {
        "重构优化"
    } else if lower.contains("文档") || lower.contains("doc") || lower.contains("readme") {
        "文档更新"
    } else if lower.contains("部署") || lower.contains("deploy") || lower.contains("云端") {
        "部署运维"
    } else if lower.contains("安全") || lower.contains("权限") || lower.contains("审计") {
        "安全审计"
    } else if lower.contains("agent") || lower.contains("运行时") || lower.contains("runtime") {
        "Agent与运行时"
    } else if lower.contains("任务") || lower.contains("状态") || lower.contains("看板") {
        "任务管理"
    } else {
        "通用功能"
    }
}

/// 从活动时间戳估算大致执行时长
fn estimate_duration(activities: &[platform_core::TaskActivity]) -> String {
    if activities.len() < 2 {
        return "未知".into();
    }
    let first = activities.first().and_then(|a| a.at.parse::<u64>().ok());
    let last = activities.last().and_then(|a| a.at.parse::<u64>().ok());
    match (first, last) {
        (Some(start), Some(end)) if end > start => {
            let secs = end - start;
            if secs < 60 {
                format!("{secs} 秒")
            } else if secs < 3600 {
                format!("{} 分钟", secs / 60)
            } else {
                format!("{} 小时 {} 分钟", secs / 3600, (secs % 3600) / 60)
            }
        }
        _ => "未知".into(),
    }
}

pub(crate) fn extract_task_completion_report(
    log: &[RuntimeLogEntry],
) -> Option<TaskCompletionReport> {
    for entry in log.iter().rev() {
        if entry.kind != "assistant" {
            continue;
        }
        if let Some(report) = parse_task_completion_report_text(&entry.message) {
            let has_content = report
                .result
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
                || report
                    .summary
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_some()
                || !report.questions.is_empty()
                || !report.follow_ups.is_empty()
                || !report.risks.is_empty();
            if has_content {
                return Some(report);
            }
        }
    }
    None
}

fn parse_task_completion_report_text(text: &str) -> Option<TaskCompletionReport> {
    for candidate in completion_json_candidates(text) {
        if let Ok(report) = serde_json::from_str::<TaskCompletionReport>(&candidate) {
            return Some(report);
        }
    }
    None
}

fn completion_json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for segment in text.rsplit("```") {
        let trimmed = segment.trim();
        let candidate = if let Some(rest) = trimmed.strip_prefix("json") {
            rest.trim()
        } else {
            trimmed
        };
        if candidate.starts_with('{') && candidate.ends_with('}') && !candidate.is_empty() {
            let owned = candidate.to_string();
            if !candidates.contains(&owned) {
                candidates.push(owned);
            }
        }
    }

    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start < end {
            let candidate = text[start..=end].trim().to_string();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

fn normalize_completion_question(
    question: &TaskCompletionQuestion,
) -> Option<(String, Option<String>)> {
    match question {
        TaskCompletionQuestion::Text(question) => {
            let trimmed = question.trim();
            (!trimmed.is_empty()).then(|| (trimmed.to_string(), None))
        }
        TaskCompletionQuestion::Detailed { question, context } => {
            let trimmed = question.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((
                    trimmed.to_string(),
                    context
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned),
                ))
            }
        }
    }
}

fn build_project_session_report_content(
    project_session: &ProjectSession,
    role_label: &str,
    summary_text: &str,
    normalized_questions: &[(String, Option<String>)],
    report: &TaskCompletionReport,
) -> String {
    let question_block = if normalized_questions.is_empty() {
        "无".into()
    } else {
        normalized_questions
            .iter()
            .enumerate()
            .map(|(index, (question, context))| {
                context
                    .as_deref()
                    .map(|context| format!("{}. {}（上下文：{}）", index + 1, question, context))
                    .unwrap_or_else(|| format!("{}. {}", index + 1, question))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let risk_block = if report.risks.is_empty() {
        "无".into()
    } else {
        report
            .risks
            .iter()
            .filter_map(|risk| {
                let trimmed = risk.trim();
                (!trimmed.is_empty()).then(|| format!("- {trimmed}"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "{}会话：{}\n\
模式：{}\n\
结果：{}\n\
摘要：{}\n\
待确认问题：\n{}\n\
后续建议数：{}\n\
风险提示：\n{}",
        role_label,
        project_session.title,
        normalize_project_session_mode(Some(&project_session.mode)),
        report.result.as_deref().unwrap_or("unknown"),
        summary_text,
        question_block,
        report.follow_ups.len(),
        risk_block
    )
}

fn compose_project_session_follow_up_task_description(
    project_session: &ProjectSession,
    role_label: &str,
    follow_up: &TaskCompletionFollowUp,
) -> String {
    let kind = follow_up
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("follow_up_task");
    let priority = follow_up
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未标注");

    format!(
        "该任务由项目{}会话自动派生。\n\
来源会话：{}\n\
会话 ID：{}\n\
会话模式：{}（{}）\n\
建议类型：{}\n\
建议优先级：{}\n\
\n\
建议说明：\n{}",
        role_label,
        project_session.title,
        project_session.id,
        role_label,
        normalize_project_session_mode(Some(&project_session.mode)),
        kind,
        priority,
        follow_up.description
    )
}

fn should_auto_create_follow_up_task(follow_up: &TaskCompletionFollowUp) -> bool {
    if follow_up.can_auto_apply == Some(true) {
        return false;
    }
    if let Some(can_auto_create_task) = follow_up.can_auto_create_task {
        return can_auto_create_task;
    }

    matches!(
        follow_up.kind.as_deref().map(str::trim).map(str::to_ascii_lowercase),
        Some(kind)
            if matches!(
                kind.as_str(),
                "bug_fix"
                    | "fix"
                    | "defect"
                    | "doc_update"
                    | "documentation"
                    | "docs"
                    | "test_gap"
                    | "test"
                    | "cleanup"
                    | "refactor_followup"
                    | "follow_up_task"
            )
    )
}

fn parse_follow_up_priority(priority: Option<&str>) -> Option<TaskPriority> {
    let priority = priority?.trim();
    if priority.is_empty() {
        return None;
    }

    let priority = priority.to_ascii_uppercase();
    match priority.as_str() {
        "P0" | "P1" | "HIGH" | "URGENT" | "CRITICAL" => Some(TaskPriority::High),
        "P2" | "MEDIUM" | "NORMAL" => Some(TaskPriority::Medium),
        "P3" | "LOW" | "MINOR" => Some(TaskPriority::Low),
        _ => None,
    }
}

fn compose_follow_up_task_description(
    source_task: &Task,
    follow_up: &TaskCompletionFollowUp,
) -> String {
    let kind = follow_up
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("follow_up_task");
    let priority = follow_up
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未标注");

    format!(
        "该任务由已完成任务\u{201c}{}\u{201d}自动派生。\n\
来源任务 ID：{}\n\
建议类型：{}\n\
建议优先级：{}\n\
\n\
建议说明：\n{}",
        source_task.title, source_task.id, kind, priority, follow_up.description
    )
}
