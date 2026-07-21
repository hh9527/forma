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

#[cfg(test)]
mod tests {
    use super::*;
    use lexer::Token;
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
    fn cst_is_lossless_and_recovery_collects_diagnostics() {
        let source = "let x = 1 // keep me\n let y = ;\n match x { => 1, _ => }";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("broken.xl", source);
        let parsed = parse(id, source);
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
        assert!(parsed.diagnostics.len() >= 2);
    }

    #[test]
    fn cst_preserves_string_quotes_text_escapes_and_interpolation() {
        let source = r#""hi\n \{name}""#;
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("strings.xl", source);
        let parsed = parse(id, source);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let tokens = parsed
            .syntax
            .children(NodeRef::ROOT)
            .flat_map(|node| collect_tokens(&parsed.syntax, node))
            .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec![
                Token::DoubleQuote,
                Token::StringText,
                Token::EscapeSequence,
                Token::StringText,
                Token::InterpolationStart,
                Token::Identifier,
                Token::RBrace,
                Token::DoubleQuote,
            ]
        );
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
    }

    fn collect_tokens(cst: &CstData, node: NodeRef) -> Vec<Token> {
        match cst.get(node) {
            Node::Token(token, _) => vec![token],
            Node::Rule(..) => cst
                .children(node)
                .flat_map(|child| collect_tokens(cst, child))
                .collect(),
        }
    }

    #[test]
    #[ignore = "manual full-file parse baseline"]
    fn full_file_parse_baseline() {
        let source = include_str!("../../../../../examples/mvp/main.xl");
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("main.xl", source);
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            assert!(!parse(id, source).has_errors());
        }
        eprintln!("1,000 full parses: {:?}", started.elapsed());
    }
}
