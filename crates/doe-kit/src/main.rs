use clap::Command;

fn main() {
    let cmd = Command::new("doe")
        .version(env!("CARGO_PKG_VERSION"))
        .arg_required_else_help(true);
    cmd.get_matches();
}
