include!(concat!(env!("OUT_DIR"), "/rint_bindings.rs"));

fn main() -> Result<std::process::ExitCode, String> {
    let mut engine = rint::Engine::new();
    register_generated_bindings(&mut engine)?;
    let status = engine.run_cli(std::env::args())?;
    Ok(std::process::ExitCode::from(status as u8))
}
