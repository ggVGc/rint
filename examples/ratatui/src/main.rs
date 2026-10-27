mod counter;
mod terminal_ui;

use terminal_ui::{draw, read_key, start, stop};

fn main() -> Result<(), String> {
    start()?;
    let result = run();
    stop();
    result
}

fn run() -> Result<(), String> {
    let mut counter = counter::Counter::new();
    loop {
        draw(counter.value())?;
        let key = read_key()?;
        if key == "q" {
            break;
        } else if key == "+" {
            counter.increment();
        } else if key == "-" {
            counter.decrement();
        }
    }
    Ok(())
}
