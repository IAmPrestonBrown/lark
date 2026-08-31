//! Prints one file, formatted. A development aid.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: fmt <file.lark>");
        return;
    };
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("cannot read {path}");
        return;
    };
    print!("{}", lark_fmt::format(&source));
}
