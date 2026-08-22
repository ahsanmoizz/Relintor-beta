fn main() {
    if let Err(error) = relintor_cloud_api::run_from_env() {
        eprintln!("Relintor cloud API stopped: {error}");
        std::process::exit(1);
    }
}
