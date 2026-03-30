use platform_core::{Project, Task};
use uuid::Uuid;

use crate::models::*;
use crate::snapshot::project_memory_snapshot;
use crate::{AppResult, AppState, BoardState};

pub(crate) fn prompt_timestamp_key(value: &str) -> u128 {
    value.parse::<u128>().unwrap_or_default()
}

pub(crate) fn prompt_preview(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "无".into();
    }

    let mut chars = compact.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(crate) fn prompt_section_or_default(lines: Vec<String>, empty_message: &str) -> String {
    if lines.is_empty() {
        empty_message.into()
    } else {
        lines.join("\n")
    }
}

pub(crate) fn project_scan_summary_for_prompt(latest_scan: Option<&ProjectScanSummary>) -> String {
    latest_scan
        .map(|scan| {
            format!(
                "工作区：{}（{}）\n技术栈摘要：{}\n顶层目录：{}\n关键文件：{}\n文档文件：{}\n提示：{}",
                scan.workspace_label,
                scan.workspace_path,
                scan.stack_summary,
                if scan.top_level_entries.is_empty() {
                    "无".into()
                } else {
                    scan.top_level_entries.join("、")
                },
                if scan.key_files.is_empty() {
                    "无".into()
                } else {
                    scan.key_files.join("、")
                },
                if scan.document_files.is_empty() {
                    "无".into()
                } else {
                    scan.document_files.join("、")
                },
                if scan.notes.is_empty() {
                    "无".into()
                } else {
                    scan.notes.join("；")
                }
            )
        })
        .unwrap_or_else(|| {
            "最近还没有项目扫描摘要；执行前要先结合目录实际情况判断，不要把旧认知当成当前事实。"
                .into()
        })
}

pub(crate) fn recent_task_activity_lines(task: &Task, limit: usize) -> Vec<String> {
    let mut activities = task.activities.iter().collect::<Vec<_>>();
    activities.sort_by_key(|activity| prompt_timestamp_key(&activity.at));
    let skip = activities.len().saturating_sub(limit);
    activities
        .into_iter()
        .skip(skip)
        .map(|activity| {
            format!(
                "- [{}] {}：{}",
                activity.at,
                activity.kind,
                prompt_preview(&activity.message, 120)
            )
        })
        .collect()
}

pub(crate) fn recent_task_runtime_lines(task: &Task, limit: usize) -> Vec<String> {
    let Some(runtime) = task.runtime.as_ref() else {
        return Vec::new();
    };

    let mut entries = runtime.log.iter().collect::<Vec<_>>();
    entries.sort_by_key(|entry| prompt_timestamp_key(&entry.at));
    let skip = entries.len().saturating_sub(limit);
    entries
        .into_iter()
        .skip(skip)
        .map(|entry| {
            format!(
                "- [{}] {}：{}",
                entry.at,
                entry.kind,
                prompt_preview(&entry.message, 120)
            )
        })
        .collect()
}

pub(crate) fn recent_project_chat_messages(
    messages: &[ProjectChatMessage],
    project_id: Uuid,
    limit: usize,
) -> Vec<ProjectChatMessage> {
    let mut relevant = messages
        .iter()
        .filter(|message| message.project_id == project_id)
        .cloned()
        .collect::<Vec<_>>();
    relevant.sort_by_key(|message| prompt_timestamp_key(&message.at));
    let skip = relevant.len().saturating_sub(limit);
    relevant.into_iter().skip(skip).collect()
}

pub(crate) fn recent_project_chat_lines(messages: &[ProjectChatMessage]) -> Vec<String> {
    messages
        .iter()
        .map(|message| {
            format!(
                "- [{}] {}：{}",
                message.at,
                message.user_display_name,
                prompt_preview(&message.content, 120)
            )
        })
        .collect()
}

