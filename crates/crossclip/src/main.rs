use std::process::ExitCode;

const USAGE: &str = "\
crossclip - a shared clipboard between your machines

USAGE:
    crossclip <COMMAND>

COMMANDS:
    run        Start the agent (default)
    help       Show this help
    version    Show the version
";

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        None | Some("run") => {
            eprintln!(
                "crossclip {}: agent not implemented yet",
                crossclip_core::VERSION
            );
            ExitCode::FAILURE
        }
        Some("version" | "--version" | "-V") => {
            println!("crossclip {}", crossclip_core::VERSION);
            ExitCode::SUCCESS
        }
        Some("help" | "--help" | "-h") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
