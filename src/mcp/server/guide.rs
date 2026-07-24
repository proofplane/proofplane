use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{common::authorize_connection, ProofplaneMcp};
use crate::mcp::docs::{find_topic, TOPICS};

const INDEX_TITLE: &str = "Proofplane guide topics";
const INDEX_MARKDOWN: &str =
    "Call `get_proofplane_guide` again with one of the topics listed in `topics`.";

#[tool_router(router = guide_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "get_proofplane_guide",
        description = "Return embedded Proofplane guidance for a topic, or the ordered topic index when the topic is omitted or unknown."
    )]
    async fn get_proofplane_guide(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetProofplaneGuideRequest>,
    ) -> Result<Json<GetProofplaneGuideResponse>, ErrorData> {
        authorize_connection(&ctx)?;

        Ok(Json(guide_response(args.topic.as_deref())))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetProofplaneGuideRequest {
    topic: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
struct GetProofplaneGuideResponse {
    topic: Option<String>,
    title: String,
    markdown: String,
    topics: Vec<GuideTopicSummary>,
}

#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
struct GuideTopicSummary {
    topic: String,
    title: String,
}

fn guide_response(topic: Option<&str>) -> GetProofplaneGuideResponse {
    if let Some(topic) = topic.map(str::trim).and_then(find_topic) {
        return GetProofplaneGuideResponse {
            topic: Some(topic.topic.to_owned()),
            title: topic.title.to_owned(),
            markdown: topic.markdown.to_owned(),
            topics: Vec::new(),
        };
    }

    GetProofplaneGuideResponse {
        topic: None,
        title: INDEX_TITLE.to_owned(),
        markdown: INDEX_MARKDOWN.to_owned(),
        topics: TOPICS
            .iter()
            .map(|topic| GuideTopicSummary {
                topic: topic.topic.to_owned(),
                title: topic.title.to_owned(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{guide_response, INDEX_MARKDOWN, INDEX_TITLE};
    use crate::mcp::docs::TOPICS;

    #[test]
    fn known_topic_returns_embedded_markdown_without_the_index() {
        let response = guide_response(Some("  submitting-evidence\t"));

        assert_eq!(response.topic.as_deref(), Some("submitting-evidence"));
        assert_eq!(response.title, "Submitting Evidence");
        assert_eq!(response.markdown, TOPICS[1].markdown);
        assert!(response.topics.is_empty());
    }

    #[test]
    fn missing_blank_case_mismatched_and_unknown_topics_return_the_same_index() {
        let expected = guide_response(None);

        assert_eq!(expected.topic, None);
        assert_eq!(expected.title, INDEX_TITLE);
        assert_eq!(expected.markdown, INDEX_MARKDOWN);
        assert_eq!(expected.topics.len(), TOPICS.len());
        for input in [Some(""), Some(" \n"), Some("Glossary"), Some("unknown")] {
            assert_eq!(guide_response(input), expected);
        }
    }

    #[test]
    fn every_registered_topic_resolves() {
        for registered in TOPICS {
            let response = guide_response(Some(registered.topic));
            assert_eq!(response.topic.as_deref(), Some(registered.topic));
            assert_eq!(response.title, registered.title);
            assert_eq!(response.markdown, registered.markdown);
            assert!(response.topics.is_empty());
        }
    }
}
