//! Load Rust file modules before name resolution.
use crate::ast::Item;
use std::path::{Path, PathBuf};

pub struct Source {
    pub items: Vec<Item>,
    /// All source files read, including the entry point.
    pub files: Vec<PathBuf>,
}

/// Parse source relative to its filename. Explicit native modules are left to
/// the host; their contents are neither parsed nor interpreted.
pub fn parse(src: &str, file: &Path, native_modules: &[String]) -> Result<Source, String> {
    let items = crate::parser::parse_program(crate::lexer::tokenize(src)?)?;
    let mut source = Source {
        items: vec![],
        files: vec![file.to_path_buf()],
    };
    let directory = file.parent().unwrap_or_else(|| Path::new("."));
    source.items = expand(items, directory, "", native_modules, &mut source.files)?;
    Ok(source)
}

/// Locate a module using Rust's `name.rs` / `name/mod.rs` convention.
pub fn module_file(directory: &Path, name: &str) -> Result<PathBuf, String> {
    let flat = directory.join(format!("{name}.rs"));
    let nested = directory.join(name).join("mod.rs");
    match (flat.is_file(), nested.is_file()) {
        (true, false) => Ok(flat),
        (false, true) => Ok(nested),
        (true, true) => Err(format!(
            "ambiguous module `{name}`: both {} and {} exist",
            flat.display(),
            nested.display()
        )),
        (false, false) => Err(format!(
            "cannot find module `{name}`: expected {} or {}",
            flat.display(),
            nested.display()
        )),
    }
}

fn expand(
    items: Vec<Item>,
    directory: &Path,
    prefix: &str,
    native_modules: &[String],
    files: &mut Vec<PathBuf>,
) -> Result<Vec<Item>, String> {
    let mut result = Vec::new();
    for item in items {
        match item {
            Item::FileMod(name) => {
                let qualified = format!("{prefix}{name}");
                if native_modules.contains(&qualified) {
                    continue;
                }
                let file = module_file(directory, &name)?;
                let src = std::fs::read_to_string(&file)
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                let items = crate::lexer::tokenize(&src)
                    .and_then(crate::parser::parse_program)
                    .map_err(|e| format!("{}:{e}", file.display()))?;
                files.push(file.clone());
                let sub = expand(
                    items,
                    &directory.join(&name),
                    &format!("{qualified}::"),
                    native_modules,
                    files,
                )
                .map_err(|e| format!("{}: {e}", file.display()))?;
                result.push(Item::Mod(name, sub));
            }
            Item::Mod(name, sub) => {
                let sub = expand(
                    sub,
                    &directory.join(&name),
                    &format!("{prefix}{name}::"),
                    native_modules,
                    files,
                )?;
                result.push(Item::Mod(name, sub));
            }
            other => result.push(other),
        }
    }
    Ok(result)
}
