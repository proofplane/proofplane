use proofplane::persistence::Postgres;
use uuid::Uuid;

use super::scenario::types::{TestFramework, TestFrameworkRequirement};

const EXAMPLE_FRAMEWORK_ID: &str = "30000000-0000-4000-8000-000000000000";
const REQ1_ID: &str = "30000000-0000-4000-8000-000000000001";
const REQ3_ID: &str = "30000000-0000-4000-8000-000000000002";

pub(super) async fn seed(postgres: &Postgres) -> Vec<TestFramework> {
    let framework = example_framework();
    let mut client = postgres
        .get()
        .await
        .expect("reference fixture database connection opens");
    let transaction = client
        .transaction()
        .await
        .expect("reference fixture transaction starts");

    transaction
        .execute(
            r#"
INSERT INTO frameworks (id, code, name, description)
VALUES ($1, $2, $3, $4)
"#,
            &[
                &framework.id,
                &framework.code,
                &framework.name,
                &framework.description,
            ],
        )
        .await
        .expect("example framework fixture inserts");

    for requirement in &framework.requirements {
        transaction
            .execute(
                r#"
INSERT INTO framework_requirements (id, framework_id, code, title, description)
VALUES ($1, $2, $3, $4, $5)
"#,
                &[
                    &requirement.id,
                    &requirement.framework_id,
                    &requirement.code,
                    &requirement.title,
                    &requirement.description,
                ],
            )
            .await
            .expect("example requirement fixture inserts");
    }

    transaction
        .commit()
        .await
        .expect("reference fixture transaction commits");

    vec![framework]
}

fn example_framework() -> TestFramework {
    let framework_id = fixture_id(EXAMPLE_FRAMEWORK_ID);

    TestFramework {
        id: framework_id,
        code: "example".to_owned(),
        name: "Example Framework".to_owned(),
        description: "Example framework for tests.".to_owned(),
        requirements: vec![
            TestFrameworkRequirement {
                id: fixture_id(REQ1_ID),
                framework_id,
                code: "REQ-1".to_owned(),
                title: "Logical access security".to_owned(),
                description: "Seeded example requirement.".to_owned(),
            },
            TestFrameworkRequirement {
                id: fixture_id(REQ3_ID),
                framework_id,
                code: "REQ-3".to_owned(),
                title: "System monitoring".to_owned(),
                description: "Seeded example requirement.".to_owned(),
            },
        ],
    }
}

fn fixture_id(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("reference fixture id is a UUID")
}
