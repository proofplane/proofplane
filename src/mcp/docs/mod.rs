#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuideTopic {
    pub topic: &'static str,
    pub title: &'static str,
    pub markdown: &'static str,
}

pub(crate) const TOPICS: [GuideTopic; 5] = [
    GuideTopic {
        topic: "glossary",
        title: "Proofplane Glossary",
        markdown: include_str!("glossary.md"),
    },
    GuideTopic {
        topic: "submitting-evidence",
        title: "Submitting Evidence",
        markdown: include_str!("submitting-evidence.md"),
    },
    GuideTopic {
        topic: "controls-and-mappings",
        title: "Controls and Mappings",
        markdown: include_str!("controls-and-mappings.md"),
    },
    GuideTopic {
        topic: "errors-and-not-found",
        title: "Errors and Not Found",
        markdown: include_str!("errors-and-not-found.md"),
    },
    GuideTopic {
        topic: "policies",
        title: "Policies",
        markdown: include_str!("policies.md"),
    },
];

pub(crate) fn find_topic(topic: &str) -> Option<&'static GuideTopic> {
    TOPICS.iter().find(|candidate| candidate.topic == topic)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{find_topic, TOPICS};

    #[test]
    fn registry_has_exact_ordered_topics_and_titles() {
        let actual = TOPICS
            .iter()
            .map(|topic| (topic.topic, topic.title))
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            [
                ("glossary", "Proofplane Glossary"),
                ("submitting-evidence", "Submitting Evidence"),
                ("controls-and-mappings", "Controls and Mappings"),
                ("errors-and-not-found", "Errors and Not Found"),
                ("policies", "Policies"),
            ]
        );
    }

    #[test]
    fn registry_slugs_are_unique_and_embedded_content_is_non_empty() {
        let unique_slugs = TOPICS
            .iter()
            .map(|topic| topic.topic)
            .collect::<HashSet<_>>();

        assert_eq!(unique_slugs.len(), TOPICS.len());
        for topic in TOPICS {
            assert!(
                !topic.title.trim().is_empty(),
                "{} has a title",
                topic.topic
            );
            assert!(
                !topic.markdown.trim().is_empty(),
                "{} has embedded markdown",
                topic.topic
            );
            assert_eq!(find_topic(topic.topic), Some(&topic));
        }
    }

    #[test]
    fn submitting_evidence_distinguishes_human_and_trusted_runtime_uploads() {
        let guide = find_topic("submitting-evidence").expect("submitting evidence guide exists");

        for expected in [
            "Human browser upload",
            "Trusted-runtime machine upload",
            "manage_evidence_submissions",
            "prepare_evidence_submission_upload",
            "Content-Length",
            "get_evidence_submission",
            "same descriptor",
            "File bytes and local paths never pass through MCP or the model",
        ] {
            assert!(
                guide.markdown.contains(expected),
                "guide contains {expected:?}"
            );
        }
    }

    #[test]
    fn policies_distinguishes_human_management_and_trusted_runtime_uploads() {
        let guide = find_topic("policies").expect("policies guide exists");

        for expected in [
            "Human browser management",
            "Trusted-runtime machine upload",
            "manage_policy_document",
            "prepare_policy_document_upload",
            "Content-Length",
            "get_policy",
            "same descriptor",
            "File bytes never pass through MCP or the model",
            "Local paths never pass through MCP or the model",
            "archive",
        ] {
            assert!(
                guide.markdown.contains(expected),
                "guide contains {expected:?}"
            );
        }
    }
}
