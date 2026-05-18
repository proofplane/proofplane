use proofplane::{config, mcp};

fn main() {
    if let Err(error) = config::load_from_env() {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("{}", mcp::startup_message());
}
