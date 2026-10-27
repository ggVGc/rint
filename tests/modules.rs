use std::path::PathBuf;

struct Project(PathBuf);

impl Project {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rint-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        let project = Self(dir);
        for (name, contents) in files {
            project.write(name, contents);
        }
        project
    }

    fn write(&self, name: &str, contents: &str) {
        let file = self.0.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }

    fn main(&self) -> PathBuf {
        self.0.join("main.rs")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn loads_nested_file_modules_and_reads_edits_at_runtime() {
    let project = Project::new(&[
        (
            "main.rs",
            "mod flat; mod nested; fn main() { assert_eq!(flat::value(), 3); assert_eq!(nested::value(), 7); }",
        ),
        ("flat.rs", "pub fn value() -> i64 { 3 }"),
        (
            "nested/mod.rs",
            "mod child; pub fn value() -> i64 { child::value() }",
        ),
        ("nested/child.rs", "pub fn value() -> i64 { 7 }"),
    ]);
    let engine = rint::Engine::new();
    assert_eq!(engine.run_file(project.main()).unwrap(), 0);
    project.write("nested/child.rs", "pub fn value() -> i64 { 8 }");
    project.write(
        "main.rs",
        "mod nested; fn main() { assert_eq!(nested::value(), 8); }",
    );
    assert_eq!(engine.run_file(project.main()).unwrap(), 0);
    let source = std::fs::read_to_string(project.main()).unwrap();
    assert_eq!(
        rint::run(
            &source,
            project.main().to_str().unwrap(),
            vec![],
            Box::new(std::io::sink()),
            None
        )
        .unwrap(),
        0
    );
}

#[test]
fn module_names_and_import_aliases_have_separate_namespaces() {
    let source = r#"
        mod a {
            use dep_a::value as native;
            pub const N: i64 = 10;
            pub fn value() -> i64 { self::N + native() }
            pub fn main() { panic!("not the entry point"); }
            pub mod child {
                pub fn value() -> i64 { super::value() + crate::b::value() }
            }
        }
        mod b {
            use dep_b::value as native;
            pub const N: i64 = 20;
            pub fn value() -> i64 { N + native() }
        }
        use a::child::value as combined;
        fn main() {
            assert_eq!(a::value(), 11);
            assert_eq!(b::value(), 22);
            assert_eq!(combined(), 33);
        }
    "#;
    let mut engine = rint::Engine::new();
    engine.register_fn("dep_a::value", || 1_i64).unwrap();
    engine.register_fn("dep_b::value", || 2_i64).unwrap();
    assert_eq!(engine.run_source(source).unwrap(), 0);
    assert_eq!(
        rint::bindings::discover(source).unwrap(),
        ["dep_a::value", "dep_b::value"]
    );
}

#[test]
fn discovers_external_calls_in_interpreted_modules() {
    let project = Project::new(&[
        ("main.rs", "mod logic; fn main() { logic::run(); }"),
        (
            "logic.rs",
            "use dep::score as score; pub fn run() { score(42); }",
        ),
    ]);
    let source = std::fs::read_to_string(project.main()).unwrap();
    let parsed = rint::modules::parse(&source, &project.main(), &[]).unwrap();
    assert_eq!(parsed.files, [project.main(), project.0.join("logic.rs")]);
    let generated =
        rint::bindings::generate_items_for_dependencies(parsed.items, &["dep".into()]).unwrap();
    assert!(generated.contains("engine.register_fn(\"dep::score\", dep::score)?;"));
}

#[test]
fn native_modules_are_explicit_and_do_not_parse_native_implementation() {
    let project = Project::new(&[
        (
            "main.rs",
            "mod adapter; fn main() { assert_eq!(adapter::value(), 42); }",
        ),
        (
            "adapter.rs",
            "// This file need not be in the interpreted subset.\nmacro_rules! native_only { () => {} }",
        ),
    ]);
    let mut engine = rint::Engine::new();
    engine.register_fn("adapter::value", || 42_i64).unwrap();
    assert!(
        engine
            .run_file(project.main())
            .unwrap_err()
            .contains("macro_rules!")
    );
    engine.register_native_module("adapter");
    assert_eq!(engine.run_file(project.main()).unwrap(), 0);
}

#[test]
fn reports_missing_ambiguous_and_invalid_module_files() {
    let project = Project::new(&[("main.rs", "mod missing; fn main() {}")]);
    let engine = rint::Engine::new();
    let error = engine.run_file(project.main()).unwrap_err();
    assert!(
        error.contains("missing.rs") && error.contains("missing/mod.rs"),
        "{error}"
    );
    project.write("missing.rs", "pub fn value() {}");
    project.write("missing/mod.rs", "pub fn value() {}");
    assert!(
        engine
            .run_file(project.main())
            .unwrap_err()
            .contains("ambiguous module")
    );
    std::fs::remove_file(project.0.join("missing/mod.rs")).unwrap();
    project.write("missing.rs", "pub fn (");
    assert!(
        engine
            .run_file(project.main())
            .unwrap_err()
            .contains("missing.rs")
    );
    assert!(
        rint::compile("mod file;")
            .err()
            .unwrap()
            .contains("requires a source filename")
    );
}

#[test]
fn resolves_distinct_module_types_and_associated_items() {
    let source = r#"
        mod a {
            pub struct State { value: i64 }
            impl State {
                pub const BASE: i64 = 10;
                pub fn new() -> Self { Self { value: Self::BASE } }
                pub fn value(&self) -> i64 { self.value }
            }
            pub enum Event { Ready(i64), Done }
        }
        mod b {
            pub struct State { value: i64 }
            impl State {
                pub fn new() -> Self { Self { value: 20 } }
                pub fn value(&self) -> i64 { self.value }
            }
        }
        use a::State as A;
        use b::State as B;
        fn main() {
            assert_eq!(A::new().value(), 10);
            assert_eq!(B::new().value(), 20);
            let event = a::Event::Ready(42);
            match event {
                a::Event::Ready(n) => assert_eq!(n, 42),
                a::Event::Done => panic!("wrong event"),
            }
        }
    "#;
    assert_eq!(rint::Engine::new().run_source(source).unwrap(), 0);
    assert!(
        rint::Engine::new()
            .compile("mod a {} fn value() {} fn main() { a::value(); }")
            .is_err()
    );
}

#[test]
fn loads_file_children_of_inline_modules() {
    let project = Project::new(&[
        (
            "main.rs",
            "mod inline { pub mod child; } fn main() { assert_eq!(inline::child::value(), 42); }",
        ),
        (
            "inline/child.rs",
            "mod leaf; pub fn value() -> i64 { leaf::value() }",
        ),
        ("inline/child/leaf.rs", "pub fn value() -> i64 { 42 }"),
    ]);
    assert_eq!(rint::Engine::new().run_file(project.main()).unwrap(), 0);
}

#[test]
fn resolves_module_reexports_chained_aliases_and_glob_imports() {
    let source = r#"
        mod a {
            pub mod inner {
                pub fn value() -> i64 { 42 }
            }
            pub use self::inner::value as exported;
            pub use dep::identity as native;
        }
        mod b {
            use crate::a::inner::*;
            pub fn result() -> i64 { value() }
        }
        use a as alias;
        use alias::exported as value;
        fn main() {
            assert_eq!(value(), 42);
            assert_eq!(a::exported(), 42);
            assert_eq!(b::result(), 42);
            assert_eq!(a::native::<i64>(42), 42);
        }
    "#;
    let mut engine = rint::Engine::new();
    engine
        .register_fn("dep::identity::<i64>", |n: i64| n)
        .unwrap();
    assert_eq!(engine.run_source(source).unwrap(), 0);
    assert_eq!(
        rint::bindings::discover(source).unwrap(),
        ["dep::identity::<i64>"]
    );
}