pub(crate) fn contains_scope_change_signal(text: &str) -> bool {
    let lowered = text.to_lowercase();
    [
        "取消",
        "撤销",
        "不做",
        "先不做",
        "去掉",
        "移除",
        "删除",
        "关闭",
        "废弃",
        "放弃",
        "不用做",
        "不要做",
        "不需要",
        "终止",
        "搁置",
        "撤回",
        "去除",
        "cancel",
        "drop",
        "remove",
        "skip",
        "disable",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

pub(crate) fn recent_scope_signal_lines(
    task: &Task,
    project_chat_messages: &[ProjectChatMessage],
) -> Vec<String> {
    let mut signals = task
        .activities
        .iter()
        .filter(|activity| {
            activity.kind == "task.canceled" || contains_scope_change_signal(&activity.message)
        })
        .map(|activity| {
            (
                prompt_timestamp_key(&activity.at),
                format!(
                    "- [任务活动 {}] {}：{}",
                    activity.at,
                    activity.kind,
                    prompt_preview(&activity.message, 120)
                ),
            )
        })
        .collect::<Vec<_>>();
    signals.extend(project_chat_messages.iter().filter_map(|message| {
        contains_scope_change_signal(&message.content).then(|| {
            (
                prompt_timestamp_key(&message.at),
                format!(
                    "- [项目聊天室 {}] {}：{}",
                    message.at,
                    message.user_display_name,
                    prompt_preview(&message.content, 120)
                ),
            )
        })
    }));
    signals.sort_by_key(|(at, _)| *at);
    let skip = signals.len().saturating_sub(6);
    signals
        .into_iter()
        .skip(skip)
        .map(|(_, line)| line)
        .collect()
}

pub(crate) fn active_project_constraint_lines(state: &BoardState, project_id: Uuid) -> Vec<String> {
    let tag_name = format!("project/{project_id}/active-constraints");
    let mut lines = state
        .memory_items
        .iter()
        .filter(|item| {
            item.scope_kind == "project"
                && item.scope_id == project_id
                && item.memory_kind == "project_constraint"
        })
        .filter_map(|item| {
            let tag = state
                .memory_tags
                .iter()
                .find(|tag| tag.memory_item_id == item.id && tag.tag == tag_name)?;
            let revision = state
                .memory_revisions
                .iter()
                .find(|revision| revision.id == tag.target_revision_id)?;
            Some(format!(
                "- {}：{}",
                revision.title,
                prompt_preview(&revision.content, 120)
            ))
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines
}

pub(crate) fn recent_project_task_summary_lines(
    state: &BoardState,
    project_id: Uuid,
    limit: usize,
) -> Vec<String> {
    let project_tasks = state
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .collect::<Vec<_>>();
    let memory = project_memory_snapshot(state, project_id);
    crate::snapshot::recent_task_summary_digests(&memory, &project_tasks, limit)
        .into_iter()
        .map(|entry| {
            format!(
                "- {}：{}",
                entry.task_title,
                prompt_preview(&entry.summary, 120)
            )
        })
        .collect()
}

pub(crate) fn open_pending_question_lines(
    state: &BoardState,
    project_id: Uuid,
    limit: usize,
) -> Vec<String> {
    let mut questions = state
        .pending_questions
        .iter()
        .filter(|question| question.project_id == project_id && question.status != "answered")
        .collect::<Vec<_>>();
    questions.sort_by_key(|question| prompt_timestamp_key(&question.created_at));
    let skip = questions.len().saturating_sub(limit);
    questions
        .into_iter()
        .skip(skip)
        .map(|question| {
            format!(
                "- {}：{}",
                question.source_task_title,
                prompt_preview(&question.question, 120)
            )
        })
        .collect()
}

fn compose_task_context_snapshot(
    task: &Task,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    project_constraint_lines: &[String],
    recent_task_summary_lines: &[String],
    pending_question_lines: &[String],
    project_chat_messages: &[ProjectChatMessage],
    recent_activity_lines: &[String],
    recent_runtime_lines: &[String],
    scope_signal_lines: &[String],
) -> String {
    let priority = task
        .priority
        .map(|priority| priority.as_str())
        .unwrap_or("UNSET");
    let snapshot = serde_json::json!({
        "project": {
            "name": project.name,
            "description": prompt_preview(&project.description, 160),
            "workspace_roots": project.workspace_roots.iter().map(|workspace| serde_json::json!({
                "label": workspace.label,
                "path": workspace.path,
                "writable": workspace.writable,
            })).collect::<Vec<_>>(),
        },
        "task": {
            "title": task.title,
            "description": prompt_preview(&task.description, 240),
            "status": task.status.as_str(),
            "priority": priority,
            "labels": task.labels,
            "thread_id": task.runtime.as_ref().and_then(|runtime| runtime.thread_id.as_deref()),
            "last_error": task.runtime.as_ref().and_then(|runtime| runtime.last_error.as_deref()),
        },
        "scan": latest_scan.map(|scan| serde_json::json!({
            "stack_summary": scan.stack_summary,
            "top_level_entries": scan.top_level_entries,
            "key_files": scan.key_files,
            "document_files": scan.document_files,
            "notes": scan.notes,
        })),
        "constraints": project_constraint_lines,
        "recent_task_summaries": recent_task_summary_lines,
        "pending_questions": pending_question_lines,
        "scope_signals": scope_signal_lines,
        "recent_activity": recent_activity_lines,
        "recent_runtime": recent_runtime_lines,
        "recent_project_chat": project_chat_messages
            .iter()
            .map(|message| {
                format!(
                    "[{}] {}: {}",
                    message.at,
                    message.user_display_name,
                    prompt_preview(&message.content, 120)
                )
            })
            .collect::<Vec<_>>(),
    });

    serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".into())
}

#[allow(dead_code)]
pub(crate) fn compose_task_prompt(
    task: &Task,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    project_constraint_lines: &[String],
    recent_task_summary_lines: &[String],
    pending_question_lines: &[String],
    project_chat_messages: &[ProjectChatMessage],
    prompt_override: Option<String>,
) -> String {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| {
            format!(
                "- {}: {}（{}）",
                workspace.label,
                workspace.path,
                if workspace.writable {
                    "可写"
                } else {
                    "只读"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let scan_summary = project_scan_summary_for_prompt(latest_scan);
    let recent_activity_lines = recent_task_activity_lines(task, 8);
    let recent_activity_summary =
        prompt_section_or_default(recent_activity_lines.clone(), "最近还没有任务活动记录。");
    let _recent_runtime_lines = recent_task_runtime_lines(task, 6);
    let recent_runtime_summary =
        prompt_section_or_default(recent_task_runtime_lines(task, 6), "最近还没有运行输出。");
    let recent_chat_summary = prompt_section_or_default(
        recent_project_chat_lines(project_chat_messages),
        "最近还没有项目聊天室消息。",
    );
    let project_constraint_summary = prompt_section_or_default(
        project_constraint_lines.to_vec(),
        "当前还没有沉淀到记忆层的项目长期约束；若最近聊天或任务活动出现明确约束，执行时仍要优先遵守。",
    );
    let recent_task_summary = prompt_section_or_default(
        recent_task_summary_lines.to_vec(),
        "当前还没有沉淀到记忆层的最近任务摘要。",
    );
    let pending_question_summary = prompt_section_or_default(
        pending_question_lines.to_vec(),
        "当前没有未回答的项目问题。",
    );
    let scope_signal_lines = recent_scope_signal_lines(task, project_chat_messages);
    let scope_signal_summary = if scope_signal_lines.is_empty() {
        "最近未检测到明确的撤销/不做信号；但执行前仍要核对最新活动、运行输出和项目聊天，不要机械照搬旧描述。".into()
    } else {
        format!(
            "最近检测到以下范围收缩或取消信号，请优先遵守这些更近的明确决策，不要继续实现对应子需求：\n{}",
            scope_signal_lines.join("\n")
        )
    };

    let mut prompt = format!(
        "你正在执行 Spotlight 项目任务。\n\
项目名称：{}\n\
项目说明：{}\n\
工作目录：\n{}\n\
任务标题：{}\n\
任务描述：{}\n\
\n\
最近扫描摘要：\n{}\n\
\n\
最近任务活动：\n{}\n\
\n\
最近运行输出：\n{}\n\
\n\
当前有效项目约束：\n{}\n\
\n\
最近项目聊天室：\n{}\n\
\n\
范围提醒：\n{}\n\
\n\
执行要求：\n\
1. 先分析再行动，给出清晰的执行步骤。\n\
2. 不要假设当前目录一定是代码仓库；它可能为空，也可能只有 Word、PDF、表格、图片或其他资料。\n\
3. 如果遇到 Office 或二进制文件，不要臆造内容，可以基于文件名、目录结构、相邻文本和可读元数据给出判断。\n\
4. 修改前要先核对上面的最近活动、最近运行输出和项目聊天室，避免重复劳动或继续做过期需求。\n\
5. 对\u{201c}当前有效项目约束\u{201d}要视为跨会话仍然有效的长期规则，除非有更新、更明确的近因决策覆盖它。\n\
6. 如果最近活动、运行输出或项目聊天表明某个子需求已撤销、先不做、去掉或删除，必须将其视为当前范围外，并在结论里说明你如何收敛范围。\n\
7. 如果任务标题或旧描述与最近明确决策冲突，以时间更近、表达更明确的决策为准；必要时先做最小安全收口，再提出后续任务。\n\
8. 项目外目录允许读取，但不要做破坏性修改。\n\
9. 输出时尽量用中文，结论、风险和建议都要清楚可读。\n\
10. 任务结束时，请在最后附加一个 ```json 代码块，字段至少包含 result、summary、questions、follow_ups、risks；如果没有内容也要给空数组。",
        project.name,
        project.description,
        workspace_list,
        task.title,
        task.description,
        scan_summary,
        recent_activity_summary,
        recent_runtime_summary,
        project_constraint_summary,
        recent_chat_summary,
        scope_signal_summary
    );

    prompt.push_str("\n\n最近任务摘要：\n");
    prompt.push_str(&recent_task_summary);
    prompt.push_str("\n\n仍待回答的项目问题：\n");
    prompt.push_str(&pending_question_summary);

    if let Some(extra_prompt) = prompt_override {
        let extra_prompt = extra_prompt.trim();
        if !extra_prompt.is_empty() {
            prompt.push_str("\n\n用户补充提示词：\n");
            prompt.push_str(extra_prompt);
        }
    }

    prompt
}

pub(crate) fn compose_task_prompt_with_snapshot(
    task: &Task,
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    project_constraint_lines: &[String],
    recent_task_summary_lines: &[String],
    pending_question_lines: &[String],
    project_chat_messages: &[ProjectChatMessage],
    prompt_override: Option<String>,
) -> String {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| {
            format!(
                "- {}: {}（{}）",
                workspace.label,
                workspace.path,
                if workspace.writable {
                    "可写"
                } else {
                    "只读"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let scan_summary = project_scan_summary_for_prompt(latest_scan);
    let recent_activity_lines = recent_task_activity_lines(task, 8);
    let recent_activity_summary =
        prompt_section_or_default(recent_activity_lines.clone(), "最近还没有任务活动记录。");
    let recent_runtime_lines = recent_task_runtime_lines(task, 6);
    let recent_runtime_summary =
        prompt_section_or_default(recent_runtime_lines.clone(), "最近还没有运行输出。");
    let recent_chat_summary = prompt_section_or_default(
        recent_project_chat_lines(project_chat_messages),
        "最近还没有项目聊天室消息。",
    );
    let project_constraint_summary = prompt_section_or_default(
        project_constraint_lines.to_vec(),
        "当前还没有沉淀到记忆层的项目长期约束；若最近聊天或任务活动出现明确约束，执行时仍要优先遵守。",
    );
    let recent_task_summary = prompt_section_or_default(
        recent_task_summary_lines.to_vec(),
        "当前还没有沉淀到记忆层的最近任务摘要。",
    );
    let pending_question_summary = prompt_section_or_default(
        pending_question_lines.to_vec(),
        "当前没有未回答的项目问题。",
    );
    let scope_signal_lines = recent_scope_signal_lines(task, project_chat_messages);
    let scope_signal_summary = if scope_signal_lines.is_empty() {
        "最近未检测到明确的撤销/不做信号；但执行前仍要核对最新活动、运行输出和项目聊天，不要机械照搬旧描述。".into()
    } else {
        format!(
            "最近检测到以下范围收缩或取消信号，请优先遵守这些更近的明确决策，不要继续实现对应子需求：\n{}",
            scope_signal_lines.join("\n")
        )
    };
    let context_snapshot = compose_task_context_snapshot(
        task,
        project,
        latest_scan,
        project_constraint_lines,
        recent_task_summary_lines,
        pending_question_lines,
        project_chat_messages,
        &recent_activity_lines,
        &recent_runtime_lines,
        &scope_signal_lines,
    );

    let mut prompt = format!(
        "你正在执行 Spotlight 项目任务。\n\
项目名称：{}\n\
项目说明：{}\n\
工作目录：\n{}\n\
任务标题：{}\n\
任务描述：{}\n\
\n\
最近扫描摘要：\n{}\n\
\n\
最近任务活动：\n{}\n\
\n\
最近运行输出：\n{}\n\
\n\
当前有效项目约束：\n{}\n\
\n\
最近项目聊天室：\n{}\n\
\n\
范围提醒：\n{}\n\
\n\
执行要求：\n\
1. 先分析再行动，给出清晰的执行步骤。\n\
2. 不要假设当前目录一定是代码仓库；它可能为空，也可能只有 Word、PDF、表格、图片或其他资料。\n\
3. 如果遇到 Office 或二进制文件，不要臆造内容，可以基于文件名、目录结构、相邻文本和可读元数据给出判断。\n\
4. 修改前要先核对上面的最近活动、最近运行输出和项目聊天室，避免重复劳动或继续做过期需求。\n\
5. 对“当前有效项目约束”要视为跨会话仍然有效的长期规则，除非有更新、更明确的近因决策覆盖它。\n\
6. 如果最近活动、运行输出或项目聊天表明某个子需求已撤销、先不做、去掉或删除，必须将其视为当前范围外，并在结论里说明你如何收敛范围。\n\
7. 如果任务标题或旧描述与最近明确决策冲突，以时间更近、表达更明确的决策为准；必要时先做最小安全收口，再提出后续任务。\n\
8. 项目外目录允许读取，但不要做破坏性修改。\n\
9. 输出时尽量用中文，结论、风险和建议都要清楚可读。\n\
10. 任务结束时，请在最后附加一个 ```json 代码块，字段至少包含 result、summary、questions、follow_ups、risks；如果没有内容也要给空数组。",
        project.name,
        project.description,
        workspace_list,
        task.title,
        task.description,
        scan_summary,
        recent_activity_summary,
        recent_runtime_summary,
        project_constraint_summary,
        recent_chat_summary,
        scope_signal_summary
    );

    prompt.push_str("\n\n最近任务摘要：\n");
    prompt.push_str(&recent_task_summary);
    prompt.push_str("\n\n仍待回答的项目问题：\n");
    prompt.push_str(&pending_question_summary);
    prompt.push_str("\n\n任务上下文快照（机器可读）：\n");
    prompt.push_str(&context_snapshot);

    if let Some(extra_prompt) = prompt_override {
        let extra_prompt = extra_prompt.trim();
        if !extra_prompt.is_empty() {
            prompt.push_str("\n\n用户补充提示词：\n");
            prompt.push_str(extra_prompt);
        }
    }

    prompt
}

pub(crate) fn compose_project_session_prompt_for_mode(
    project: &Project,
    latest_scan: Option<&ProjectScanSummary>,
    user_prompt: &str,
    mode: &str,
) -> String {
    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|workspace| {
            format!(
                "- {}: {}（{}）",
                workspace.label,
                workspace.path,
                if workspace.writable {
                    "可写"
                } else {
                    "只读"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let scan_summary = latest_scan
        .map(|scan| {
            format!(
                "最近扫描摘要：{}\n顶层目录：{}\n关键文件：{}\n文档文件：{}\n提示：{}",
                scan.stack_summary,
                if scan.top_level_entries.is_empty() {
                    "无".into()
                } else {
                    scan.top_level_entries.join("、")
                },
                if scan.key_files.is_empty() {
                    "无".into()
                } else {
                    scan.key_files.join("、")
                },
                if scan.document_files.is_empty() {
                    "无".into()
                } else {
                    scan.document_files.join("、")
                },
                if scan.notes.is_empty() {
                    "无".into()
                } else {
                    scan.notes.join("；")
                }
            )
        })
        .unwrap_or_else(|| "最近还没有项目扫描摘要；如果需要判断目录结构、文档和构建入口，请先建议用户执行项目扫描。".into());

    let mode = normalize_project_session_mode(Some(mode));
    match mode {
        "planner" => format!(
            "你正在进行 Spotlight 的项目规划器会话，不直接执行代码任务。\n\
项目名称：{}\n\
项目说明：{}\n\
可见工作目录：\n{}\n\
{}\n\
\n\
当前用户目标：{}\n\
\n\
你的职责：\n\
1. 把用户的目标扩写成可执行的产品/工程规格。\n\
2. 明确建议的里程碑、验收标准、关键依赖和评估维度。\n\
3. 如果适合引入 planner / generator / evaluator 分工，请直接写清楚分工方式。\n\
4. 只做规划，不要假装已经完成实现。\n\
\n\
输出要求：\n\
1. 优先用中文回答。\n\
2. 先基于目录结构、代码和文档做判断，不要臆造未读取到的内容。\n\
3. 规划要突出当前最小可用切片，避免大而全重写。\n\
4. 明确哪些部分是已知事实，哪些部分是推断或待验证项。\n\
5. 最后必须附一个 ```json 代码块，字段至少包含 result、summary、questions、follow_ups、risks。\n\
6. follow_ups 里的每一项都尽量包含 kind、title、description、priority、can_auto_create_task、can_auto_apply。",
            project.name, project.description, workspace_list, scan_summary, user_prompt
        ),
        "evaluator" => format!(
            "你正在进行 Spotlight 的项目评估器会话，职责是扮演外部 QA / skeptical reviewer。\n\
项目名称：{}\n\
项目说明：{}\n\
可见工作目录：\n{}\n\
{}\n\
\n\
当前评估目标：{}\n\
\n\
你的职责：\n\
1. 重点寻找功能缺口、实现漏洞、质量风险和验收不充分之处。\n\
2. 不要替生成器辩护；如果发现问题，要直接指出影响和修复方向。\n\
3. 尽量区分阻塞问题、重要问题和次要问题。\n\
4. 优先给出可执行的修复建议或后续任务，而不是泛泛而谈。\n\
\n\
输出要求：\n\
1. 优先用中文回答。\n\
2. 先基于目录结构、代码、文档和现有行为做判断，不要臆造未读取到的内容。\n\
3. 结论里要明确：当前是否可继续推进、哪里还不达标、为什么。\n\
4. 最后必须附一个 ```json 代码块，字段至少包含 result、summary、questions、follow_ups、risks。\n\
5. follow_ups 里的每一项都尽量包含 kind、title、description、priority、can_auto_create_task、can_auto_apply。",
            project.name, project.description, workspace_list, scan_summary, user_prompt
        ),
        _ => format!(
            "你正在进行 Spotlight 的项目级问答会话，而不是直接执行一个任务。\n\
项目名称：{}\n\
项目说明：{}\n\
可见工作目录：\n{}\n\
{}\n\
\n\
当前用户问题：{}\n\
\n\
回答要求：\n\
1. 优先用中文回答。\n\
2. 先基于目录结构、代码和文档做判断，不要臆造未读取到的内容。\n\
3. 如果信息不足，要明确指出缺口，并给出下一步建议。\n\
4. 如果适合拆成任务，请顺手给出建议任务标题和说明。\n\
5. 若涉及实际改动，先说明影响范围和风险，再给方案。",
            project.name, project.description, workspace_list, scan_summary, user_prompt
        ),
    }
}

pub(crate) fn project_session_developer_instructions_for_mode(mode: &str) -> String {
    match normalize_project_session_mode(Some(mode)) {
        "planner" => [
            "你是 Spotlight 的项目规划器。",
            "你的职责是把简短目标扩写成可执行规格、里程碑、验收标准和下一批任务建议。",
            "默认优先做范围澄清、依赖梳理、实现切片和评估维度设计，不直接假装完成实现。",
            "如果信息不足，要明确指出事实缺口和假设边界。",
            "回答尽量用中文，结构清晰，最后附结构化 JSON 代码块。",
        ]
        .join(" "),
        "evaluator" => [
            "你是 Spotlight 的项目评估器。",
            "你的职责是扮演外部 QA / skeptical reviewer，主动寻找功能缺口、回归风险和质量问题。",
            "不要替实现辩护；如果发现问题，要说清影响、证据和修复方向。",
            "回答尽量用中文，结论要明确可执行，最后附结构化 JSON 代码块。",
        ]
        .join(" "),
        _ => [
            "你是 Spotlight 的项目协作 Agent。",
            "当前会话的目标是帮助用户理解项目目录、文档、代码结构和下一步改动方向。",
            "默认优先做分析、解释、风险提示和任务拆解，而不是直接执行破坏性修改。",
            "当需要代码改动时，要先说清楚影响范围、依赖条件和建议步骤。",
            "回答尽量用中文，结构清晰，可读性高。",
        ]
        .join(" "),
    }
}

pub(crate) fn task_developer_instructions() -> String {
    [
        "你是 Spotlight 的本地工程 Agent。",
        "你需要在当前工作目录内完成软件任务，并保持结果可回顾。",
        "优先输出清晰的计划、执行过程、命令结果、风险判断和最终结论。",
        "如果用户暂停后补充提示词，要在同一线程里继续推进，不要丢失上下文。",
        "执行前必须综合最近任务活动、最近运行输出、项目聊天室和目录扫描摘要；如果出现撤销、不做、去掉、删除、放弃等新决策，要按最新决策收缩范围，不要继续实现已取消子需求。",
        "平台会在任务启动前自动从主分支切出任务分支，并在任务完成后尝试按门禁规则合并回主分支。",
        "除非任务明确要求，不要自行执行危险 Git 历史改写，也不要跳过测试就声称可以合并。",
        "任务完成时，请在回复末尾附加一个可机读的 JSON 代码块，例如包含 result、summary、questions、follow_ups、risks。",
        "questions 用于放仍需用户回答的澄清问题；follow_ups 用于放建议自动生成的后续任务，字段建议包含 kind、title、description、priority、can_auto_create_task、can_auto_apply。",
    ]
    .join(" ")
}

pub(crate) async fn resolve_task_execution_context(
    state: &AppState,
    task_id: Uuid,
    prompt_override: Option<String>,
) -> AppResult<TaskExecutionContext> {
    let guard = state.inner.lock().await;
    let task = guard
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .cloned()
        .ok_or_else(|| (axum::http::StatusCode::NOT_FOUND, "未找到任务".into()))?;
    let project = crate::task_ops::find_project(&guard, task.project_id)?.clone();
    let workspace_root = crate::task_ops::primary_workspace_path(&project)?;
    let latest_scan = guard.project_scans.get(&task.project_id).cloned();
    let project_constraints = active_project_constraint_lines(&guard, task.project_id);
    let recent_task_summaries = recent_project_task_summary_lines(&guard, task.project_id, 4);
    let pending_question_lines_vec = open_pending_question_lines(&guard, task.project_id, 4);
    let recent_project_chat =
        recent_project_chat_messages(&guard.project_chat_messages, task.project_id, 8);
    let prompt = compose_task_prompt_with_snapshot(
        &task,
        &project,
        latest_scan.as_ref(),
        &project_constraints,
        &recent_task_summaries,
        &pending_question_lines_vec,
        &recent_project_chat,
        prompt_override,
    );
    Ok(TaskExecutionContext {
        workspace_root,
        prompt,
    })
}

/// 构造任务重新评估提示词。
///
/// 核心设计：不只看任务自身历史，而是让 Agent 综合评估**当前代码库的真实状态**。
/// 在多 Agent 分布式协作下，一个任务可能被 A 做了一半、B 接手、C 做了类似工作并完成——
/// 因此 Agent 必须先检查工作区现状，再决定任务是否还有继续的必要。
pub(crate) fn compose_task_reassess_prompt(
    task: &Task,
    project: &Project,
    sibling_tasks: &[&Task],
    latest_scan: Option<&ProjectScanSummary>,
    project_constraints: &[String],
    recent_chat: &[ProjectChatMessage],
) -> String {
        let status_label = crate::state::task_status_label(task.status);

    let recent_activities = recent_task_activity_lines(task, 12);
    let activity_section = prompt_section_or_default(recent_activities, "没有任务活动记录。");

    let recent_runtime = recent_task_runtime_lines(task, 10);
    let runtime_section = prompt_section_or_default(recent_runtime, "没有运行时输出。");

    let last_error = task
        .runtime
        .as_ref()
        .and_then(|rt| rt.last_error.as_deref())
        .unwrap_or("无");

    let has_thread = task
        .runtime
        .as_ref()
        .and_then(|rt| rt.thread_id.as_deref())
        .is_some();

    let snapshot_reason = task
        .state_snapshot
        .reason
        .as_deref()
        .unwrap_or("无状态快照");

    // 同项目下其他任务的摘要——这是分布式评估的关键输入
    let mut sibling_lines = Vec::new();
    for sibling in sibling_tasks {
        if sibling.id == task.id {
            continue;
        }
            let sib_status = crate::state::task_status_label(sibling.status);
        let sib_summary = sibling.state_snapshot.reason.as_deref().unwrap_or("");
        let overlap_hint = if titles_overlap(&task.title, &sibling.title) {
            " [标题相近，可能重叠]"
        } else {
            ""
        };
        sibling_lines.push(format!(
            "- [{sib_status}] {}{overlap_hint}{}",
            sibling.title,
            if sib_summary.is_empty() {
                String::new()
            } else {
                format!(" \u{2014} {}", prompt_preview(sib_summary, 80))
            },
        ));
    }
    let sibling_section = if sibling_lines.is_empty() {
        "当前项目下没有其他任务。".into()
    } else {
        sibling_lines.join("\n")
    };

    let scan_section = crate::prompt::project_scan_summary_for_prompt(latest_scan);

    let constraint_section =
        prompt_section_or_default(project_constraints.to_vec(), "无项目约束。");

    let chat_section =
        prompt_section_or_default(recent_project_chat_lines(recent_chat), "最近没有项目聊天。");

    let scope_signals = recent_scope_signal_lines(task, recent_chat);
    let scope_section = if scope_signals.is_empty() {
        "未检测到撤销/不做信号。".into()
    } else {
        scope_signals.join("\n")
    };

    let workspace_list = project
        .workspace_roots
        .iter()
        .map(|ws| {
            format!(
                "- {}: {}（{}）",
                ws.label,
                ws.path,
                if ws.writable { "可写" } else { "只读" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "你是 Spotlight 的任务治理 Agent。\n\
你的职责不是执行任务，而是**评估一个任务在当前项目现状下是否还需要继续**。\n\
\n\
## 核心原则\n\
\n\
在多 Agent 分布式协作中，同一个任务可能被不同 Agent 先后接手，\n\
其他 Agent 可能已经通过另一个任务完成了相同或类似的工作。\n\
因此你不能只看这个任务自身的历史，必须：\n\
\n\
1. **先检查当前代码库**：读取工作目录中的实际文件、git log、测试结果，判断任务描述的工作是否已经体现在代码里\n\
2. **再对照同项目其他任务**：看看有没有已完成的任务覆盖了相同范围\n\
3. **最后综合任务自身状态**：结合运行日志、活动记录和错误信息做最终判断\n\
\n\
## 待评估任务\n\
\n\
- 项目：{project_name}\n\
- 标题：{title}\n\
- 当前状态：{status}\n\
- 描述：{description}\n\
\n\
## 当前工作目录\n\
{workspace_list}\n\
\n\
## 最近项目扫描\n\
{scan_summary}\n\
\n\
## 运行时上下文\n\
- 是否保留可恢复 thread：{has_thread}\n\
- 最后错误：{last_error}\n\
- 状态快照：{snapshot_reason}\n\
\n\
## 该任务的最近活动\n\
{activities}\n\
\n\
## 该任务的最近运行输出\n\
{runtime}\n\
\n\
## 同项目其他任务（关键上下文）\n\
{siblings}\n\
\n\
## 项目约束\n\
{constraints}\n\
\n\
## 最近项目聊天\n\
{chat}\n\
\n\
## 范围变更信号\n\
{scope_signals}\n\
\n\
## 评估步骤（必须按顺序执行）\n\
\n\
### 第一步：检查代码库现状\n\
请在工作目录中实际检查：\n\
- `git log --oneline -20` 查看最近提交，是否有与本任务相关的改动\n\
- 检查任务描述中提到的文件/功能是否已经存在且正确\n\
- 如果有测试要求，跑一下看是否通过\n\
- 对照任务描述的交付清单，逐项核对\n\
\n\
### 第二步：对照其他任务\n\
看上面\u{201c}同项目其他任务\u{201d}列表中标记为 [\u{5df2}\u{5b8c}\u{6210}] 的任务，\n\
判断是否有任务已经覆盖了当前任务的全部或部分范围。\n\
\n\
### 第三步：做出判断\n\
\n\
| 判定 | 条件 |\n\
|------|------|\n\
| **DONE** | 代码库中已经体现了任务目标的全部成果（不管是谁完成的） |\n\
| **PARTIAL_DONE** | 代码库中已体现部分成果，剩余部分价值不大或可拆新任务 |\n\
| **CANCELED** | 任务目标已被明确取消、废弃，或被另一个更好的方案替代 |\n\
| **RESTART** | 任务目标仍然有效，代码库中尚未体现，有可恢复的 thread |\n\
| **REOPEN** | 任务目标仍然有效，但运行上下文已丢失，需从头执行 |\n\
| **MANUAL_REVIEW** | 证据矛盾或不足，需要人工决定 |\n\
\n\
## 输出格式\n\
\n\
先给出你的检查过程和推理（用中文），然后在末尾附加：\n\
```json\n\
{{\n\
  \"decision\": \"DONE | PARTIAL_DONE | CANCELED | RESTART | REOPEN | MANUAL_REVIEW\",\n\
  \"confidence\": 0.0 到 1.0,\n\
  \"reason\": \"一句话判断依据\",\n\
  \"code_evidence\": [\"检查到的关键代码/文件证据\"],\n\
  \"overlapping_tasks\": [\"与本任务工作重叠的已完成任务标题\"],\n\
  \"remaining_work\": \"如果是 PARTIAL_DONE，说明剩余部分；否则为 null\",\n\
  \"resume_hint\": \"如果是 RESTART，给出恢复提示词；否则为 null\",\n\
  \"suggested_follow_ups\": [\"如果需要拆出新任务，给出标题建议\"]\n\
}}\n\
```",
        project_name = project.name,
        title = task.title,
        status = status_label,
        description = prompt_preview(&task.description, 500),
        workspace_list = workspace_list,
        scan_summary = scan_section,
        has_thread = if has_thread { "是" } else { "否" },
        last_error = last_error,
        snapshot_reason = snapshot_reason,
        activities = activity_section,
        runtime = runtime_section,
        siblings = sibling_section,
        constraints = constraint_section,
        chat = chat_section,
        scope_signals = scope_section,
    )
}

/// 简单判断两个任务标题是否可能有工作重叠
fn titles_overlap(a: &str, b: &str) -> bool {
    // 提取版本号前缀 [X.Y.Z]，如果在同一个版本段内则可能重叠
    let va = extract_version_prefix(a);
    let vb = extract_version_prefix(b);
    if let (Some(va), Some(vb)) = (va.as_deref(), vb.as_deref()) {
        if va == vb {
            return true;
        }
        // 同一个 minor 版本 (0.1.x)
        let pa: Vec<&str> = va.splitn(3, '.').collect();
        let pb: Vec<&str> = vb.splitn(3, '.').collect();
        if pa.len() >= 2 && pb.len() >= 2 && pa[0] == pb[0] && pa[1] == pb[1] {
            return true;
        }
    }

    // 关键词重叠检测
    let keywords_a = significant_keywords(a);
    let keywords_b = significant_keywords(b);
    let overlap = keywords_a.intersection(&keywords_b).count();
    overlap >= 2
}

fn extract_version_prefix(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let end = trimmed.find(']')?;
    Some(trimmed[1..end].to_string())
}

fn significant_keywords(title: &str) -> std::collections::HashSet<String> {
    let stop_words = [
        "的", "与", "和", "或", "是", "在", "了", "把", "被", "对", "the", "a", "an", "and", "or",
        "in", "on", "for", "to", "of",
    ];
    title
        .split(|c: char| {
            c.is_whitespace() || c == '/' || c == '、' || c == '，' || c == '[' || c == ']'
        })
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.len() >= 2 && !stop_words.contains(&w.as_str()))
        .collect()
}
