use codex_core_skills::model::SkillMetadata;
use codex_plugin::PluginCapabilitySummary;

use crate::skills_helpers::skill_description;
use crate::skills_helpers::skill_display_name;
use crate::project_agent_workbench::ProjectAgentMentionCatalog;
use crate::project_agent_workbench::ProjectAgentTaskMention;

use super::candidate::Candidate;
use super::candidate::MentionType;
use super::candidate::Selection;

pub(crate) fn build_search_catalog(
    skills: Option<&[SkillMetadata]>,
    plugins: Option<&[PluginCapabilitySummary]>,
    project_agents: Option<&ProjectAgentMentionCatalog>,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Some(project_agents) = project_agents {
        candidates.extend(
            project_agents
                .recent_tasks
                .iter()
                .map(project_agent_task_candidate),
        );
        candidates.extend(
            project_agents
                .agents
                .iter()
                .filter(|agent| agent.enabled)
                .map(project_agent_candidate),
        );
    }
    if let Some(skills) = skills {
        candidates.extend(skills.iter().map(skill_candidate));
    }

    if let Some(plugins) = plugins {
        candidates.extend(plugins.iter().map(plugin_candidate));
    }

    candidates
}

fn project_agent_task_candidate(task: &ProjectAgentTaskMention) -> Candidate {
    Candidate {
        display_name: format!("@{} · {}", task.agent_id, task.task_title),
        description: Some(task.task_id.clone()),
        search_terms: vec![
            task.agent_id.clone(),
            task.task_id.clone(),
            task.task_title.clone(),
            task.session_thread_id.clone(),
        ],
        mention_type: MentionType::ProjectAgentTask,
        selection: Selection::ProjectAgentTask(task.clone()),
    }
}

fn project_agent_candidate(
    agent: &codex_app_server_protocol::ProjectAgentRosterEntry,
) -> Candidate {
    Candidate {
        display_name: format!("@{}", agent.id),
        description: Some(agent.description.clone()),
        search_terms: vec![agent.id.clone(), agent.description.clone()],
        mention_type: MentionType::ProjectAgent,
        selection: Selection::Tool {
            insert_text: format!("@{}", agent.id),
            path: None,
        },
    }
}

fn skill_candidate(skill: &SkillMetadata) -> Candidate {
    let display_name = skill_display_name(skill);
    let description = optional_skill_description(skill);
    let skill_name = skill.name.clone();
    let search_terms = if display_name == skill.name {
        vec![skill_name.clone()]
    } else {
        vec![skill_name.clone(), display_name.clone()]
    };
    Candidate {
        display_name,
        description,
        search_terms,
        mention_type: MentionType::Skill,
        selection: Selection::Tool {
            insert_text: format!("${skill_name}"),
            path: Some(skill.path_to_skills_md.to_string_lossy().into_owned()),
        },
    }
}

fn plugin_candidate(plugin: &PluginCapabilitySummary) -> Candidate {
    let (plugin_name, marketplace_name) = plugin
        .config_name
        .split_once('@')
        .unwrap_or((plugin.config_name.as_str(), ""));
    let mention_name = plugin_mention_name(plugin_name, plugin.display_name.as_str());
    let mut search_terms = vec![plugin_name.to_string(), plugin.config_name.clone()];
    if plugin.display_name != plugin_name {
        search_terms.push(plugin.display_name.clone());
    }
    if !marketplace_name.is_empty() {
        search_terms.push(marketplace_name.to_string());
    }

    Candidate {
        display_name: plugin.display_name.clone(),
        description: plugin_description(plugin),
        search_terms,
        mention_type: MentionType::Plugin,
        selection: Selection::Tool {
            insert_text: format!("@{mention_name}"),
            path: Some(format!("plugin://{}", plugin.config_name)),
        },
    }
}

fn plugin_mention_name(plugin_name: &str, display_name: &str) -> String {
    let plugin_segments = split_plugin_name_segments(plugin_name);
    let display_segments = split_display_name_segments(display_name);

    if plugin_segments.len() == display_segments.len()
        && plugin_segments.iter().zip(&display_segments).all(
            |((plugin_segment, _), display_segment)| {
                plugin_segment.eq_ignore_ascii_case(display_segment.as_str())
            },
        )
    {
        let mut result = String::new();
        for ((_, separator), display_segment) in plugin_segments.into_iter().zip(display_segments) {
            result.push_str(display_segment.as_str());
            if let Some(separator) = separator {
                result.push(separator);
            }
        }
        return result;
    }

    title_case_plugin_name(plugin_name)
}

