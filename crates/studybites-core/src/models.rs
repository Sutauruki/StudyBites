use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub archived: bool,
    pub exam_date: Option<String>,
    /// Number of top-level + nested topics, for the subject list.
    pub topic_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: i64,
    pub subject_id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub week: Option<i64>,
    pub position: i64,
    pub is_off_scheme: bool,
    pub supports_topic_id: Option<i64>,
    pub is_preview: bool,
    /// Direct children, ordered by position. Phase 1 nests one level deep.
    pub children: Vec<Topic>,
}

/// Input for creating a topic.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NewTopic {
    pub subject_id: i64,
    pub parent_id: Option<i64>,
    pub title: String,
    pub week: Option<i64>,
    #[serde(default)]
    pub is_off_scheme: bool,
}
