use proofplane::{config, migrations};

fn main() {
    if let Err(error) = config::load_from_env() {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("{}", migrations::startup_message());
    println!("proofplane seed scaffold ready");
}
