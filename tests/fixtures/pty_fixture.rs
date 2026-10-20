//! Built only with --features test-fixture. Never included in the desktop bundle.
use std::{
    io::{self, Write},
    time::Duration,
};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("exit");
    match mode {
        "linger" => loop {
            std::thread::sleep(Duration::from_millis(100));
        },
        "job-host" => run_job_host(&args[1]),
        "descendant" => {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("linger")
                .spawn()
                .unwrap();
            println!("CHILD_PID={}", child.id());
            if let Some(path) = args.get(1) {
                std::fs::write(
                    path,
                    serde_json::to_vec(&[std::process::id(), child.id()]).unwrap(),
                )
                .unwrap();
            }
            io::stdout().flush().unwrap();
            // The parent waits while the child stays alive. Only Job cleanup can terminate it.
            child.wait().unwrap();
        }
        "flood" => {
            for i in 0..100_000 {
                println!("{i:06} 中文 🌱 0123456789abcdefghijklmnopqrstuvwxyz");
            }
            io::stdout().flush().unwrap();
        }
        "echo" => {
            println!("READY");
            io::stdout().flush().unwrap();
            let mut line = String::new();
            while io::stdin().read_line(&mut line).unwrap() > 0 {
                print!("ECHO:{line}");
                io::stdout().flush().unwrap();
                if line.trim() == "quit" {
                    break;
                }
                line.clear();
            }
        }
        "metadata" => {
            println!("ARGS={}", serde_json::to_string(&args[1..]).unwrap());
            println!("CWD={}", std::env::current_dir().unwrap().display());
            println!(
                "ENV={}",
                std::env::var("TERMINDER_FIXTURE").unwrap_or_default()
            );
            println!(
                "PROGRAM={}",
                std::env::var("TERM_PROGRAM").unwrap_or_default()
            );
        }
        _ => {
            print!("\x1b[31mFIXTURE_READY\x1b[0m\r\n中文 🌱\r\nTAIL_COMPLETE\r\n");
            io::stdout().flush().unwrap();
            std::process::exit(23);
        }
    }
}

fn run_job_host(pid_file: &str) {
    use std::{collections::BTreeMap, sync::Arc};
    use terminder_session_core::{backend::native_factory, model::*, transport::GlobalBudget};
    let spec = SpawnSpec {
        profile_id: "crash-fixture".into(),
        profile_name: "Crash fixture".into(),
        program: std::env::current_exe().unwrap(),
        args: vec!["descendant".into(), pid_file.into()],
        env: BTreeMap::new(),
        cwd: std::env::current_dir().unwrap(),
        cwd_source: "explicit".into(),
        size: TerminalSize::default(),
    };
    let backend = native_factory()(spec, Arc::new(GlobalBudget::default())).unwrap();
    loop {
        while backend.io().pop_output().is_some() {}
        std::thread::sleep(Duration::from_millis(10));
    }
}
