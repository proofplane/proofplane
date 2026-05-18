use proofplane::{config, observability, worker, VERSION};

fn main() {
    let config = match config::load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = observability::init_tracing(&config.observability) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    tracing::info!(
        binary = "worker",
        version = VERSION,
        "{}",
        worker::startup_message()
    );
}
