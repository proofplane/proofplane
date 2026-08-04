use testcontainers::{
    core::{Host, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

pub struct TestPubSub {
    // Keep deltio alive for the whole test suite.
    _container: ContainerAsync<GenericImage>,
    emulator_host: String,
}

impl TestPubSub {
    pub async fn start() -> Self {
        let container = GenericImage::new("ghcr.io/jeffijoe/deltio", "latest")
            .with_exposed_port(8085.tcp())
            .with_wait_for(WaitFor::seconds(2))
            .with_host("host.docker.internal", Host::HostGateway)
            .start()
            .await
            .expect("deltio starts");
        let host = container.get_host().await.expect("deltio has a host");
        let port = container
            .get_host_port_ipv4(8085)
            .await
            .expect("deltio exposes Pub/Sub");

        Self {
            _container: container,
            emulator_host: format!("{host}:{port}"),
        }
    }

    pub fn emulator_host(&self) -> &str {
        &self.emulator_host
    }
}
