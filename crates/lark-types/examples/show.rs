//! Prints every diagnostic for a file. A development aid.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: show <file.lark> [search-dir ...]");
        return;
    };
    let search: Vec<std::path::PathBuf> = args.map(std::path::PathBuf::from).collect();
    let Ok(resolution) = lark_resolve::resolve_path(std::path::Path::new(&path), &search) else {
        eprintln!("cannot read {path}");
        return;
    };

    let mut all = resolution.diagnostics.clone();
    all.extend(lark_types::check_resolution(&resolution));
    all.sort_by_position();
    print!("{}", lark_diag::render_all(&all, &resolution.sources));
}
