// SPDX-License-Identifier: MPL-2.0
use ru_dbviewer::{CAPABILITIES, HOST_RANGE, app::App, protocol::Rpc};
use serde_json::json;
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !args.is_empty() {
        match args[0].as_str() {
            "--help" if args.len() == 1 => {
                println!(
                    "ru-dbviewer — SQLite and PostgreSQL for Runyte\n\nUsage: ru-dbviewer [--help | --version | --print-config [--plugin-id ID]]\nWithout arguments, speaks the runyte-1 plugin protocol over stdin/stdout."
                );
                return;
            }
            "--version" if args.len() == 1 => {
                println!("ru-dbviewer {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--print-config" => {
                let id = if args.len() == 1 {
                    "dbviewer"
                } else if args.len() == 3 && args[1] == "--plugin-id" {
                    &args[2]
                } else {
                    fail("Invalid config arguments")
                };
                if id.is_empty()
                    || id.len() > 48
                    || !id
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                {
                    fail("Invalid plugin ID");
                }
                let executable =
                    std::env::current_exe().unwrap_or_else(|_| fail("Cannot locate executable"));
                println!("{}",serde_json::to_string_pretty(&json!({"plugins":[{"id":id,"enabled":true,"api":"runyte-1","runyte":HOST_RANGE,"executable":executable,"args":[],"capabilities":CAPABILITIES,"bindings":{"back":"-"}}]})).unwrap());
                return;
            }
            _ => fail("Unknown arguments; use --help"),
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .unwrap_or_else(|_| fail("Runtime initialization failed"));
    let (rpc, events) = Rpc::start().unwrap_or_else(|_| fail("Transport initialization failed"));
    let result = runtime.block_on(App::run(rpc.clone(), events));
    rpc.stop();
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    if result.is_err() {
        std::process::exit(1);
    }
}
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}
