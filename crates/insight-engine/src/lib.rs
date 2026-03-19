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
            key: "milestone_risk_forecast".into(),
            default_model_class: "large".into(),
            cache_ttl_minutes: 240,
        },
    ]
}
