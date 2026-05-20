use refinery::{embed_migrations, Error, Report};
use tokio_postgres::Client;
use tracing::debug;

embed_migrations!("./migrations");

pub async fn migrate(client: &mut Client) -> Result<(), Error> {
    migrations::runner()
        .run_async(client)
        .await
        .map(|report: Report| {
            debug!(
                "done! ran {} migrations",
                report.applied_migrations().iter().count()
            );
        })
}
