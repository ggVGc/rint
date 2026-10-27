use rint::{
    Engine, bindings,
    native::{FromValue, IntoValue},
    value::Value,
};

#[test]
fn discovers_aliases_generics_and_function_values_without_local_bindings() {
    let source = r#"
        use dependency::nested::score as score;
        use dependency as dep;
        fn local(n: i64) -> i64 { n }
        fn main() {
            let callback = score;
            callback("hello");
            dep::identity::<i64>(local(1));
            dep::identity::<i32>(2);
            dep::identity::<i64>(3);
            let score = |n| n + 1;
            score(3);
            let other = other_dependency::score;
        }
    "#;
    assert_eq!(
        bindings::discover(source).unwrap(),
        vec![
            "dependency::nested::score",
            "dependency::identity::<i64>",
            "dependency::identity::<i32>",
            "other_dependency::score",
        ]
    );
    let generated = bindings::generate(source).unwrap();
    assert!(generated.contains(
        "engine.register_fn(\"dependency::identity::<i64>\", dependency::identity::<i64>)?;"
    ));
    assert_eq!(generated, bindings::generate(source).unwrap());
}

#[test]
fn invokes_native_functions_and_preserves_distinct_paths() {
    let mut engine = Engine::new();
    engine.register_fn("a::score", |s: &str| s.len()).unwrap();
    engine.register_fn("b::score", |n: i32| n * 2).unwrap();
    engine
        .register_fn("b::identity::<i64>", |n: i64| n)
        .unwrap();
    engine
        .register_fn("b::values", |v: Vec<i32>| {
            v.into_iter().map(|n| n + 1).collect::<Vec<_>>()
        })
        .unwrap();
    engine.register_fn("b::nothing", || ()).unwrap();
    engine
        .register_fn("b::result", || Result::<i32, String>::Err("failure".into()))
        .unwrap();
    let source = r#"
        use a::score as measure;
        use b as dep;
        fn main() {
            let callback = measure;
            assert_eq!(callback("hello"), 5);
            assert_eq!(dep::score(21), 42);
            assert_eq!(dep::identity::<i64>(7), 7);
            assert_eq!(dep::values(vec![1, 2]), vec![2, 3]);
            assert_eq!(dep::nothing(), ());
            assert_eq!(dep::result().unwrap_err(), "failure");
            let score = |n| n + 1;
            assert_eq!(score(1), 2);
        }
    "#;
    assert_eq!(engine.run_source(source).unwrap(), 0);
}

#[test]
fn reports_native_errors_with_call_location_and_checks_arity() {
    let mut engine = Engine::new();
    engine.register_fn("dep::byte", |n: u8| n).unwrap();
    engine
        .register_raw("dep::fail", |_| Err("host failure".into()))
        .unwrap();
    for (call, expected) in [
        ("dep::byte(256)", "out of range for u8"),
        ("dep::byte(-1)", "out of range for u8"),
        ("dep::byte(true)", "expected u8, got bool"),
        ("dep::byte()", "expected 1 arguments, got 0"),
        ("dep::byte(1, 2)", "expected 1 arguments, got 2"),
        ("dep::fail()", "host failure"),
    ] {
        let err = engine
            .run_source(&format!("fn main() {{ {call}; }}"))
            .unwrap_err();
        assert!(err.contains("<script>:1:"), "{err}");
        assert!(err.contains(expected), "{err}");
    }
    assert!(
        engine
            .register_fn("dep::byte", |n: u8| n)
            .unwrap_err()
            .contains("duplicate")
    );
    assert!(
        engine
            .compile("fn main() { missing::function(); }")
            .is_err()
    );
    // An unregistered dependency path must not resolve to a same-named builtin.
    assert!(
        engine
            .compile("fn main() { missing::String::from(\"hello\"); }")
            .is_err()
    );
}

#[test]
fn rejects_invalid_scripts_and_unqualified_unknown_names() {
    assert!(bindings::generate("fn main( {").is_err());
    assert!(bindings::generate("fn main() { missing(); }").is_err());
    assert!(
        bindings::discover("fn main() { println!(\"{}\", std::cmp::max(1, 2)); }")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn checks_integer_returns_and_converts_vectors() {
    assert!(u64::MAX.into_value().unwrap_err().contains("out of range"));
    assert!(u8::from_value(&Value::Int(256)).is_err());
    assert_eq!(
        Vec::<i32>::from_value(&vec![1_i32, 2].into_value().unwrap()).unwrap(),
        vec![1, 2]
    );
}

#[test]
fn cli_reads_edits_at_runtime_and_forwards_arguments_and_exit_status() {
    use std::{cell::RefCell, rc::Rc};
    let path = std::env::temp_dir().join(format!(
        "rint-cli-{}-{}.rs",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let values = Rc::new(RefCell::new(Vec::new()));
    let captured = values.clone();
    let mut engine = Engine::new();
    engine
        .register_fn("dep::record", move |n: i64| captured.borrow_mut().push(n))
        .unwrap();
    for n in [1, 100] {
        std::fs::write(
            &path,
            format!(
                r#"
            fn main() {{
                let args: Vec<String> = std::env::args().collect();
                assert_eq!(args.len(), 2);
                assert_eq!(args[1], "argument");
                dep::record({n});
                std::process::exit(7);
            }}
        "#
            ),
        )
        .unwrap();
        assert_eq!(
            engine
                .run_cli([
                    "runner".into(),
                    path.to_string_lossy().into_owned(),
                    "argument".into()
                ])
                .unwrap(),
            7
        );
    }
    assert_eq!(*values.borrow(), [1, 100]);
    assert!(
        engine
            .run_cli(["runner".into()])
            .unwrap_err()
            .contains("usage:")
    );
    std::fs::remove_file(&path).unwrap();
    assert!(
        engine
            .run_cli(["runner".into(), path.to_string_lossy().into_owned()])
            .is_err()
    );
}

#[test]
fn automatic_bindings_only_target_external_dependencies() {
    let dependencies = vec!["external".to_string()];
    let generated = bindings::generate_for_dependencies(
        "use external::score as score; fn main() { score(1); }",
        &dependencies,
    )
    .unwrap();
    assert!(generated.contains("external::score"));
    for source in [
        "fn main() { application::score(1); }",
        "use application::score; fn main() { score(1); }",
        "fn main() { crate::internal::score(1); }",
    ] {
        let error = bindings::generate_for_dependencies(source, &dependencies).unwrap_err();
        assert!(
            error.contains("not an external dependency") || error.contains("unresolved path"),
            "{error}"
        );
    }
    // Inline application modules are already resolved as interpreted code.
    let generated = bindings::generate_for_dependencies(
        "mod internal { pub fn score(n: i64) -> i64 { n + 1 } } fn main() { internal::score(1); }",
        &dependencies,
    )
    .unwrap();
    assert!(!generated.contains("engine.register_fn"));
}
