//! Binary entry of the ACTUS Explorer server.

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(async {
            if let Err(e) = actus_web::serve().await {
                eprintln!("server error: {e}");
                std::process::exit(1);
            }
        });
}
