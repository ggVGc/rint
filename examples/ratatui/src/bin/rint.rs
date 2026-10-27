include!(concat!(env!("OUT_DIR"), "/rint_bindings.rs"));

// Restore the terminal even if interpreted execution fails or unwinds.
struct RestoreTerminal;
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        terminal_ui::stop();
    }
}

fn main() -> Result<std::process::ExitCode, String> {
    let mut engine = rint::Engine::new();
    register_generated_bindings(&mut engine)?;
    let _restore = RestoreTerminal;
    let status = engine.run_cli(std::env::args())?;
    Ok(std::process::ExitCode::from(status as u8))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    #[test]
    fn interpreted_counter_handles_keys_and_quits() {
        let frames = Rc::new(RefCell::new(Vec::new()));
        let captured = frames.clone();
        let keys = RefCell::new(VecDeque::from(["+", "+", "-", "ignored", "q"]));
        let mut engine = rint::Engine::new();
        engine
            .register_fn("terminal_ui::draw", move |count: i64| {
                captured.borrow_mut().push(count);
                Ok::<(), String>(())
            })
            .unwrap();
        engine
            .register_fn("terminal_ui::read_key", move || {
                Ok::<String, String>(
                    keys.borrow_mut()
                        .pop_front()
                        .expect("script should quit")
                        .into(),
                )
            })
            .unwrap();
        engine
            .register_fn("terminal_ui::start", || Ok::<(), String>(()))
            .unwrap();
        engine.register_fn("terminal_ui::stop", || ()).unwrap();
        assert_eq!(engine.run_source(include_str!("../main.rs")).unwrap(), 0);
        assert_eq!(*frames.borrow(), [0, 1, 2, -3, -3]);
    }
}
