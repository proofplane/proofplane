use proofplane::{config, worker};

fn main() {
    if let Err(error) = config::load_from_env() {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("{}", worker::startup_message());
}
