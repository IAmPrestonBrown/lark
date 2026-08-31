//! Prints the tree and the errors for a file. A development aid.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: dump <file.lark> [--errors]");
        return;
    };
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("cannot read {path}");
        return;
    };
    let parsed = lark_syntax::parse(&source, &lark_syntax::NoNames);
    if args.next().as_deref() == Some("--errors") {
        for error in parsed.errors() {
            println!("{} at {:?}", error.code, error.span);
        }
        println!("{} errors", parsed.errors().len());
    } else {
        print!("{}", parsed.tree_text());
    }
}
