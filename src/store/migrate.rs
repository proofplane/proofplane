use refinery::{embed_migrations, Error, Report, Runner};
use tokio_postgres::Client;
use tracing::debug;

embed_migrations!("./migrations");

pub fn migration_runner() -> Runner {
    migrations::runner()
}

pub async fn migrate(client: &mut Client) -> Result<(), Error> {
    migration_runner()
        .run_async(client)
        .await
        .map(|report: Report| {
            debug!(
                "done! ran {} migrations",
                report.applied_migrations().iter().count()
            );
        })
}
