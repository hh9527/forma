use std::path::PathBuf;

fn main() {
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    for language in ["forma", "json", "toml", "yaml"] {
        let directory = output.join(language);
        std::fs::create_dir_all(&directory).expect("create parser output directory");
        let grammar = format!("src/syntax/{language}/grammar.llw");
        let generated = directory.to_str().expect("UTF-8 output path");
        let success = lelwel::compile(&grammar, generated, false, false, 0, false, true)
            .expect("compile Lelwel grammar");
        assert!(success, "invalid Lelwel grammar: {grammar}");
        println!("cargo:rerun-if-changed={grammar}");
    }
}
