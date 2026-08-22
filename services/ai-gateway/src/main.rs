fn main() {
    if let Err(error) = relintor_ai_gateway::run_from_env() {
        eprintln!("Relintor AI gateway stopped: {error}");
        std::process::exit(1);
    }
}
