use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match pgsandbox::cli::run(std::env::args().skip(1).collect()).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("pgsandbox failed to start: {error}");
            ExitCode::from(1)
        }
    }
}