fn split_plugin_name_segments(plugin_name: &str) -> Vec<(String, Option<char>)> {
    let mut segments = Vec::new();
    let mut current = String::new();

    for ch in plugin_name.chars() {
        if matches!(ch, '-' | '_') {
            if !current.is_empty() {
                segments.push((std::mem::take(&mut current), Some(ch)));
            }
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        segments.push((current, None));
    }

    segments
}

fn split_display_name_segments(display_name: &str) -> Vec<String> {
    display_name
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn title_case_plugin_name(plugin_name: &str) -> String {
    let mut result = String::with_capacity(plugin_name.len());
    let mut capitalize_next = true;

    for ch in plugin_name.chars() {
        if matches!(ch, '-' | '_') {
            capitalize_next = true;
            result.push(ch);
            continue;
        }

        if capitalize_next && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
            capitalize_next = false;
        }
    }

    result
}

fn plugin_description(plugin: &PluginCapabilitySummary) -> Option<String> {
    let capability_labels = plugin_capability_labels(plugin);
    plugin.description.clone().or_else(|| {
        Some(if capability_labels.is_empty() {
            "Plugin".to_string()
        } else {
            format!("Plugin - {}", capability_labels.join(" - "))
        })
    })
}

fn plugin_capability_labels(plugin: &PluginCapabilitySummary) -> Vec<String> {
    let mut labels = Vec::new();
    if plugin.has_skills {
        labels.push("skills".to_string());
    }
    if !plugin.mcp_server_names.is_empty() {
        let mcp_server_count = plugin.mcp_server_names.len();
        labels.push(if mcp_server_count == 1 {
            "1 MCP server".to_string()
        } else {
            format!("{mcp_server_count} MCP servers")
        });
    }
    if !plugin.app_connector_ids.is_empty() {
        let app_count = plugin.app_connector_ids.len();
        labels.push(if app_count == 1 {
            "1 app".to_string()
        } else {
            format!("{app_count} apps")
        });
    }
    labels
}

fn optional_skill_description(skill: &SkillMetadata) -> Option<String> {
    let description = skill_description(skill).trim();
    (!description.is_empty()).then(|| description.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn plugin_mention_name_uses_display_segments_when_they_match_plugin_name() {
        assert_eq!(
            plugin_mention_name("mcp-search", "MCP Search"),
            "MCP-Search"
        );
        assert_eq!(
            plugin_mention_name("google_calendar", "Google Calendar"),
            "Google_Calendar"
        );
    }

    #[test]
    fn plugin_mention_name_falls_back_to_title_cased_plugin_name() {
        assert_eq!(plugin_mention_name("sample", "Sample Plugin"), "Sample");
        assert_eq!(
            plugin_mention_name("browser-use", "Browser Use"),
            "Browser-Use"
        );
    }

    #[test]
    fn project_agent_candidates_are_enabled_mentions() {
        let agents = vec![
            codex_app_server_protocol::ProjectAgentRosterEntry {
                id: "query".to_string(),
                description: "Project query specialist".to_string(),
                enabled: true,
                active_session_thread_id: None,
                session: None,
                current_task: None,
            },
            codex_app_server_protocol::ProjectAgentRosterEntry {
                id: "disabled".to_string(),
                description: "Disabled specialist".to_string(),
                enabled: false,
                active_session_thread_id: None,
                session: None,
                current_task: None,
            },
        ];

        assert_eq!(
            build_search_catalog(
                /*skills*/ None,
                /*plugins*/ None,
                Some(&ProjectAgentMentionCatalog {
                    agents,
                    recent_tasks: Vec::new(),
                }),
            ),
            vec![Candidate {
                display_name: "@query".to_string(),
                description: Some("Project query specialist".to_string()),
                search_terms: vec!["query".to_string(), "Project query specialist".to_string(),],
                mention_type: MentionType::ProjectAgent,
                selection: Selection::Tool {
                    insert_text: "@query".to_string(),
                    path: None,
                },
            }]
        );
    }

    #[test]
    fn recent_project_agent_task_is_first_and_searchable_by_all_identifiers() {
        let session_thread_id = codex_protocol::ThreadId::new().to_string();
        let task = ProjectAgentTaskMention {
            root_thread_id: codex_protocol::ThreadId::new(),
            task_id: "task-auth".to_string(),
            task_title: "Review auth flow".to_string(),
            agent_id: "query".to_string(),
            session_thread_id: session_thread_id.clone(),
        };
        let catalog = ProjectAgentMentionCatalog {
            agents: vec![codex_app_server_protocol::ProjectAgentRosterEntry {
                id: "query".to_string(),
                description: "Project query specialist".to_string(),
                enabled: true,
                active_session_thread_id: None,
                session: None,
                current_task: None,
            }],
            recent_tasks: vec![task.clone()],
        };
        let candidates = build_search_catalog(
            /*skills*/ None,
            /*plugins*/ None,
            Some(&catalog),
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.mention_type)
                .collect::<Vec<_>>(),
            vec![MentionType::ProjectAgentTask, MentionType::ProjectAgent]
        );
        for query in [
            task.agent_id.as_str(),
            task.task_id.as_str(),
            task.task_title.as_str(),
            session_thread_id.as_str(),
        ] {
            let rows = super::super::filter::filtered_candidates(
                &candidates,
                &[],
                query,
                super::super::search_mode::SearchMode::Tools,
                /*show_file_matches*/ false,
            );
            assert!(matches!(
                rows.first().map(|row| &row.selection),
                Some(Selection::ProjectAgentTask(selected)) if selected == &task
            ));
        }
    }
}
