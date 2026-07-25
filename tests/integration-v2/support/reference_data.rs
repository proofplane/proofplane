use proofplane::repository::Postgres;
use uuid::Uuid;

use super::scenario::types::{TestFramework, TestFrameworkRequirement};

const SOC2_ID: &str = "30000000-0000-4000-8000-000000000000";
const CC61_ID: &str = "30000000-0000-4000-8000-000000000001";
const CC71_ID: &str = "30000000-0000-4000-8000-000000000002";

pub(super) async fn seed(postgres: &Postgres) -> Vec<TestFramework> {
    let framework = soc2();
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
        .expect("SOC 2 framework fixture inserts");

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
            .expect("SOC 2 requirement fixture inserts");
    }

    transaction
        .commit()
        .await
        .expect("reference fixture transaction commits");

    vec![framework]
}

fn soc2() -> TestFramework {
    let framework_id = fixture_id(SOC2_ID);

    TestFramework {
        id: framework_id,
        code: "soc2".to_owned(),
        name: "SOC 2".to_owned(),
        description: "SOC 2 Trust Services Criteria.".to_owned(),
        requirements: vec![
            TestFrameworkRequirement {
                id: fixture_id(CC61_ID),
                framework_id,
                code: "CC6.1".to_owned(),
                title: "Logical access security".to_owned(),
                description: "Seeded SOC 2 requirement.".to_owned(),
            },
            TestFrameworkRequirement {
                id: fixture_id(CC71_ID),
                framework_id,
                code: "CC7.1".to_owned(),
                title: "System monitoring".to_owned(),
                description: "Seeded SOC 2 requirement.".to_owned(),
            },
        ],
    }
}

fn fixture_id(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("reference fixture id is a UUID")
}
