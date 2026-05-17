use proofplane::migrations;

fn main() {
    println!("{}", migrations::startup_message());
    println!("proofplane seed scaffold ready");
}
