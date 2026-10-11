//! Parser tests, including the B6 error-recovery cases (docs/phase-0-spec.md).

use logos_syntax::{Structure, SyntaxKind, parse};

/// The tree's text is the source, whatever the errors.
fn assert_lossless(src: &str) -> logos_syntax::Parse {
    let p = parse(src);
    assert_eq!(p.syntax().to_string(), src);
    p
}

fn errors(src: &str) -> Vec<(String, &str)> {
    assert_lossless(src)
        .errors
        .iter()
        .map(|e| (e.message.clone(), &src[e.span.clone()]))
        .collect()
}

const MODEL: &str = r#"extensions [ nw table ]
__includes [ "utils.nls" ]
globals [ total max-age ]  ; counters
breed [ wolves wolf ]
directed-link-breed [ roads road ]
wolves-own [ energy ]
turtles-own [ age ]
patches-own [ grass? ]
links-own [ weight ]
roads-own [ capacity ]

to setup
  clear-all
  create-wolves 5 [ set energy random 10 ]
  reset-ticks
end

to-report total-energy [ who-set factor ]
  report factor * sum [ energy ] of who-set
end

TO Go
  ask wolves [ if (energy > 0) [ fd 1 ] ]
  tick
END
"#;

#[test]
fn reads_declarations_and_procedures() {
    let p = assert_lossless(MODEL);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let mut s = Structure::default();
    s.add(&p.source_file());
    assert_eq!(s.globals, ["TOTAL", "MAX-AGE"]);
    assert_eq!(s.turtles_own, ["AGE"]);
    assert_eq!(s.patches_own, ["GRASS?"]);
    assert_eq!(s.links_own, ["WEIGHT"]);
    assert_eq!(s.extensions, ["NW", "TABLE"]);
    assert_eq!(s.includes, ["utils.nls"]);
    assert_eq!(s.breeds.len(), 2);
    assert_eq!(
        (
            s.breeds[0].name.as_str(),
            s.breeds[0].singular.as_deref(),
            s.breeds[0].owns.clone()
        ),
        ("WOLVES", Some("WOLF"), vec!["ENERGY".to_owned()])
    );
    assert!(s.breeds[1].is_link && s.breeds[1].directed);
    assert_eq!(s.breeds[1].owns, ["CAPACITY"]);
    assert_eq!(
        s.procedures,
        [
            ("SETUP".to_owned(), false, vec![]),
            (
                "TOTAL-ENERGY".to_owned(),
                true,
                vec!["WHO-SET".to_owned(), "FACTOR".to_owned()]
            ),
            ("GO".to_owned(), false, vec![]),
        ]
    );
    let go = p.source_file().procedures().nth(2).unwrap();
    assert!(go.has_end());
    // `ask wolves [ if (energy > 0) [ fd 1 ] ]`: one bracket group with a paren group and a
    // nested bracket group inside.
    let body = go.body().unwrap();
    let outer: Vec<_> = body.children().collect();
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].kind(), SyntaxKind::BRACKET_GROUP);
    let inner: Vec<_> = outer[0].children().map(|n| n.kind()).collect();
    assert_eq!(inner, [SyntaxKind::PAREN_GROUP, SyntaxKind::BRACKET_GROUP]);
}

// ---- B6: error recovery ---------------------------------------------------------------

#[test]
fn missing_end_is_reported_and_the_next_procedure_still_parses() {
    let src = "to setup\n  clear-all\n\nto go\n  tick\nend\n";
    assert_eq!(errors(src), [("missing END".to_owned(), "to setup")]);
    let p = parse(src);
    let names: Vec<_> = p
        .source_file()
        .procedures()
        .map(|p| p.name().unwrap().text().to_owned())
        .collect();
    assert_eq!(names, ["setup", "go"]);
    assert!(!p.source_file().procedures().next().unwrap().has_end());
}

#[test]
fn missing_end_before_a_declaration_and_at_the_end_of_the_file() {
    let src = "to a\n  set breed wolves\nglobals [ g ]\nto b\n  fd 1";
    assert_eq!(
        errors(src),
        [
            ("missing END".to_owned(), "to a"),
            ("missing END".to_owned(), "to b"),
        ]
    );
    let mut s = Structure::default();
    s.add(&parse(src).source_file());
    assert_eq!(
        s.globals,
        ["G"],
        "the declaration after the unfinished procedure is read"
    );
}

#[test]
fn unbalanced_brackets() {
    // An unclosed `[` is reported at the bracket, and `end` still closes the procedure.
    let src = "to go\n  ask turtles [ fd 1\nend\nto b\nend";
    assert_eq!(errors(src), [("missing ]".to_owned(), "[")]);
    assert_eq!(parse(src).source_file().procedures().count(), 2);

    // A stray closer.
    let src = "to go\n  fd 1 ]\nend";
    assert_eq!(
        errors(src),
        [("] without a matching opening bracket".to_owned(), "]")]
    );

    // A `]` that closes an outer bracket while a `(` is still open.
    let src = "to go\n  ask turtles [ fd (1 + 2 ]\nend";
    let e = errors(src);
    assert_eq!(e, [("missing )".to_owned(), "(")]);

    // Unclosed declaration list.
    let src = "globals [ a b\nto go\nend";
    assert_eq!(errors(src), [("missing ]".to_owned(), "[")]);
    let mut s = Structure::default();
    s.add(&parse(src).source_file());
    assert_eq!(s.globals, ["A", "B"]);
    assert_eq!(s.procedures.len(), 1);
}

#[test]
fn unterminated_string() {
    let src = "to go\n  print \"hello\n  tick\nend";
    assert_eq!(
        errors(src),
        [("Closing double quote is missing".to_owned(), "\"hello")]
    );
    assert!(
        parse(src)
            .source_file()
            .procedures()
            .next()
            .unwrap()
            .has_end()
    );
}

#[test]
fn other_errors() {
    assert_eq!(
        errors("fd 1\nto go\nend"),
        [(
            "expected a declaration or TO / TO-REPORT".to_owned(),
            "fd 1"
        )]
    );
    assert_eq!(
        errors("to\nend"),
        [("expected a procedure name".to_owned(), "to")]
    );
    assert_eq!(
        errors("globals x"),
        [
            ("expected [ after GLOBALS".to_owned(), "globals"),
            ("expected a declaration or TO / TO-REPORT".to_owned(), "x")
        ]
    );
    assert_eq!(
        errors("globals [ 1 ]"),
        [("expected a name".to_owned(), "1")]
    );
    assert_eq!(
        errors("end"),
        [("expected a declaration or TO / TO-REPORT".to_owned(), "end")]
    );
    assert_eq!(
        errors("to go\nend ]"),
        [("expected a declaration or TO / TO-REPORT".to_owned(), "]")]
    );
}

#[test]
fn never_panics_on_fragments() {
    let src = MODEL;
    // Every prefix and every suffix of a model, cut at every character boundary.
    for (i, _) in src.char_indices() {
        assert_lossless(&src[..i]);
        assert_lossless(&src[i..]);
    }
    for s in [
        "[",
        "]",
        "(",
        ")",
        "to",
        "to-report [",
        "end end",
        "\"",
        "{{",
        "breed [",
        ";",
    ] {
        assert_lossless(s);
    }
}
