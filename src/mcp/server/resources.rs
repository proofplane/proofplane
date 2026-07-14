use rmcp::model::{
    Annotated, ListResourcesResult, RawResource, ReadResourceResult, Resource, ResourceContents,
};

use super::common::not_found;
use crate::mcp::docs::{GuideTopic, TOPICS};

const DOC_URI_PREFIX: &str = "proofplane://docs/";
const MARKDOWN_MIME_TYPE: &str = "text/markdown";

pub(super) fn list_doc_resources() -> ListResourcesResult {
    ListResourcesResult::with_all_items(TOPICS.iter().map(resource_metadata).collect())
}

pub(super) fn read_doc_resource(uri: &str) -> Result<ReadResourceResult, rmcp::ErrorData> {
    let topic = TOPICS
        .iter()
        .find(|topic| doc_uri(topic.topic) == uri)
        .ok_or_else(not_found)?;

    Ok(ReadResourceResult::new(vec![ResourceContents::text(
        topic.markdown,
        uri,
    )
    .with_mime_type(MARKDOWN_MIME_TYPE)]))
}

fn resource_metadata(topic: &GuideTopic) -> Resource {
    Annotated::new(
        RawResource::new(doc_uri(topic.topic), topic.topic)
            .with_title(topic.title)
            .with_mime_type(MARKDOWN_MIME_TYPE),
        None,
    )
}

fn doc_uri(topic: &str) -> String {
    format!("{DOC_URI_PREFIX}{topic}")
}

#[cfg(test)]
mod tests {
    use rmcp::model::{ErrorCode, ResourceContents};

    use super::{list_doc_resources, read_doc_resource, MARKDOWN_MIME_TYPE};
    use crate::mcp::docs::TOPICS;

    #[test]
    fn list_has_exact_ordered_metadata_without_pagination() {
        let listed = list_doc_resources();

        assert_eq!(listed.meta, None);
        assert_eq!(listed.next_cursor, None);
        assert_eq!(listed.resources.len(), TOPICS.len());
        for (resource, topic) in listed.resources.iter().zip(TOPICS) {
            assert_eq!(resource.uri, format!("proofplane://docs/{}", topic.topic));
            assert_eq!(resource.name, topic.topic);
            assert_eq!(resource.title.as_deref(), Some(topic.title));
            assert_eq!(resource.mime_type.as_deref(), Some(MARKDOWN_MIME_TYPE));
            assert_eq!(resource.description, None);
            assert_eq!(resource.size, None);
            assert_eq!(resource.annotations, None);
        }
    }

    #[test]
    fn every_registered_uri_returns_the_registry_markdown() {
        for topic in TOPICS {
            let uri = format!("proofplane://docs/{}", topic.topic);
            let result = read_doc_resource(&uri).expect("registered resource reads");

            assert_eq!(result.contents.len(), 1);
            assert_eq!(
                result.contents[0],
                ResourceContents::text(topic.markdown, uri).with_mime_type(MARKDOWN_MIME_TYPE)
            );
        }
    }

    #[test]
    fn malformed_and_unknown_uris_return_the_existing_not_found_problem() {
        for uri in [
            "",
            "proofplane://docs/",
            "proofplane://docs/unknown",
            "proofplane://docs/Glossary",
            "proofplane://DOCS/glossary",
            "http://docs/glossary",
            "proofplane:docs/glossary",
            "proofplane://docs/glossary/extra",
            "proofplane://docs/glossary?view=full",
            "proofplane://docs/glossary#top",
            "proofplane://docs/%67lossary",
            "proofplane://docs/submitting%2Devidence",
        ] {
            let error = read_doc_resource(uri).expect_err("invalid resource is rejected");
            assert_eq!(error.code, ErrorCode(-32002), "URI: {uri}");
            assert_eq!(
                error.data.expect("problem data")["problem"]["code"],
                "not_found"
            );
        }
    }
}
