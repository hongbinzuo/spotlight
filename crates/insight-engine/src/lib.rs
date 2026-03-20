use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsightScenario {
    pub key: String,
    pub default_model_class: String,
    pub cache_ttl_minutes: u32,
}

pub fn starter_scenarios() -> Vec<InsightScenario> {
    vec![
        InsightScenario {
            key: "daily_summary".into(),
            default_model_class: "medium".into(),
            cache_ttl_minutes: 60,
        },
        InsightScenario {
            key: "build_failure_explainer".into(),
            default_model_class: "small".into(),
            cache_ttl_minutes: 15,
        },
        InsightScenario {
            key: "acceptance_assistant".into(),
            default_model_class: "medium".into(),
            cache_ttl_minutes: 30,
        },
        InsightScenario {
            key: "delivery_eta_forecast".into(),
            default_model_class: "large".into(),
            cache_ttl_minutes: 240,
        },
        InsightScenario {
            key: "milestone_risk_forecast".into(),
            default_model_class: "large".into(),
            cache_ttl_minutes: 240,
        },
        InsightScenario {
            key: "token_efficiency_advisor".into(),
            default_model_class: "small".into(),
            cache_ttl_minutes: 120,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::starter_scenarios;

    #[test]
    fn starter_scenarios_cover_v0_1_7_basics() {
        let keys: HashSet<_> = starter_scenarios()
            .into_iter()
            .map(|scenario| scenario.key)
            .collect();

        for required in [
            "daily_summary",
            "build_failure_explainer",
            "acceptance_assistant",
            "delivery_eta_forecast",
            "token_efficiency_advisor",
        ] {
            assert!(
                keys.contains(required),
                "missing starter scenario: {required}"
            );
        }
    }

    #[test]
    fn starter_scenarios_have_unique_keys() {
        let scenarios = starter_scenarios();
        let unique_keys: HashSet<_> = scenarios
            .iter()
            .map(|scenario| scenario.key.as_str())
            .collect();

        assert_eq!(
            unique_keys.len(),
            scenarios.len(),
            "starter scenario keys must be unique"
        );
    }
}
