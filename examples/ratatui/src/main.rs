use terminal_ui::{draw, read_key, start, stop};

fn main() -> Result<(), String> {
    start()?;
    let result = run();
    stop();
    result
}

fn run() -> Result<(), String> {
    let mut count = 0;
    loop {
        draw(count)?;
        let key = read_key()?;
        if key == "q" {
            break;
        } else if key == "+" {
            count += 1;
        } else if key == "-" {
            count -= 5;
        }
    }
    Ok(())
}
