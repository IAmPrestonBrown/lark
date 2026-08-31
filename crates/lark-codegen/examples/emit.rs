//! Prints the emitted C for a file. A development aid.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: emit <file.lark> [c|h|map] [search-dir ...]");
        return;
    };
    let what = args.next().unwrap_or_else(|| "c".to_owned());
    let search: Vec<std::path::PathBuf> = args.map(std::path::PathBuf::from).collect();
    let Ok(resolution) = lark_resolve::resolve_path(std::path::Path::new(&path), &search) else {
        eprintln!("cannot read {path}");
        return;
    };
    let Some(root) = resolution.root else {
        return;
    };
    let options = lark_codegen::Options::default();
    let mut diagnostics = lark_diag::Diagnostics::new();
    let program = lark_mono::collect(&resolution.graph, &mut diagnostics);
    let Some(emitted) = lark_codegen::emit(&resolution.graph, root, &options, &program) else {
        return;
    };
    match what.as_str() {
        "h" => print!("{}", emitted.header),
        "map" => print!("{}", emitted.line_map_text("source")),
        _ => print!("{}", emitted.c),
    }
}
