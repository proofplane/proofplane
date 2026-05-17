use proofplane::{package_name, VERSION};

fn main() {
    assert_eq!(package_name(), "proofplane");
    assert!(!VERSION.is_empty());
}
