pub mod ast;
pub mod lexer;
pub mod parser;

pub use parser::CstData;

pub fn parse(source_id: crate::source::SourceId, source: &str) -> super::Parse<CstData> {
    let mut diagnostics = Vec::new();
    let cst = parser::Parser::new(source, &mut diagnostics).parse(&mut diagnostics);
    finish_parse(source_id, cst.into_data(), diagnostics)
}

pub fn parse_document(
    source_id: crate::source::SourceId,
    source: &crate::document::DocumentText,
) -> super::Parse<CstData> {
    let mut diagnostics = Vec::new();
    let (tokens, spans) = lexer::tokenize_document(source, &mut diagnostics);
    let cst =
        parser::Parser::from_token_stream(source.byte_len(), tokens, spans).parse(&mut diagnostics);
    finish_parse(source_id, cst.into_data(), diagnostics)
}

fn finish_parse(
    source_id: crate::source::SourceId,
    syntax: CstData,
    diagnostics: Vec<parser::Diagnostic>,
) -> super::Parse<CstData> {
    let mut diagnostics = super::convert_diagnostics(source_id, diagnostics);
    for issue in ast::validate(source_id, &syntax) {
        let diagnostic = issue.into_diagnostic();
        let start = diagnostic.labels[0].location.start;
        if !diagnostics.iter().any(|existing| {
            existing
                .labels
                .first()
                .is_some_and(|label| label.location.start == start)
        }) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics.sort_by_key(|diagnostic| {
        diagnostic
            .labels
            .first()
            .map_or(u32::MAX, |label| label.location.start)
    });
    super::Parse {
        syntax,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{AstNode, Binding, ExpectedSyntax, Program, StringLiteral};
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
    fn cst_preserves_native_declarations_losslessly() {
        let source = "native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any); map";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("native.xl", source);
        let parsed = parse(id, source);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
        let program = Program::cast(&parsed.syntax, NodeRef::ROOT).unwrap();
        assert!(matches!(
            program.body().unwrap().bindings().next(),
            Some(Binding::Native(_))
        ));
    }

    #[test]
    fn cst_preserves_type_and_field_decorators_losslessly() {
        let source = "@outer @factory(1) type T = Int; { @field value: 2 }";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("decorators.xl", source);
        let parsed = parse(id, source);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
        let program = Program::cast(&parsed.syntax, NodeRef::ROOT).unwrap();
        let Binding::Type(binding) = program.body().unwrap().bindings().next().unwrap() else {
            panic!("expected type binding")
        };
        assert_eq!(binding.decorators().count(), 2);
    }

    #[test]
    fn decorators_reject_unsupported_binding_targets() {
        let source = "@f let value = 1; value";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("unsupported.xl", source);
        let parsed = parse(id, source);
        assert!(parsed.has_errors());
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

    #[test]
    fn cst_preserves_explicit_call_sections() {
        let source = r"value |> transform\(_1, 123, _0)";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("section.xl", source);
        let parsed = parse(id, source);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
        let tokens = parsed
            .syntax
            .children(NodeRef::ROOT)
            .flat_map(|node| collect_tokens(&parsed.syntax, node))
            .collect::<Vec<_>>();
        assert!(tokens.contains(&Token::SectionLParen));
        assert!(tokens.contains(&Token::IndexedPlaceholder));
    }

    #[test]
    fn typed_views_query_later_syntax_around_a_missing_value() {
        let source = "let x = ; let y = 2; y";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("incomplete.xl", source);
        let parsed = parse(id, source);
        let program = Program::root(&parsed.syntax);
        let body = program.body().unwrap();
        let bindings = body.bindings().collect::<Vec<_>>();
        assert_eq!(bindings.len(), 2);
        assert_eq!(parsed.diagnostics.len(), 1);
        let names = bindings
            .iter()
            .map(|binding| {
                let range = binding.name().unwrap().range().to_usize();
                &source[range]
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["x", "y"]);
        assert_eq!(
            bindings[1].name().unwrap().range(),
            bindings[1].name().unwrap().range()
        );
        let Binding::Let(first) = bindings[0] else {
            panic!("expected let binding");
        };
        assert!(first.value().is_none());
        assert!(body.result().is_some());

        let issues = ast::validate(id, &parsed.syntax);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].expected, ExpectedSyntax::BindingValue);
        assert!(issues[0].location.start == issues[0].location.end);
        assert_eq!(
            std::mem::size_of_val(&program),
            std::mem::size_of_val(&program.syntax())
        );
    }

    #[test]
    fn typed_queries_tolerate_error_subtrees_and_arbitrary_input() {
        let samples = [
            "",
            "let",
            "let = ;",
            "let x = 1, 2; let y = 3; y",
            "\0",
            "let 名字 = ; 名字",
        ];
        for source in samples {
            let mut sources = crate::source::SourceDatabase::default();
            let id = sources.add("sample.xl", source);
            let parsed = parse(id, source);
            let program = Program::root(&parsed.syntax);
            let _ = program.body().map(|body| {
                body.bindings()
                    .map(|binding| binding.name())
                    .collect::<Vec<_>>()
            });
            let _ = ast::validate(id, &parsed.syntax);
        }

        let source = "let x = 1, 2; let y = 3; y";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("error.xl", source);
        let parsed = parse(id, source);
        assert!(contains_rule_error(&parsed.syntax, NodeRef::ROOT));
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);

        let source = "let = 1; 0";
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("missing-name.xl", source);
        let parsed = parse(id, source);
        let issues = ast::validate(id, &parsed.syntax);
        assert!(
            issues
                .iter()
                .any(|issue| issue.expected == ExpectedSyntax::BindingName)
        );
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn unknown_escape_remains_inside_a_queryable_string() {
        let source = r#""a\(b""#;
        let mut sources = crate::source::SourceDatabase::default();
        let id = sources.add("escape.xl", source);
        let parsed = parse(id, source);
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].labels[0].location.range(), 2..4);
        let string_node = find_rule(&parsed.syntax, NodeRef::ROOT, parser::Rule::StringLiteral)
            .expect("string literal remains in CST");
        let string = StringLiteral::cast(&parsed.syntax, string_node).unwrap();
        let parts = string
            .parts()
            .filter_map(|part| part.token().map(|token| token.kind()))
            .collect::<Vec<_>>();
        assert_eq!(
            parts,
            [
                Token::StringText,
                Token::UnknownEscapeSequence,
                Token::StringText
            ]
        );
        let mut reconstructed = String::new();
        reconstruct(&parsed.syntax, source, NodeRef::ROOT, &mut reconstructed);
        assert_eq!(reconstructed, source);
    }

    fn find_rule(cst: &CstData, node: NodeRef, expected: parser::Rule) -> Option<NodeRef> {
        if matches!(cst.get(node), Node::Rule(rule, _) if rule == expected) {
            return Some(node);
        }
        cst.children(node)
            .find_map(|child| find_rule(cst, child, expected))
    }

    fn contains_rule_error(cst: &CstData, node: NodeRef) -> bool {
        matches!(cst.get(node), Node::Rule(parser::Rule::Error, _))
            || cst
                .children(node)
                .any(|child| contains_rule_error(cst, child))
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
