use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Ready {
        protocol_version: u32,
        dataset_version: String,
        default_route: String,
        retriever_version: String,
        routes: Vec<String>,
    },
    Search {
        protocol_version: u32,
        request_id: String,
        cues: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filters: Option<Filters>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        route: Option<String>,
    },
    Results {
        protocol_version: u32,
        request_id: String,
        dataset_version: String,
        retriever_version: String,
        route: String,
        degraded_routes: Vec<String>,
        elapsed_ms: f64,
        truncated: bool,
        results: Vec<ResultItem>,
    },
    Error {
        protocol_version: u32,
        request_id: Option<String>,
        code: String,
        message: String,
        fatal: bool,
    },
    Shutdown {
        protocol_version: u32,
        request_id: String,
    },
    Bye {
        protocol_version: u32,
        request_id: String,
    },
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResultItem {
    pub asset_uri: Option<String>,
    pub attribution: String,
    pub caption: Option<String>,
    pub dataset_version: String,
    pub id: String,
    pub kind: String,
    pub language: String,
    pub matched_fields: Vec<String>,
    pub people: Vec<String>,
    pub rank: usize,
    pub retriever_version: String,
    pub routes: Vec<String>,
    pub safe: bool,
    pub scores: Scores,
    pub source: String,
    pub source_url: String,
    pub tags: Vec<String>,
    pub template: Option<String>,
    pub text: Option<String>,
    pub title: String,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scores {
    pub dense_rank: Option<usize>,
    pub fused: f64,
    pub lexical_rank: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_messages_round_trip_without_schema_loss() {
        for line in include_str!("../tests/fixtures/protocol/messages.jsonl").lines() {
            let expected: serde_json::Value = serde_json::from_str(line).unwrap();
            let message: Message = serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(serde_json::to_value(message).unwrap(), expected);
        }
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let invalid = r#"{"type":"shutdown","protocol_version":1,"request_id":"x","extra":true}"#;
        assert!(serde_json::from_str::<Message>(invalid).is_err());
    }
}
