pub mod lexer;
pub mod parser;

pub use parser::CstData;

pub fn parse(source_id: crate::source::SourceId, source: &str) -> super::Parse<CstData> {
    let mut diagnostics = Vec::new();
    let cst = parser::Parser::new(source, &mut diagnostics).parse(&mut diagnostics);
    super::Parse {
        syntax: cst.into_data(),
        diagnostics: super::convert_diagnostics(source_id, diagnostics),
    }
}

pub fn parse_document(
    source_id: crate::source::SourceId,
    source: &crate::document::DocumentText,
) -> super::Parse<CstData> {
    let mut diagnostics = Vec::new();
    let (tokens, spans) = lexer::tokenize_document(source, &mut diagnostics);
    let cst =
        parser::Parser::from_token_stream(source.byte_len(), tokens, spans).parse(&mut diagnostics);
    super::Parse {
        syntax: cst.into_data(),
        diagnostics: super::convert_diagnostics(source_id, diagnostics),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parser::{Node, NodeRef};

    fn reconstruct(cst: &CstData, source: &str, node: NodeRef, output: &mut String) {
        match cst.get(node) {
            Node::Token(..) => output.push_str(&source[cst.span(node)]),
            Node::Rule(..) => {
                for child in cst.children(node) {
                    reconstruct(cst, source, child, output);
                }
            }
        }
    }

    #[test]
    fn cst_is_lossless_for_toml_tables_comments_and_strings() {
        let source = "# package\n[package]\nname = \"telora\" # comment\nlines = '''a\nb'''\n";
        let mut sources = crate::SourceDatabase::default();
        let id = sources.add("data.toml", source);
        let parsed = parse(id, source);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
    }
}
