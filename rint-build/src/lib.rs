//! Build-script support for generating rint dependency bindings.
use std::{path::Path, process::Command};

/// Parse a script and generate registrations for external functions it uses.
/// Call from build.rs; reports the script as a Cargo rebuild input.
pub fn generate(path: impl AsRef<Path>) -> Result<Bindings, String> {
    generate_with_native_modules(path, &[])
}

/// Generate bindings with explicitly compiled, top-level application modules.
/// Each module must have a `mod name;` declaration in the application and expose
/// functions supported by rint's native boundary. The generated file includes
/// these modules in the runner as well as registering their referenced functions.
pub fn generate_with_native_modules(
    path: impl AsRef<Path>,
    native_modules: &[&str],
) -> Result<Bindings, String> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let declarations = rint::parser::parse_program(rint::lexer::tokenize(&source)?)?;
    for name in native_modules {
        if !declarations
            .iter()
            .any(|item| matches!(item, rint::ast::Item::FileMod(module) if module == name))
        {
            return Err(format!(
                "native module `{name}` must have a top-level `mod {name};` declaration"
            ));
        }
    }
    let native_names: Vec<String> = native_modules.iter().map(|name| name.to_string()).collect();
    let parsed = rint::modules::parse(&source, path, &native_names)?;
    for file in &parsed.files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let dependencies = external_dependencies()?;
    let mut prelude = String::new();
    for name in native_modules {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("invalid native module name `{name}`"));
        }
        let file =
            rint::modules::module_file(path.parent().unwrap_or_else(|| Path::new(".")), name)?;
        println!("cargo:rerun-if-changed={}", file.display());
        let file = file
            .canonicalize()
            .map_err(|e| format!("{}: {e}", file.display()))?;
        prelude.push_str(&format!(
            "#[path = {:?}]\nmod {name};\n",
            file.to_string_lossy()
        ));
    }
    let generated = rint::bindings::generate_items_with_native_modules(
        parsed.items,
        &dependencies,
        &native_names,
    )
    .map_err(|e| format!("{}:{e}", path.display()))?;
    prelude.push_str(&generated);
    Ok(Bindings { source: prelude })
}

/// Read dependency names from Cargo rather than treating arbitrary qualified
/// paths (including the host's own library) as native code.
fn external_dependencies() -> Result<Vec<String>, String> {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .ok_or("CARGO_MANIFEST_DIR is not set; call from build.rs")?;
    let manifest = Path::new(&manifest_dir).join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version=1",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cannot read Cargo dependency metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("invalid Cargo metadata: {e}"))?;
    dependency_names(&metadata, &manifest)
}

fn dependency_names(metadata: &serde_json::Value, manifest: &Path) -> Result<Vec<String>, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata has no packages")?;
    let package = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|p| Path::new(p) == manifest)
        })
        .ok_or_else(|| format!("Cargo metadata has no package for {}", manifest.display()))?;
    let dependencies = package["dependencies"]
        .as_array()
        .ok_or("Cargo metadata has no dependencies")?;
    Ok(dependencies
        .iter()
        .filter(|dep| dep["kind"].is_null())
        .filter_map(|dep| dep["rename"].as_str().or_else(|| dep["name"].as_str()))
        .map(|name| name.replace('-', "_"))
        .collect())
}

pub struct Bindings {
    source: String,
}

impl Bindings {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn write_to_out_dir(&self, filename: &str) -> Result<(), String> {
        let out = std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set; call from build.rs")?;
        let path = Path::new(&out).join(filename);
        std::fs::write(&path, &self.source).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_normal_external_dependencies_are_bindable() {
        let metadata = serde_json::json!({
            "packages": [{
                "name": "application",
                "manifest_path": "/project/Cargo.toml",
                "dependencies": [
                    { "name": "original", "rename": "alias", "kind": null },
                    { "name": "terminal-ui", "rename": null, "kind": null },
                    { "name": "test-helper", "rename": null, "kind": "dev" },
                    { "name": "rint-build", "rename": null, "kind": "build" }
                ]
            }]
        });
        assert_eq!(
            dependency_names(&metadata, Path::new("/project/Cargo.toml")).unwrap(),
            ["alias", "terminal_ui"]
        );
    }
}
